//! World state: city files (`gamedata/Cities/*.cty`) and the first part of
//! savegames. Reader: `FUN_0050abf0` (savegame/city load) and its sections.
//!
//! The engine serialises its global arrays field by field. Field names below are
//! the in-memory offsets of each record (`fXX`) until their meaning is known;
//! known meanings are documented on the field.
//!
//! After the offices section a city/savegame continues with an embedded `.ed3`
//! scene (only its extent is needed here) and, for `.SAV`, the dynamic
//! simulation state (see `save.rs`).

use crate::error::{Error, Result};
use crate::reader::{Reader, fixed_cstr};
use crate::scene::Scene;

#[derive(Debug, Clone, Default)]
pub struct Header {
    pub version: u32,
    /// Bit 1 (`& 2`): city file (no dynamic state follows).
    pub flags: u8,
    pub name: String,
    pub f28: [u8; 8],
    pub f30: u8,
    pub f31: u8,
    pub f32: Vec<u8>,
    pub f40: Vec<u8>,
    pub f50: Vec<u8>,
    pub f63: u8,
    pub f64: u32,
    pub f60: [u8; 3],
    pub f68: u32,
    pub f6c: [u8; 2],
    pub f70: [i32; 2],
    pub f78: [u32; 8],
    /// 160×120 BGR thumbnail.
    pub thumbnail: Vec<u8>,
    pub f98: u32,
    pub description: String,
}

impl Header {
    pub fn is_city(&self) -> bool {
        self.flags & 2 != 0
    }
}

#[derive(Debug, Clone, Default)]
pub struct Globals {
    pub g6438e4: u32,
    pub g71daf0: u32,
    /// 14 bytes at `0x79cca0`: current date/time block.
    pub date: [u8; 14],
    pub g71d095: u8,
    pub g71d0c8: u32,
    pub g71d088: u32,
    pub g71d08c: u32,
    pub g71d090: u32,
    pub g71d09c: u32,
    pub gfb5740: [u8; 0x18],
    pub g6438e8: u32,
    pub g71d058: u32,
}

/// Object instance (`f3_gm_lpObjekteINST`, 0x43 bytes in memory).
#[derive(Debug, Clone, Default)]
pub struct ObjectInstance {
    pub f00: u16,
    pub f02: u32,
    pub f06: u32,
    pub f0a: u32,
    pub f0e: u32,
    pub f12: u8,
    pub f13: u8,
    pub f1c: [u8; 31],
}

#[derive(Debug, Clone)]
pub enum BuildingExtra {
    Raw([u8; 0x30]),
    /// Type 0x1e: 64 plant cells (`f3g_PlantMap`).
    Plants(Vec<PlantCell>),
    /// Types 'H'/'I'/'J': graveyard (`gv_AllocGraveyardField`).
    Graveyard(Graveyard),
    /// Types 'K'/'L'/'M' (`ahm_AllocAhmInfo`).
    Ahm(AhmInfo),
}

#[derive(Debug, Clone, Default)]
pub struct PlantCell {
    pub f00: u32,
    pub f04: u32,
    pub f08: u8,
    pub f09: u8,
    pub f0a: u16,
    pub f10: u32,
    pub f0c: u8,
    pub f0d: u8,
}

#[derive(Debug, Clone, Default)]
pub struct Graveyard {
    pub f00: u32,
    pub kind: u32,
    pub f18: [u8; 4],
    pub zombies: Vec<Zombie>,
    pub graves: Vec<Grave>,
}

#[derive(Debug, Clone, Default)]
pub struct Zombie {
    pub f00: u32,
    pub f04: [u8; 16],
    pub f14: [u8; 16],
    pub f24: [u8; 5],
    pub f2c: [u32; 6],
}

#[derive(Debug, Clone, Default)]
pub struct Grave {
    pub f00: u32,
    pub f04: u32,
    pub f08: u16,
    pub f0a: u16,
    pub f0c: u8,
    pub zombie: u32,
}

#[derive(Debug, Clone, Default)]
pub struct AhmInfo {
    pub f00: u32,
    pub f04: u32,
    pub f08: [u8; 4],
    pub pairs: [(u32, u32); 6],
}

