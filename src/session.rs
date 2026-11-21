//! Network-game sessions: every player's copy of one save point.
//!
//! In a network game each machine writes its own `NAME.SAV`/`NAME.SRV`. The
//! copies share the world but differ in whose character is kind 6 (the
//! owner) and in the header's owner details. All copies of one save point
//! carry the same checksum list in header `+0x78`, which the lobby compares
//! before it offers a saved game (`FUN_00521130`). Together with version,
//! city and in-game date, that list identifies a save point here.

use crate::ops::{self, KIND_HUMAN_LOCAL, KIND_HUMAN_REMOTE};
use crate::reader::fixed_cstr;
use crate::save::SaveFile;
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Identifies one save point across all players' copies.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SaveKey {
    pub version: u32,
    pub city: String,
    pub date: [u8; 14],
    pub checksums: [u32; 8],
}

impl SaveKey {
    pub fn of(s: &SaveFile) -> Self {
        let h = &s.world.header;
        SaveKey { version: h.version, city: h.name.clone(), date: s.world.globals.date, checksums: h.f78 }
    }
}

/// One player's copy: the `.SAV` and, if present, its `.SRV`.
pub struct Copy {
    pub sav: SaveFile,
    pub srv: Option<SaveFile>,
    /// Location relative to the scanned root (`None` for files added from
    /// elsewhere).
    pub rel: Option<PathBuf>,
    pub modified: Option<SystemTime>,
    /// The owner's character id (kind 6 in this copy).
    pub owner: Option<u16>,
    /// Owner name from the header.
    pub header_owner: String,
    /// Problems noticed while identifying the copy.
    pub notes: Vec<String>,
    /// Byte-identical copies found elsewhere; they get the same edits.
    pub twins: Vec<Copy>,
}

impl Copy {
    pub fn load(sav_path: &Path, root: Option<&Path>) -> Result<Self> {
        let sav = SaveFile::load(sav_path)?;
        if sav.is_snapshot() {
            bail!("{} is a snapshot (.SRV); select the .SAV", sav_path.display());
        }
        let mut notes = Vec::new();
        let srv = match sibling(sav_path, "SRV") {
            Some(p) => match SaveFile::load(&p) {
                Ok(s) if SaveKey::of(&s) == SaveKey::of(&sav) => Some(s),
                Ok(_) => {
                    notes.push(format!("{} belongs to a different save; ignored", p.display()));
                    None
                }
                Err(e) => {
                    notes.push(format!("{}: {e:#}", p.display()));
                    None
                }
            },
            None => {
                notes.push("no matching .SRV next to the .SAV".into());
                None
            }
        };
        let h = &sav.world.header;
        let header_owner = format!("{} {}", fixed_cstr(&h.f40), fixed_cstr(&h.f50)).trim().to_owned();
        let owners: Vec<u16> =
            sav.world.characters.list.iter().filter(|c| c.f02 == KIND_HUMAN_LOCAL).map(|c| c.id).collect();
        let owner = match owners.as_slice() {
            [one] => Some(*one),
            [] => {
                // Owner already handed to the AI: fall back to the header name.
                let by_name = sav.world.characters.list.iter().find(|c| ops::full_name(c) == header_owner).map(|c| c.id);
                notes.push(format!("no character of kind 6; owner taken from header name ({header_owner})"));
                by_name
            }
            many => {
                notes.push(format!("{} characters of kind 6 (hot-seat save?)", many.len()));
                many.first().copied()
            }
        };
        if let Some(o) = owner
            && let Ok(c) = sav.character(o)
            && ops::full_name(c) != header_owner
        {
            notes.push(format!("header names {header_owner}, kind-6 character is {}", ops::full_name(c)));
        }
        let rel = root.and_then(|r| sav_path.strip_prefix(r).ok()).map(Path::to_path_buf);
        let modified = std::fs::metadata(sav_path).and_then(|m| m.modified()).ok();
        Ok(Copy { sav, srv, rel, modified, owner, header_owner, notes, twins: Vec::new() })
    }

