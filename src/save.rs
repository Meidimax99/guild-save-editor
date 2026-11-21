//! Loading, locating and writing savegame data.
//!
//! A `.SAV`/`.SRV` is a gzip stream. Its decompressed content starts with the
//! world state parsed by [`crate::world`] (header, globals, objects, buildings,
//! dynasties, characters, cities, offices), followed by the embedded city
//! scene and, in `.SAV` files only, the dynamic simulation state. Edits patch
//! the decompressed bytes in place (or splice records out of the dynamic
//! state) and recompress; everything not touched is written back verbatim.

use crate::reader::Reader;
use crate::world::{Character, WorldFile};
use anyhow::{Context, Result, bail, ensure};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub struct SaveFile {
    pub path: PathBuf,
    pub raw: Vec<u8>,
    pub world: WorldFile,
}

/// A pending notice / message box in the dynamic state (`FUN_0050de20`,
/// 0x14c bytes in memory at `0x726d00`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Notice {
    pub offset: usize,
    pub len: usize,
    pub kind: u8,
    /// Character slot the notice belongs to (`+0x08`).
    pub character: u16,
    pub text: String,
}

/// Location of the notice section inside the decompressed data.
pub struct NoticeSection {
    pub count_offset: usize,
    pub notices: Vec<Notice>,
}

/// Notice kinds the engine cancels when it hands a human's character to the
/// AI (`FUN_00527c60`, the "player left" handler).
pub const AI_CANCELLED_NOTICES: [u8; 14] =
    [0x5d, 0x11, 0x30, 0x35, 0x42, 0x60, 0x4b, 0x5b, 0x5c, 0x5a, 0x46, 0x2e, 0x2d, 0x41];