/// Building instance (`f3_gm_lpGebaeudeINST`, 0xa9 bytes in memory).
#[derive(Debug, Clone)]
pub struct BuildingInstance {
    /// Prototype index into `A_Geb.dat`.
    pub proto: u8,
    pub f01: u32,
    pub name: String,
    pub f25: u16,
    pub f27: u16,
    pub f29: u16,
    pub f2b: u32,
    pub f2f: u8,
    pub f34: u8,
    pub f35: u32,
    pub f39: u32,
    pub f3d: u32,
    pub f41: u32,
    pub f45: u32,
    pub f49: u32,
    pub f4d: [u32; 3],
    pub f5a: u16,
    pub f5c: u8,
    pub extra: BuildingExtra,
    pub f99: [u8; 16],
}

#[derive(Debug, Clone)]
pub struct BuildingLabel {
    pub name: String,
    pub a: [u8; 16],
    pub b: [u8; 16],
}

/// 16 records of 0xac bytes at `0x802140`.
#[derive(Debug, Clone, Default)]
pub struct Dynasty {
    pub id: u16,
    pub f02: [u8; 16],
    pub f14: u32,
    pub f28: u32,
    pub f1c: u32,
    pub f20: u32,
    pub f2c: u32,
    pub f30: u32,
    pub f34: u32,
    pub f38: u32,
    pub f40: u32,
    pub f44: u32,
    pub f48: u32,
    pub f4c: u32,
    pub f50: u32,
    pub ext: [u32; 7],
    pub f60: f32,
    pub f74: Vec<u8>,
    pub f88: u32,
    pub f8c: [u8; 16],
    pub f9c: [u8; 12],
    pub fa8: u32,
    pub f70: u32,
}

/// Character record (0x21c bytes in memory at `0x79cd40`).
#[derive(Debug, Clone, Default)]
pub struct Character {
    /// File offset of this record (decompressed save).
    pub offset: usize,
    /// File offset of the kind byte (`+0x02`).
    pub kind_offset: usize,
    /// File offset of the 8 × u32 slots at `+0x5c`.
    pub f5c_offset: usize,
    pub id: u16,
    pub f20c: i32,
    pub f02: u8,
    pub f04: u32,
    pub f08: u8,
    pub f09: u8,
    pub f0a: u16,
    pub f0c: u8,
    pub f0d: u8,
    pub f10: u32,
    pub f14: u32,
    pub f18: u32,
    pub f1c: u32,
    pub f20: u32,
    pub f24: u32,
    pub f28: u16,
    pub f2c: u32,
    pub f30: [u8; 16],
    pub f40: [u8; 16],
    pub f50: u16,
    /// Stored minus 0x59a in the file (`+= 0x59a` on load).
    pub f54: u32,
    pub f58: u8,
    pub f5c: [u8; 32],
    pub f7c: u32,
    pub f80: [u8; 5],
    pub f88: Vec<u8>,
    pub f164: [u8; 6],
    pub f16c: u32,
    pub f170: u32,
    pub f174: u32,
    pub f17c: u32,
    pub f180: u8,
    pub f184: u32,
    /// Stored minus 0x639 in the file.
    pub f18c: u32,
    pub f190: u32,
    pub f194: u32,
    pub f198: u32,
    pub f19c: u32,
    pub f1a0: u32,
    pub f1a4: u32,
    pub f1a8: u32,
    pub f1ac: u32,
    pub f1b0: u8,
    pub f1b1: u8,
    pub f1b4: [u8; 16],
    pub f1c8: u32,
    pub f1cc: u32,
    pub f1d0: u32,
    pub f1d4: [u8; 16],
    pub f1e4: u32,
    pub f1e8: u32,
    pub f1ec: u32,
    pub f1f0: u32,
    pub f1c5: u8,
    pub f184b: u32,
    pub f1f4: [u8; 0x18],
    pub f210: u32,
    pub f214: [u8; 5],
}

#[derive(Debug, Clone, Default)]
pub struct Characters {
    pub g71da90: u16,
    pub g71d0cc: u32,
    pub g71d0d0: u32,
    pub g71d0d4: [u32; 8],
    pub list: Vec<Character>,
}

#[derive(Debug, Clone, Default)]
pub struct MarketEntry {
    pub f00: u16,
    pub f04: u32,
    pub f08: u32,
    pub f0c: u32,
    pub f18: u32,
    pub f1c: u32,
    pub f28: u32,
    pub f2c: u32,
    pub f30: u32,
    pub f34: u16,
    pub f24: u32,
}

/// Five records at `0x802c20` (stride 0x2a90), one entry per ware.
#[derive(Debug, Clone, Default)]
pub struct Market {
    pub head: [u32; 4],
    pub wares: Vec<MarketEntry>,
}