    pub fn key(&self) -> SaveKey {
        SaveKey::of(&self.sav)
    }

    pub fn owner_name(&self) -> String {
        self.owner.and_then(|o| self.sav.character(o).ok()).map(ops::full_name).unwrap_or_else(|| self.header_owner.clone())
    }

    pub fn files(&self) -> impl Iterator<Item = &SaveFile> {
        std::iter::once(&self.sav).chain(self.srv.iter())
    }
}

/// `NAME.SAV` → `NAME.SRV` (any case), if it exists.
fn sibling(path: &Path, ext: &str) -> Option<PathBuf> {
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_str()?;
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case(stem))
            && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext))
    })
}

/// All `.SAV` files below `root`.
pub fn find_saves(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("sav")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// A save point found in a scan, with every copy that belongs to it.
pub struct Found {
    pub key: SaveKey,
    pub copies: Vec<Copy>,
}

impl Found {
    pub fn name(&self) -> &str {
        &self.copies[0].sav.world.header.description
    }

    pub fn newest(&self) -> Option<SystemTime> {
        self.copies.iter().filter_map(|c| c.modified).max()
    }
}

pub struct ScanResult {
    pub found: Vec<Found>,
    /// Files that could not be read.
    pub errors: Vec<String>,
}

/// Scan a folder tree and group the saves by save point.
pub fn scan(root: &Path, mut progress: impl FnMut(usize, usize)) -> ScanResult {
    let paths = find_saves(root);
    let mut groups: BTreeMap<SaveKey, Vec<Copy>> = BTreeMap::new();
    let mut errors = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        progress(i, paths.len());
        match Copy::load(p, Some(root)) {
            Ok(c) => groups.entry(c.key()).or_default().push(c),
            Err(e) => errors.push(format!("{}: {e:#}", p.display())),
        }
    }
    let mut found: Vec<Found> = groups.into_iter().map(|(key, copies)| Found { key, copies }).collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.newest()));
    ScanResult { found, errors }
}

/// A human player of the session and the copy found for them.
pub struct Seat {
    pub id: u16,
    pub name: String,
    pub dynasty: String,
    /// Kind in the reference copy (6 or 7; 5 if already handed to the AI).
    pub kind: u8,
    /// Index into `Session::copies`.
    pub copy: Option<usize>,
}

/// One save point with the copies loaded so far.
pub struct Session {
    pub key: SaveKey,
    pub root: Option<PathBuf>,
    pub copies: Vec<Copy>,
    /// Copies of an already loaded player that differ from it; not used.
    pub conflicts: Vec<(PathBuf, String)>,
}

impl Session {
    pub fn new(found: Found, root: Option<PathBuf>) -> Self {
        let mut s = Session { key: found.key, root, copies: Vec::new(), conflicts: Vec::new() };
        let mut copies = found.copies;
        // Prefer the newest file when a player's copy appears more than once.
        copies.sort_by_key(|c| std::cmp::Reverse(c.modified));
        for c in copies {
            let path = c.sav.path.clone();
            if let Err(e) = s.add(c) {
                s.conflicts.push((path, format!("{e:#}")));
            }
        }
        s
    }

    pub fn reference(&self) -> &SaveFile {
        &self.copies[0].sav
    }

    pub fn name(&self) -> &str {
        &self.reference().world.header.description
    }

    /// Every human player (kind 6 or 7 in any copy), plus players already
    /// handed to the AI whose own copy is loaded.
    pub fn seats(&self) -> Vec<Seat> {
        let r = self.reference();
        let mut seats: Vec<Seat> = r
            .world
            .characters
            .list
            .iter()
            .filter(|c| {
                ops::is_human(c)
                    || self.copies.iter().any(|cp| cp.owner == Some(c.id))
                    || self.copies.iter().any(|cp| cp.sav.character(c.id).is_ok_and(ops::is_human))
            })
            .map(|c| Seat {
                id: c.id,
                name: ops::full_name(c),
                dynasty: ops::dynasty_name(r, c),
                kind: c.f02,
                copy: self.copies.iter().position(|cp| cp.owner == Some(c.id)),
            })
            .collect();
        seats.sort_by_key(|s| s.id);
        seats
    }