impl SaveFile {
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        ensure!(data.starts_with(&[0x1f, 0x8b]), "{} is not a gzip-compressed savegame", path.display());
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(&data[..]).read_to_end(&mut raw).context("decompressing")?;
        let world = WorldFile::parse(&raw).with_context(|| format!("parsing {}", path.display()))?;
        Ok(SaveFile { path: path.to_owned(), raw, world })
    }

    pub fn version(&self) -> u32 {
        self.world.header.version
    }

    /// `.SRV` snapshots carry only world state and scene.
    pub fn is_snapshot(&self) -> bool {
        self.world.header.is_city()
    }

    pub fn character(&self, id: u16) -> Result<&Character> {
        self.world
            .characters
            .list
            .iter()
            .find(|c| c.id == id)
            .with_context(|| format!("no character with id {id} in {}", self.path.display()))
    }

    /// Re-parse after an edit so offsets stay valid.
    pub fn reparse(&mut self) -> Result<()> {
        self.world = WorldFile::parse(&self.raw).context("re-parsing edited data")?;
        Ok(())
    }

    /// File offset of header byte `+0x31`: the number of human players in a
    /// network game (`DAT_0064db2c` when saving; the host waits for this many
    /// players when the save is loaded, `FUN_00520e00`).
    pub const HUMAN_COUNT_OFFSET: usize = 4 + 1 + 0x20 + 8 + 1;

    pub fn header_human_count(&self) -> u8 {
        self.raw[Self::HUMAN_COUNT_OFFSET]
    }

    pub fn set_header_human_count(&mut self, n: u8) {
        self.raw[Self::HUMAN_COUNT_OFFSET] = n;
    }

    /// Turn this copy into the copy of another human player (`target`): what
    /// the game would have written on that player's machine. Patches the
    /// character kinds (target 6, other humans 7), the local player slot and
    /// the header owner fields (name, profession, religion, coat of arms,
    /// gender; selection cleared). Header wealth (`f68`, display only) keeps
    /// the source value. Requires save version > 0x10053 (header layout).
    pub fn make_owner(&mut self, target: u16) -> Result<()> {
        ensure!(self.version() > 0x10053, "unsupported save version {:#x}", self.version());
        let t = self.character(target)?.clone();
        ensure!(matches!(t.f02, 6 | 7), "character {target} is not a human player");
        let humans: Vec<(usize, u16)> =
            self.world.characters.list.iter().filter(|c| matches!(c.f02, 6 | 7)).map(|c| (c.kind_offset, c.id)).collect();
        for (off, id) in humans {
            self.raw[off] = if id == target { 6 } else { 7 };
        }
        let slot = self.world.characters.g71da90_offset;
        self.raw[slot..slot + 2].copy_from_slice(&target.to_le_bytes());
        // Header owner fields (file offsets for v > 0x10053).
        let h = &mut self.raw;
        h[0x3d..0x4d].copy_from_slice(&t.f30);
        h[0x4d..0x5d].copy_from_slice(&t.f40);
        h[0x5d] = t.f0c;
        h[0x5e..0x62].copy_from_slice(&(t.f54 as u32).to_le_bytes());
        h[0x62] = t.f164[0];
        h[0x63] = t.f164[2];
        h[0x64] = t.f0d;
        h[0x69] = t.f09;
        h[0x6b..0x73].fill(0xff);
        self.reparse()?;
        // Read back to make sure the offsets hit the intended fields.
        let hd = &self.world.header;
        ensure!(
            hd.f40 == t.f30.to_vec() && hd.f50 == t.f40.to_vec() && hd.f63 == t.f0c && self.world.characters.g71da90 == target,
            "header layout check failed"
        );
        Ok(())
    }

    pub fn set_kind(&mut self, id: u16, kind: u8) -> Result<u8> {
        let c = self.character(id)?;
        let (off, old) = (c.kind_offset, c.f02);
        self.raw[off] = kind;
        Ok(old)
    }

    /// Walk the dynamic state up to the notice section (sections 1–5 of the
    /// savegame tail: `FUN_004e0d00`, `FUN_0050d8e0`, `FUN_0050d9b0`,
    /// `FUN_004db440`, `FUN_0050de20`).
    pub fn notices(&self) -> Result<NoticeSection> {
        ensure!(!self.is_snapshot(), "snapshot (.SRV) files have no dynamic state");
        let v = self.version();
        let mut r = Reader::new(&self.raw);
        r.set_pos(self.world.dynamic_offset);
        // 1
        ensure!(r.u32()? == 0x1d, "unexpected dynamic-state layout (section 1 magic)");
        r.skip(30 * 4)?;
        let (n1, n2) = if v > 0x10040 { (r.u32()? as usize, r.u32()? as usize) } else { (0x80, 0x200) };
        ensure!(n1 <= 0x10000 && n2 <= 0x10000, "unexpected dynamic-state layout (section 1 counts)");
        r.skip(n1 * 41 + n2 * 8)?; // {u32, date[14], u32, u32, u16, u8, 3 × u32}
        // 2
        if v >= 0x10016 {
            let n = if v < 0x1003a { 0x100 } else if v > 0x10041 { 0x300 } else { 0x200 };
            r.skip(n * n * if v > 0x10035 { 2 } else { 1 })?;
        }
        // 3
        r.skip(25 * 4 + 2 + 16 * 5 * 4 + 5 * 0x44 + 64 * 9)?;
        // 4
        if v >= 0x1002a {
            let n = r.u32()? as usize;
            ensure!(n <= 256, "unexpected dynamic-state layout (avatar count {n})");
            r.skip(n * 5)?;
        }
        // 5
        let count_offset = r.pos();
        let n = r.u32()? as usize;
        let size = r.u32()? as usize;
        ensure!(n <= 0x1000 && size <= 0x1000, "unexpected dynamic-state layout (notices {n}, size {size})");
        let mut notices = Vec::with_capacity(n);
        for _ in 0..n {
            let offset = r.pos();
            let kind = r.u8()?;
            r.skip(4)?;
            let character = r.u16()?;
            r.skip(4 + 4 + 0x30 + 3 * 14 + 4 + 1)?;
            if v > 0x1002e {
                r.skip(32)?;
            }
            let len = r.u32()? as usize;
            ensure!(len <= 0x400, "unexpected dynamic-state layout (notice text {len})");
            let text = crate::reader::fixed_cstr(r.bytes(len)?);
            r.skip(size)?;
            notices.push(Notice { offset, len: r.pos() - offset, kind, character, text });
        }
        // Sanity check against the following lists so a layout mismatch is
        // detected before anything is spliced.
        for rec in [4 + 1 + 12 + 14 + 0x80, 1 + 12 + 0x40] {
            let n = r.u32()? as usize;
            ensure!(n <= 0x10000, "unexpected dynamic-state layout after notices");
            r.skip(n * rec)?;
        }
        let n6 = r.u32()?;
        ensure!(n6 <= 0x1000, "unexpected dynamic-state layout after notices ({n6})");
        Ok(NoticeSection { count_offset, notices })
    }

    /// Remove the given notices from the dynamic state.
    pub fn drop_notices(&mut self, section: &NoticeSection, drop: &[usize]) -> Result<()> {
        let mut drop = drop.to_vec();
        drop.sort_unstable();
        for &i in drop.iter().rev() {
            let n = &section.notices[i];
            self.raw.drain(n.offset..n.offset + n.len);
        }
        let remaining = (section.notices.len() - drop.len()) as u32;
        self.raw[section.count_offset..section.count_offset + 4].copy_from_slice(&remaining.to_le_bytes());
        Ok(())
    }

    pub fn write(&self, out: &Path) -> Result<()> {
        let tmp = out.with_extension("tmp-edit");
        {
            let f = std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
            let mut gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            gz.write_all(&self.raw)?;
            gz.finish()?.sync_all()?;
        }
        // Verify the result loads before replacing anything.
        let check = SaveFile::load(&tmp)?;
        if check.raw != self.raw {
            bail!("verification of {} failed", tmp.display());
        }
        std::fs::rename(&tmp, out)?;
        Ok(())
    }
}

/// The `.SAV`/`.SRV` pair belonging to `path` (whichever exist).
pub fn companions(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_owned()];
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let other = match ext.to_ascii_uppercase().as_str() {
        "SAV" => Some(if ext == "SAV" { "SRV" } else { "srv" }),
        "SRV" => Some(if ext == "SRV" { "SAV" } else { "sav" }),
        _ => None,
    };
    if let Some(o) = other {
        let p = path.with_extension(o);
        if p.exists() {
            out.push(p);
        }
    }
    out
}