/// Four records at `0x810100` (stride 0x2f4): the city and its neighbours.
#[derive(Debug, Clone, Default)]
pub struct CityInfo {
    pub name: String,
    pub f20: [u8; 8],
    pub f28: u8,
    pub f2c: u32,
    pub f30: u32,
    pub f34: u16,
    pub f38: [u8; 8],
    pub f40: u8,
    pub f41: u8,
    pub f44: Vec<[u8; 8]>,
    pub f94: Vec<[u8; 8]>,
    pub fe4: u8,
    pub fe5: u8,
    pub fe8: [u32; 4],
    pub ff8: [u32; 4],
    pub f108: u32,
    pub f10c: [u8; 11],
    pub f117: u8,
    pub f118: [u8; 7],
    pub f11f: [u8; 4],
    pub f124: Vec<[u8; 18]>,
    pub f1b4: u32,
    pub f1b8: u32,
    pub f1bc: Vec<u8>,
    pub f2cc: [u8; 8],
}

#[derive(Debug, Clone, Default)]
pub struct Office {
    pub a: u8,
    pub b: u32,
    pub c: u8,
    pub d: u32,
    pub e: u8,
    pub f: u32,
}

#[derive(Debug, Clone)]
pub struct WorldFile {
    pub header: Header,
    pub globals: Globals,
    pub objects: Vec<ObjectInstance>,
    pub buildings: Vec<BuildingInstance>,
    pub building_labels: Vec<BuildingLabel>,
    pub dynasties: Vec<Dynasty>,
    pub characters: Characters,
    pub markets: Vec<Market>,
    pub cities: Vec<CityInfo>,
    pub offices: Vec<Office>,
    /// Offset of the embedded city scene.
    pub tail_offset: usize,
    /// Offset where the embedded scene ends (start of the savegame-only
    /// dynamic state; equals the file size for cities and `.SRV` files).
    pub dynamic_offset: usize,
}

fn arr<const N: usize>(r: &mut Reader) -> Result<[u8; N]> {
    Ok(r.bytes(N)?.try_into().unwrap())
}

fn read_header(r: &mut Reader) -> Result<Header> {
    let mut h = Header { version: r.u32()?, ..Default::default() };
    let v = h.version;
    if !(0x10026..0x1005b).contains(&v) {
        return Err(Error::Invalid(format!("unsupported world version {v:#x}")));
    }
    h.flags = r.u8()?;
    h.name = fixed_cstr(r.bytes(0x20)?);
    h.f28 = arr(r)?;
    h.f30 = r.u8()?;
    if v > 0x10032 {
        h.f31 = r.u8()?;
    }
    if v > 0x10027 {
        h.f32 = r.bytes(14)?.to_vec();
        h.f40 = r.bytes(16)?.to_vec();
        if v > 0x10053 {
            h.f50 = r.bytes(16)?.to_vec();
            h.f63 = r.u8()?;
            h.f64 = r.u32()?;
        }
        h.f60 = arr(r)?;
        h.f68 = r.u32()?;
        h.f6c = arr(r)?;
        if v >= 0x1002b {
            h.f70 = [r.i32()?, r.i32()?];
        }
        if v > 0x10033 {
            for i in 0..8 {
                h.f78[i] = r.u32()?;
            }
        }
        h.thumbnail = r.bytes(0xe100)?.to_vec();
    }
    h.f98 = if v >= 0x10038 { r.u32()? } else { 2 };
    h.description = if v >= 0x10039 { fixed_cstr(r.bytes(0x60)?) } else { "Savegame".into() };
    Ok(h)
}

fn read_globals(r: &mut Reader, v: u32) -> Result<Globals> {
    let mut g = Globals {
        g6438e4: r.u32()?,
        g71daf0: r.u32()?,
        date: arr(r)?,
        g71d095: r.u8()?,
        g71d0c8: r.u32()?,
        g71d088: r.u32()?,
        g71d08c: r.u32()?,
        g71d090: r.u32()?,
        ..Default::default()
    };
    if v > 0x10021 {
        g.g71d09c = r.u32()?;
    }
    g.gfb5740 = arr(r)?;
    if v > 0x1002f {
        g.g6438e8 = r.u32()?;
    }
    g.g71d058 = if v >= 0x1003d { r.u32()? } else { 1_000_000 };
    Ok(g)
}