    /// Check that a copy belongs to this save point and to a player that has
    /// no copy yet.
    pub fn check(&self, c: &Copy) -> Result<()> {
        if c.key() != self.key {
            let h = &c.sav.world.header;
            bail!(
                "{} is a different save ({} / {}, {}), not this one",
                c.sav.path.display(),
                h.description,
                h.name,
                date_text(&c.sav)
            );
        }
        if let Some(first) = self.copies.first() {
            let ids = |s: &SaveFile| -> Vec<(u16, String)> {
                s.world.characters.list.iter().filter(|c| ops::is_player(c)).map(|c| (c.id, ops::full_name(c))).collect()
            };
            if ids(&first.sav) != ids(&c.sav) {
                bail!("{}: the players differ from the other copies", c.sav.path.display());
            }
        }
        let Some(owner) = c.owner else {
            bail!("{}: cannot tell which player this copy belongs to", c.sav.path.display());
        };
        if let Some(have) = self.copies.iter().find(|x| x.owner == Some(owner)) {
            if same_files(have, c) {
                bail!(
                    "{}: same file as the already loaded copy of {} ({})",
                    c.sav.path.display(),
                    c.owner_name(),
                    have.sav.path.display()
                );
            }
            bail!(
                "{}: a different copy of {} is already loaded ({})",
                c.sav.path.display(),
                c.owner_name(),
                have.sav.path.display()
            );
        }
        Ok(())
    }

    /// Add a copy. A byte-identical copy of an already loaded player is
    /// kept as a twin (edited along with it) and reported as `Ok(false)`.
    pub fn add(&mut self, c: Copy) -> Result<bool> {
        if self.all_copies().any(|x| x.sav.path == c.sav.path) {
            bail!("{} is already loaded", c.sav.path.display());
        }
        if c.key() == self.key
            && let Some(have) = self.copies.iter_mut().find(|x| x.owner.is_some() && x.owner == c.owner && x.sav.raw == c.sav.raw)
        {
            have.twins.push(c);
            return Ok(false);
        }
        self.check(&c)?;
        self.copies.push(c);
        Ok(true)
    }

    /// Load and add a single save file; returns a description of the result.
    pub fn add_file(&mut self, path: &Path) -> Result<String> {
        let root = self.root.clone();
        let c = Copy::load(path, root.as_deref().filter(|r| path.starts_with(r)))?;
        let name = c.owner_name();
        Ok(if self.add(c)? {
            format!("added copy of {name}")
        } else {
            format!("identical to the loaded copy of {name}; it will be edited too")
        })
    }

    /// Add the copies of this save found by scanning a folder; returns one
    /// log line per file (copies of other saves are summarised).
    pub fn add_scanned(&mut self, res: ScanResult) -> Vec<String> {
        let mut log: Vec<String> = res.errors.into_iter().map(|e| format!("error: skipped {e}")).collect();
        let mut other = Vec::new();
        let mut added = 0;
        for f in res.found {
            if f.key != self.key {
                other.push(format!("“{}” ({}, {} file(s))", f.name(), date_text(&f.copies[0].sav), f.copies.len()));
                continue;
            }
            for c in f.copies {
                let (path, name) = (c.sav.path.clone(), c.owner_name());
                match self.add(c) {
                    Ok(true) => {
                        added += 1;
                        log.push(format!("{}: added copy of {name}", path.display()));
                    }
                    Ok(false) => log.push(format!("{}: identical to the loaded copy of {name}; it will be edited too", path.display())),
                    Err(e) => log.push(format!("error: {e:#}")),
                }
            }
        }
        if !other.is_empty() {
            log.push(format!("ignored copies of other saves: {}", other.join(", ")));
        }
        log.push(format!("{added} new player cop(ies) added"));
        log
    }