fn read_objects(r: &mut Reader) -> Result<Vec<ObjectInstance>> {
    let n = r.i32()?;
    (0..n.max(0))
        .map(|_| {
            Ok(ObjectInstance {
                f00: r.u16()?,
                f02: r.u32()?,
                f06: r.u32()?,
                f0a: r.u32()?,
                f0e: r.u32()?,
                f12: r.u8()?,
                f13: r.u8()?,
                f1c: arr(r)?,
            })
        })
        .collect()
}

const GRAVE_COUNTS: [usize; 8] = [0, 12, 20, 30, 0, 16, 32, 64];
const ZOMBIE_COUNTS: [usize; 8] = [0, 16, 32, 64, 0, 8, 12, 16];

fn read_buildings(r: &mut Reader, v: u32) -> Result<(Vec<BuildingInstance>, Vec<BuildingLabel>)> {
    let n = r.i32()?;
    let mut out = Vec::with_capacity(n.max(0) as usize);
    for _ in 0..n.max(0) {
        let proto = r.u8()?;
        let f01 = r.u32()?;
        let name = fixed_cstr(r.bytes(0x20)?);
        let (f25, f27, f29) = (r.u16()?, r.u16()?, r.u16()?);
        let f2b = r.u32()?;
        let f2f = r.u8()?;
        let f34 = r.u8()?;
        let (f35, f39, f3d) = (r.u32()?, r.u32()?, r.u32()?);
        let mut f41 = r.u32()?;
        if f41 > 4 {
            f41 = 2;
        }
        let f45 = r.u32()?;
        let f49 = if v > 0x10027 { r.u32()? } else { 0 };
        if v < 0x10032 {
            r.u32()?;
        }
        let mut f4d = [0; 3];
        if v > 0x10056 {
            for x in &mut f4d {
                *x = r.u32()?;
            }
        }
        let f5a = r.u16()?;
        let f5c = r.u8()?;
        let extra = match proto {
            0x1e => BuildingExtra::Plants(
                (0..64)
                    .map(|_| {
                        Ok(PlantCell {
                            f00: r.u32()?,
                            f04: r.u32()?,
                            f08: r.u8()?,
                            f09: r.u8()?,
                            f0a: r.u16()?,
                            f10: r.u32()?,
                            f0c: r.u8()?,
                            f0d: r.u8()?,
                        })
                    })
                    .collect::<Result<_>>()?,
            ),
            b'H' | b'I' | b'J' if v >= 0x10047 => {
                let f00 = r.u32()?;
                let kind = r.u32()?;
                let f18 = arr(r)?;
                let k = (kind as usize).min(7);
                let zombies = (0..ZOMBIE_COUNTS[k])
                    .map(|_| {
                        let f00 = r.u32()?;
                        let f04 = arr(r)?;
                        let f14 = arr(r)?;
                        let f24 = arr(r)?;
                        let mut f2c = [0; 6];
                        for x in &mut f2c {
                            *x = r.u32()?;
                        }
                        Ok(Zombie { f00, f04, f14, f24, f2c })
                    })
                    .collect::<Result<_>>()?;
                let graves = (0..GRAVE_COUNTS[k])
                    .map(|_| {
                        Ok(Grave {
                            f00: r.u32()?,
                            f04: r.u32()?,
                            f08: r.u16()?,
                            f0a: r.u16()?,
                            f0c: r.u8()?,
                            zombie: r.u32()?,
                        })
                    })
                    .collect::<Result<_>>()?;
                BuildingExtra::Graveyard(Graveyard { f00, kind, f18, zombies, graves })
            }
            b'K' | b'L' | b'M' if v >= 0x10049 => {
                let f00 = r.u32()?;
                let f04 = r.u32()?;
                let f08 = arr(r)?;
                let mut pairs = [(0, 0); 6];
                for p in &mut pairs {
                    *p = (r.u32()?, r.u32()?);
                }
                BuildingExtra::Ahm(AhmInfo { f00, f04, f08, pairs })
            }
            _ => BuildingExtra::Raw(arr(r)?),
        };
        let f99 = if v >= 0x10043 { arr(r)? } else { [0; 16] };
        out.push(BuildingInstance {
            proto,
            f01,
            name,
            f25,
            f27,
            f29,
            f2b,
            f2f,
            f34,
            f35,
            f39,
            f3d,
            f41,
            f45,
            f49,
            f4d,
            f5a,
            f5c,
            extra,
            f99,
        });
    }
    let n2 = r.i32()?;
    let labels = (0..n2.max(0))
        .map(|_| Ok(BuildingLabel { name: fixed_cstr(r.bytes(0x40)?), a: arr(r)?, b: arr(r)? }))
        .collect::<Result<_>>()?;
    Ok((out, labels))
}