    /// Add a file, or every matching copy below a folder.
    pub fn add_path(&mut self, path: &Path) -> Result<Vec<String>> {
        if path.is_dir() {
            Ok(self.add_scanned(scan(path, |_, _| {})))
        } else {
            Ok(vec![format!("{}: {}", path.display(), self.add_file(path)?)])
        }
    }

    /// Every copy including twins.
    pub fn all_copies(&self) -> impl Iterator<Item = &Copy> {
        self.copies.iter().flat_map(|c| std::iter::once(c).chain(c.twins.iter()))
    }

    /// Hand the given characters to the AI in every loaded copy and write
    /// the results. `out_dir = None` edits in place (keeping `.bak` files).
    pub fn apply(&mut self, to_ai: &[u16], keep_notices: bool, out_dir: Option<&Path>) -> Result<Vec<String>> {
        if to_ai.is_empty() {
            bail!("no player selected");
        }
        let mut log = Vec::new();
        // Edit everything in memory first; write only if all edits succeed.
        let mut edited: Vec<(SaveFile, PathBuf)> = Vec::new();
        for c in self.all_copies() {
            for f in c.files() {
                let mut f = SaveFile { path: f.path.clone(), raw: f.raw.clone(), world: f.world.clone() };
                for &id in to_ai {
                    if f.character(id).is_ok_and(ops::is_human) {
                        log.extend(ops::to_ai(&mut f, id, keep_notices).with_context(|| format!("editing {}", f.path.display()))?);
                    }
                }
                let dst = match out_dir {
                    None => f.path.clone(),
                    Some(d) => self.output_path(c, &f.path, d),
                };
                edited.push((f, dst));
            }
        }
        for (f, dst) in &edited {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            log.extend(ops::write_with_backup(f, dst)?);
        }
        // Continue working on the edited state.
        let mut it = edited.into_iter();
        let mut update = |c: &mut Copy| {
            let (sav, dst) = it.next().unwrap();
            c.sav = SaveFile { path: dst, ..sav };
            if c.srv.is_some() {
                let (srv, dst) = it.next().unwrap();
                c.srv = Some(SaveFile { path: dst, ..srv });
            }
        };
        for c in &mut self.copies {
            update(c);
            for t in &mut c.twins {
                update(t);
            }
        }
        Ok(log)
    }

    /// Mirror the scanned tree below `out`; files added from elsewhere go to
    /// `out/added/<owner>/`.
    fn output_path(&self, c: &Copy, file: &Path, out: &Path) -> PathBuf {
        let name = file.file_name().unwrap_or_default();
        match (&self.root, &c.rel) {
            (Some(root), Some(_)) if file.starts_with(root) => out.join(file.strip_prefix(root).unwrap()),
            _ => out.join("added").join(sanitize(&c.owner_name())).join(name),
        }
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect()
}

fn same_files(a: &Copy, b: &Copy) -> bool {
    a.sav.raw == b.sav.raw
}

pub fn date_text(s: &SaveFile) -> String {
    let (round, year, hour, minute) = ops::date(s);
    format!("{year} (round {round}) {hour:02}:{minute:02}")
}

pub fn time_text(t: Option<SystemTime>) -> String {
    t.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_else(|| "?".into())
}

/// Role of a seat, independent of whose copy is the reference.
pub fn seat_role(kind: u8) -> &'static str {
    if is_human_kind(kind) { "human player" } else { "AI player" }
}

/// Kinds that mark a human seat.
pub fn is_human_kind(k: u8) -> bool {
    matches!(k, KIND_HUMAN_LOCAL | KIND_HUMAN_REMOTE)
}