fn read_dynasties(r: &mut Reader, v: u32) -> Result<Vec<Dynasty>> {
    (0..16)
        .map(|_| {
            let mut d = Dynasty { id: r.u16()?, f02: arr(r)?, f14: r.u32()?, ..Default::default() };
            if v > 0x1002b {
                d.f28 = r.u32()?;
            }
            d.f1c = r.u32()?;
            d.f20 = r.u32()?;
            d.f2c = r.u32()?;
            d.f30 = r.u32()?;
            d.f34 = r.u32()?;
            d.f38 = r.u32()?;
            d.f40 = r.u32()?;
            d.f44 = r.u32()?;
            d.f48 = r.u32()?;
            d.f4c = r.u32()?;
            d.f50 = r.u32()?;
            if v > 0x10013 {
                for x in &mut d.ext {
                    *x = r.u32()?;
                }
            }
            d.f60 = if v >= 0x10056 { r.f32()? } else { 1.0 };
            if v > 0x10014 {
                let n = if v > 0x10045 { 0x11 } else { 0x0e };
                d.f74 = r.bytes(n)?.to_vec();
            }
            d.f88 = r.u32()?;
            d.f8c = arr(r)?;
            d.f9c = arr(r)?;
            d.fa8 = r.u32()?;
            if v > 0x10017 {
                d.f70 = r.u32()?;
            }
            Ok(d)
        })
        .collect()
}

fn read_characters(r: &mut Reader, v: u32) -> Result<Characters> {
    let mut cs = Characters { g71da90: r.u16()?, ..Default::default() };
    let n = r.i32()?;
    cs.g71d0cc = r.u32()?;
    cs.g71d0d0 = r.u32()?;
    if v > 0x10016 {
        for x in &mut cs.g71d0d4 {
            *x = r.u32()?;
        }
    }
    for _ in 0..n.max(0) {
        let offset = r.pos();
        let mut c = Character { offset, id: r.u16()?, ..Default::default() };
        c.f20c = if v >= 0x1003e { r.i32()? } else { -1 };
        c.kind_offset = r.pos();
        c.f02 = r.u8()?;
        c.f04 = r.u32()?;
        c.f08 = r.u8()?;
        c.f09 = r.u8()?;
        c.f0a = r.u16()?;
        c.f0c = r.u8()?;
        c.f0d = r.u8()?;
        c.f10 = r.u32()?;
        c.f14 = r.u32()?;
        c.f18 = r.u32()?;
        c.f1c = r.u32()?;
        c.f20 = r.u32()?;
        c.f24 = r.u32()?;
        if v > 0x1003a {
            c.f28 = r.u16()?;
            c.f2c = r.u32()?;
        }
        c.f30 = arr(r)?;
        if v > 0x10030 {
            c.f40 = arr(r)?;
        }
        c.f50 = r.u16()?;
        c.f54 = r.u32()?.wrapping_add(0x59a);
        c.f58 = r.u8()?;
        c.f5c_offset = r.pos();
        c.f5c = arr(r)?;
        if v > 0x10023 {
            c.f7c = r.u32()?;
        }
        c.f80 = arr(r)?;
        c.f88 = r.bytes(0xa8)?.to_vec();
        c.f164 = arr(r)?;
        c.f16c = r.u32()?;
        c.f170 = r.u32()?;
        c.f174 = if v < 0x10020 { r.u16()? as u32 } else { r.u32()? };
        c.f17c = r.u32()?;
        c.f180 = r.u8()?;
        c.f18c = r.u32()?.wrapping_add(0x639);
        c.f190 = r.u32()?;
        if v < 0x1003e {
            c.f190 = 4;
        }
        c.f194 = r.u32()?;
        c.f198 = r.u32()?;
        c.f19c = r.u32()?;
        c.f1a0 = r.u32()?;
        c.f1a4 = r.u32()?;
        c.f1a8 = r.u32()?;
        c.f1ac = r.u32()?;
        c.f1b0 = r.u8()?;
        c.f1b1 = r.u8()?;
        c.f1b4 = arr(r)?;
        c.f1c8 = r.u32()?;
        c.f1cc = if v >= 0x10049 { r.u32()? } else { 0 };
        c.f1d0 = r.u32()?;
        c.f1d4 = arr(r)?;
        c.f1e4 = r.u32()?;
        c.f1e8 = r.u32()?;
        c.f1ec = r.u32()?;
        if v > 0x10029 {
            c.f1f0 = r.u32()?;
        }
        if v > 0x10020 {
            c.f1c5 = r.u8()?;
        }
        c.f184b = r.u32()?;
        if v > 0x10020 {
            c.f1f4 = arr(r)?;
        }
        if v > 0x10035 {
            c.f210 = r.u32()?;
            c.f214 = arr(r)?;
        }
        cs.list.push(c);
    }
    Ok(cs)
}

fn read_markets(r: &mut Reader, v: u32) -> Result<(Vec<Market>, Vec<CityInfo>)> {
    let n = if v > 0x10047 { r.i32()? } else { 0x3e };
    let mut markets = Vec::new();
    for _ in 0..5 {
        let mut m = Market { head: [r.u32()?, r.u32()?, r.u32()?, r.u32()?], wares: Vec::new() };
        for _ in 0..n.max(0) {
            m.wares.push(MarketEntry {
                f00: r.u16()?,
                f04: r.u32()?,
                f08: r.u32()?,
                f0c: r.u32()?,
                f18: r.u32()?,
                f1c: r.u32()?,
                f28: r.u32()?,
                f2c: r.u32()?,
                f30: r.u32()?,
                f34: r.u16()?,
                f24: r.u32()?,
            });
        }
        markets.push(m);
    }
    let mut cities = Vec::new();
    for _ in 0..4 {
        let mut c = CityInfo {
            name: fixed_cstr(r.bytes(0x20)?),
            f20: arr(r)?,
            f28: r.u8()?,
            f2c: r.u32()?,
            f30: r.u32()?,
            f34: r.u16()?,
            f38: arr(r)?,
            f40: r.u8()?,
            f41: r.u8()?,
            ..Default::default()
        };
        c.f44 = (0..10).map(|_| arr(r)).collect::<Result<_>>()?;
        c.f94 = (0..10).map(|_| arr(r)).collect::<Result<_>>()?;
        c.fe4 = r.u8()?;
        c.fe5 = r.u8()?;
        for x in &mut c.fe8 {
            *x = r.u32()?;
        }
        for x in &mut c.ff8 {
            *x = r.u32()?;
        }
        c.f108 = r.u32()?;
        c.f10c = arr(r)?;
        c.f117 = r.u8()?;
        c.f118 = arr(r)?;
        c.f11f = arr(r)?;
        c.f124 = (0..8).map(|_| arr(r)).collect::<Result<_>>()?;
        c.f1b4 = r.u32()?;
        c.f1b8 = r.u32()?;
        c.f1bc = r.bytes(0xd0)?.to_vec();
        if v > 0x10036 {
            c.f2cc = arr(r)?;
        }
        cities.push(c);
    }
    Ok((markets, cities))
}

fn read_offices(r: &mut Reader, v: u32) -> Result<Vec<Office>> {
    let expected = if v >= 0x10051 { 0x2a } else { 0x25 };
    let n = r.u32()?;
    if n != expected {
        return Err(Error::Invalid(format!("offices: count {n}, expected {expected}")));
    }
    (0..n)
        .map(|_| Ok(Office { a: r.u8()?, b: r.u32()?, c: r.u8()?, d: r.u32()?, e: r.u8()?, f: r.u32()? }))
        .collect()
}

impl WorldFile {
    /// Parse a city file or the city-equivalent prefix of a savegame.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let header = read_header(&mut r)?;
        let v = header.version;
        let globals = read_globals(&mut r, v)?;
        let objects = read_objects(&mut r)?;
        let (buildings, building_labels) = read_buildings(&mut r, v)?;
        let dynasties = read_dynasties(&mut r, v)?;
        let characters = read_characters(&mut r, v)?;
        let (markets, cities) = read_markets(&mut r, v)?;
        let offices = if v >= 0x10045 { read_offices(&mut r, v)? } else { Vec::new() };
        let scene_start = r.pos();
        let (_, scene_len) = Scene::parse_prefix(&data[scene_start..])?;
        let dynamic_offset = scene_start + scene_len;
        if header.is_city() && dynamic_offset != data.len() {
            return Err(Error::Invalid(format!("{} bytes after the embedded scene", data.len() - dynamic_offset)));
        }
        Ok(WorldFile {
            header,
            globals,
            objects,
            buildings,
            building_labels,
            dynasties,
            characters,
            markets,
            cities,
            offices,
            tail_offset: scene_start,
            dynamic_offset,
        })
    }
}
