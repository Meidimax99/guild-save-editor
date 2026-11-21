//! Read-only view of everything parsed from a save, as one generic tree.
//!
//! Every parsed structure derives `serde::Serialize`; this module serializes
//! them into a `serde_json::Value` so a viewer can show all fields without
//! knowing their types. Field names are those of the Rust structs (`fXX` =
//! in-memory offset of a not yet named field).

use crate::save::SaveFile;
use crate::scene::Scene;
use crate::world::WorldFile;
use anyhow::{Context as _, Result};
use serde_json::{Map, Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct Explored {
    pub path: PathBuf,
    pub root: Value,
    /// Lookups used to show values as text.
    pub ctx: Context,
}

/// What is needed to turn raw values into readable text: the game's text
/// table (loaded from the game installation if found) and name lookups built
/// from the save itself.
#[derive(Default)]
pub struct Context {
    pub texts: Option<std::sync::Arc<crate::text::TextDb>>,
    pub game_dir: Option<PathBuf>,
    /// Character object id (`+0x04`) → name.
    pub char_by_object: std::collections::HashMap<u32, String>,
    /// Character slot → name.
    pub char_by_slot: std::collections::HashMap<u16, String>,
    /// Dynasty id → name.
    pub dynasty: std::collections::HashMap<u16, String>,
    /// Building object id → name.
    pub building_by_object: std::collections::HashMap<u32, String>,
}

/// Game texts are loaded once per game folder.
static TEXTS: std::sync::Mutex<Option<(PathBuf, std::sync::Arc<crate::text::TextDb>)>> = std::sync::Mutex::new(None);

fn load_texts(near: &Path) -> Option<(PathBuf, std::sync::Arc<crate::text::TextDb>)> {
    let dir = crate::vfs::find_game_dir_from(near.parent().unwrap_or(near)).or_else(crate::vfs::find_game_dir)?;
    let mut cache = TEXTS.lock().ok()?;
    if let Some((d, t)) = cache.as_ref()
        && *d == dir
    {
        return Some((d.clone(), t.clone()));
    }
    let vfs = crate::vfs::Vfs::open_game_dir(&dir).ok()?;
    let lang = vfs
        .list()
        .iter()
        .find_map(|p| p.strip_prefix("textbin_").and_then(|r| r.split('/').next()).map(str::to_owned))
        .unwrap_or_else(|| "german".into());
    let db = std::sync::Arc::new(crate::text::TextDb::load(&vfs, &lang).ok()?);
    *cache = Some((dir.clone(), db.clone()));
    Some((dir, db))
}

impl Context {
    fn build(world: &WorldFile, near: &Path) -> Self {
        let mut c = Context::default();
        if let Some((dir, db)) = load_texts(near) {
            c.game_dir = Some(dir);
            c.texts = Some(db);
        }
        for ch in &world.characters.list {
            let name = crate::ops::full_name(ch);
            c.char_by_object.insert(ch.f04, name.clone());
            c.char_by_slot.insert(ch.id, name);
        }
        for d in &world.dynasties {
            let n = crate::reader::fixed_cstr(&d.f02);
            if d.id != 0xffff {
                c.dynasty.insert(d.id, if n.is_empty() { format!("dynasty {:#x}", d.id) } else { n });
            }
        }
        for b in &world.buildings {
            c.building_by_object.insert(b.f01, b.name.clone());
        }
        c
    }

    fn text(&self, id: u32) -> Option<String> {
        self.texts.as_ref()?.get(id).filter(|s| !s.is_empty()).map(str::to_owned)
    }

    /// `base + v` from the game's text table, with the number in brackets.
    fn enum_text(&self, base: u32, v: u64) -> String {
        match self.text(base + v as u32) {
            Some(t) => format!("{t} ({v})"),
            None => v.to_string(),
        }
    }

    fn character_object(&self, id: u64) -> String {
        if id == 0xffff_ffff {
            return "–".into();
        }
        match self.char_by_object.get(&(id as u32)) {
            Some(n) => n.clone(),
            None => format!("#{id}"),
        }
    }
}

/// Load any world-state file: `.SAV`, `.SRV` (gzip) or `.cty` (plain).
pub fn load(path: &Path) -> Result<Explored> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let compressed = data.starts_with(&[0x1f, 0x8b]);
    let raw = if compressed {
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(&data[..]).read_to_end(&mut raw).context("decompressing")?;
        raw
    } else {
        data.clone()
    };
    let world = WorldFile::parse(&raw).with_context(|| format!("parsing {}", path.display()))?;
    let ctx = Context::build(&world, path);

    let mut root = Map::new();
    let file_type = if !world.header.is_city() {
        "full save (.SAV)"
    } else if world.header.flags & 4 != 0 {
        "snapshot (.SRV)"
    } else {
        "city (.cty)"
    };
    root.insert(
        "file".into(),
        json!({
            "path": path.display().to_string(),
            "type": file_type,
            "gzip": compressed,
            "size on disk": data.len(),
            "decompressed size": raw.len(),
            "version": world.header.version,
            "world state": format!("0x0 .. {:#x}", world.tail_offset),
            "embedded scene": format!("{:#x} .. {:#x}", world.tail_offset, world.dynamic_offset),
            "dynamic state": format!("{:#x} .. {:#x} ({} bytes)", world.dynamic_offset, raw.len(), raw.len() - world.dynamic_offset),
        }),
    );
    if let Value::Object(w) = serde_json::to_value(&world)? {
        for (k, v) in w {
            if !matches!(k.as_str(), "tail_offset" | "dynamic_offset") {
                root.insert(k, v);
            }
        }
    }
    match Scene::parse_prefix(&raw[world.tail_offset..]) {
        Ok((scene, _)) => {
            root.insert("scene".into(), serde_json::to_value(&scene)?);
        }
        Err(e) => {
            root.insert("scene".into(), json!(format!("error: {e}")));
        }
    }
    if !world.header.is_city() {
        let save = SaveFile { path: path.to_owned(), raw, world };
        match save.notices() {
            Ok(n) => {
                root.insert("notices".into(), serde_json::to_value(&n.notices)?);
            }
            Err(e) => {
                root.insert("notices".into(), json!(format!("error: {e:#}")));
            }
        }
    }
    Ok(Explored { path: path.to_owned(), root: Value::Object(root), ctx })
}

/// One step of a path into the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Key(String),
    Index(usize),
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Step::Key(k) => write!(f, "{k}"),
            Step::Index(i) => write!(f, "[{i}]"),
        }
    }
}

pub fn get<'a>(root: &'a Value, path: &[Step]) -> Option<&'a Value> {
    path.iter().try_fold(root, |v, s| match (s, v) {
        (Step::Key(k), Value::Object(m)) => m.get(k),
        (Step::Index(i), Value::Array(a)) => a.get(*i),
        _ => None,
    })
}

/// Arrays of small integers are shown as bytes.
pub fn as_bytes(v: &Value) -> Option<Vec<u8>> {
    let a = v.as_array()?;
    if a.is_empty() {
        return None;
    }
    a.iter().map(|x| x.as_u64().filter(|&n| n <= 255).map(|n| n as u8)).collect()
}

pub fn is_composite(v: &Value) -> bool {
    match v {
        Value::Object(m) => !m.is_empty(),
        Value::Array(a) => !a.is_empty() && (a.len() > 16 || a.iter().any(|x| x.is_object() || x.is_array())),
        _ => false,
    }
}

pub fn type_name(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(_) => "bool".into(),
        Value::Number(n) if n.is_f64() => "float".into(),
        Value::Number(_) => "int".into(),
        Value::String(_) => "text".into(),
        Value::Array(a) if as_bytes(v).is_some() => format!("bytes[{}]", a.len()),
        Value::Array(a) => match a.first() {
            Some(Value::Object(_)) => format!("records[{}]", a.len()),
            _ => format!("list[{}]", a.len()),
        },
        Value::Object(m) => format!("record ({} fields)", m.len()),
    }
}

/// Text decoded from a byte array if it looks like a NUL-padded string.
pub fn bytes_text(b: &[u8]) -> Option<String> {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    if end == 0 || b[end..].iter().any(|&c| c != 0) {
        return None;
    }
    let s = &b[..end];
    if s.iter().all(|&c| c >= 0x20 && c != 0x7f) {
        Some(crate::reader::decode_cp1252(s))
    } else {
        None
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

/// Short one-line rendering of a value.
pub fn preview(v: &Value) -> String {
    match v {
        Value::Null => "–".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => format!("{f}"),
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        Value::Array(a) => {
            if let Some(b) = as_bytes(v) {
                if let Some(t) = bytes_text(&b) {
                    return format!("\"{t}\"");
                }
                if b.iter().all(|&x| x == 0) {
                    return format!("{} × 00", b.len());
                }
                if b.len() <= 32 {
                    return hex(&b);
                }
                return format!("{} … ({} bytes)", hex(&b[..16]), b.len());
            }
            if a.is_empty() {
                return "[]".into();
            }
            if a.len() <= 16 && a.iter().all(|x| !x.is_object() && !x.is_array()) {
                return format!("[{}]", a.iter().map(preview).collect::<Vec<_>>().join(", "));
            }
            type_name(v)
        }
        Value::Object(m) => {
            // Name-like fields make a record recognizable.
            for k in ["name", "id", "description"] {
                if let Some(x) = m.get(k)
                    && !x.is_object()
                    && !x.is_array()
                {
                    return format!("{{{k}: {} …}}", preview(x));
                }
            }
            type_name(v)
        }
    }
}

/// Other readings of an integer, for hover tooltips (many unnamed fields are
/// floats or packed values stored as integers).
pub fn int_readings(v: &Value) -> Option<String> {
    let n = v.as_i64()?;
    let u = n as u32;
    let f = f32::from_bits(u);
    let mut s = format!("hex {:#x}\ni32 {}\nu32 {}", u, u as i32, u);
    if f.is_finite() && f != 0.0 && (1e-4..1e9).contains(&f.abs()) {
        s += &format!("\nf32 {f}");
    }
    let b = u.to_le_bytes();
    s += &format!("\nbytes {}", hex(&b));
    Some(s)
}

/// Top-level sections that are technical rather than editable content.
pub fn is_technical_section(key: &str) -> bool {
    matches!(key, "file" | "scene")
}

/// Fields hidden unless "show everything" is on: raw byte data that is not
/// text, and file offsets.
pub fn is_technical_field(key: &str, v: &Value) -> bool {
    if key == "offset" || key.ends_with("_offset") || key == "len" {
        return true;
    }
    match as_bytes(v) {
        Some(b) => bytes_text(&b).is_none(),
        None => false,
    }
}

/// Whether a path step leads into hidden data.
pub fn path_is_technical(root: &Value, path: &[Step]) -> bool {
    let mut node = root;
    for (i, s) in path.iter().enumerate() {
        if let Step::Key(k) = s {
            if i == 0 && is_technical_section(k) {
                return true;
            }
        }
        let Some(next) = get(node, std::slice::from_ref(s)) else { return false };
        if let Step::Key(k) = s
            && is_technical_field(k, next)
        {
            return true;
        }
        node = next;
    }
    false
}

/// The kind of record a path points into, e.g. `characters/list` for a
/// character (indices are dropped).
pub fn scope_of(path: &[Step]) -> String {
    path.iter()
        .filter_map(|s| match s {
            Step::Key(k) => Some(k.as_str()),
            Step::Index(_) => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Human-readable names for fields whose meaning is known from the game's
/// code. `scope` is [`scope_of`] the record's path. Fields not listed keep
/// their raw name (`fXX` = in-memory offset).
pub fn label(scope: &str, key: &str) -> Option<&'static str> {
    Some(match (scope, key) {
        // Header (`FUN_00507760` writes, `FUN_00521130`/`FUN_005783a0` read).
        ("header", "flags") => "file type (5 save, 6 snapshot)",
        ("header", "name") => "city",
        ("header", "f31") => "human players",
        ("header", "f32") => "game date",
        ("header", "f40") => "owner first name",
        ("header", "f50") => "owner last name",
        ("header", "f60") => "owner master grade, ?, rank",
        ("header", "f63") => "owner religion",
        ("header", "f64") => "owner coat of arms (+0x59a)",
        ("header", "f6c") => "owner gender, ?",
        ("header", "f78") => "player save checksums",
        ("header", "thumbnail") => "thumbnail",
        ("header", "description") => "save name",
        // Globals.
        ("globals", "g6438e4") => "next object id",
        ("globals", "date") => "game date",
        ("globals", "g71d088") => "next family id",
        ("globals", "g71d08c") => "player dynasties",
        ("globals", "g71d090") => "characters alive",
        // Characters (`FUN_004f5860` creates them).
        ("characters", "g71da90") => "local player (slot)",
        ("characters/list", "id") => "slot",
        ("characters/list", "f20c") => "attending session (id)",
        ("characters/list", "f02") => "kind",
        ("characters/list", "f04") => "object id",
        ("characters/list", "f08") => "life (0 = dead)",
        ("characters/list", "f09") => "gender (0 m, 1 f)",
        ("characters/list", "f0a") => "age",
        ("characters/list", "f0c") => "religion",
        ("characters/list", "f0d") => "rank (Stand)",
        ("characters/list", "f30") => "first name",
        ("characters/list", "f40") => "last name",
        ("characters/list", "f50") => "family / dynasty id",
        ("characters/list", "f54") => "coat of arms",
        ("characters/list", "f58") => "generation",
        ("characters/list", "f5c") => "family: spouse, ?, children (object ids)",
        ("characters/list", "f80") => "appearance",
        ("characters/list", "f164") => "master grade, job, …",
        // Dynasties.
        ("dynasties", "id") => "dynasty id",
        ("dynasties", "f02") => "name",
        // Objects and buildings.
        ("objects", "f02") => "object id",
        ("buildings", "proto") => "prototype (A_Geb.dat)",
        ("buildings", "f01") => "object id",
        ("buildings", "name") => "name",
        ("buildings", "f27") => "owner",
        _ => return None,
    })
}


/// Game texts used to decode character fields (ids in the global text table
/// `0x144a280`, found through the character dump `FUN_004d3ab0`).
mod text_id {
    pub const RELIGION: u32 = 1158; // katholisch, katharisch
    pub const GENDER: u32 = 1161; // männlich, weiblich
    pub const RANK: u32 = 276; // Unfreier, Herr, Bürger, Patrizier, …
    pub const MASTER_GRADE: u32 = 304; // Kein Meistergrad, Meisterdieb, …
    pub const JOB: u32 = 525; // (kein Personal), Diebesgeselle, …
}

fn u32s(v: &Value) -> Option<Vec<u32>> {
    let b = as_bytes(v)?;
    (b.len() % 4 == 0).then(|| b.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
}

/// A 14-byte game date (round i32, hour u16, minute i32, second i32).
pub fn date_text(b: &[u8]) -> Option<String> {
    if b.len() != 14 {
        return None;
    }
    let round = i32::from_le_bytes(b[0..4].try_into().unwrap());
    let hour = u16::from_le_bytes(b[4..6].try_into().unwrap());
    let minute = i32::from_le_bytes(b[6..10].try_into().unwrap());
    if !(0..1000).contains(&round) || hour > 23 || !(0..60).contains(&minute) {
        return None;
    }
    Some(format!("{} {hour:02}:{minute:02} (round {round})", 1400 + round))
}

/// A readable rendering of a value, if one is known; `None` = show the raw
/// value. `scope` is [`scope_of`] the record, `key` the field.
pub fn display(ctx: &Context, scope: &str, key: &str, v: &Value) -> Option<String> {
    use text_id::*;
    let n = v.as_u64();
    let field = |s: &str, k: &str| scope == s && key == k;
    // Specific fields first.
    let specific = match (scope, key) {
        ("characters/list", "f02") => n.map(|k| format!("{} ({k})", crate::ops::kind_name(k as u8))),
        ("characters/list", "f09") | ("header", "f6c") => {
            let g = n.or_else(|| v.as_array().and_then(|a| a.first()).and_then(Value::as_u64))?;
            Some(ctx.enum_text(GENDER, g))
        }
        ("characters/list", "f0c") | ("header", "f63") => n.map(|r| ctx.enum_text(RELIGION, r)),
        ("characters/list", "f0d") => n.map(|r| ctx.enum_text(RANK, r)),
        ("characters/list", "f164") => {
            let b = as_bytes(v)?;
            Some(format!(
                "grade: {}, job: {}, {}",
                ctx.enum_text(MASTER_GRADE, b[0] as u64),
                ctx.enum_text(JOB, b[1] as u64),
                hex(&b[2..])
            ))
        }
        ("header", "f60") => {
            let a = v.as_array()?;
            let g = a.first()?.as_u64()?;
            let r = a.get(2)?.as_u64()?;
            Some(format!("grade: {}, rank: {}, ? {}", ctx.enum_text(MASTER_GRADE, g), ctx.enum_text(RANK, r), preview(&a[1])))
        }
        ("characters/list", "f50") => n.map(|d| match ctx.dynasty.get(&(d as u16)) {
            Some(name) => format!("{name} ({d:#x})"),
            None => format!("{d:#x}"),
        }),
        ("characters/list", "f54") | ("header", "f64") => n.map(|a| format!("{}", a as i64 - 0x59a)),
        ("characters/list", "f5c") => {
            let ids = u32s(v)?;
            let spouse = ctx.character_object(ids[0] as u64);
            let kids: Vec<String> =
                ids[2..].iter().filter(|&&x| x != 0xffff_ffff && x != 0xffff).map(|&x| ctx.character_object(x as u64)).collect();
            Some(format!(
                "spouse: {spouse}; children: {}{}",
                if kids.is_empty() { "–".to_string() } else { kids.join(", ") },
                if ids[1] != 0xffff_ffff && ids[1] != 0xffff { format!("; ? {}", ctx.character_object(ids[1] as u64)) } else { String::new() }
            ))
        }
        ("characters", "g71da90") | ("notices", "character") => {
            n.map(|s| ctx.char_by_slot.get(&(s as u16)).map(|x| format!("{x} ({s})")).unwrap_or_else(|| s.to_string()))
        }
        ("buildings", "f27") => n.map(|s| {
            if s == 0xffff {
                "–".into()
            } else {
                ctx.char_by_slot.get(&(s as u16)).map(|x| format!("{x} ({s})")).unwrap_or_else(|| s.to_string())
            }
        }),
        ("header", "f70") => {
            let a = v.as_array()?;
            let ids: Vec<String> = a
                .iter()
                .filter_map(Value::as_i64)
                .map(|x| {
                    if x == -1 {
                        "–".into()
                    } else {
                        ctx.building_by_object.get(&(x as u32)).cloned().unwrap_or_else(|| ctx.character_object(x as u64))
                    }
                })
                .collect();
            Some(ids.join(", "))
        }
        _ => None,
    };
    if specific.is_some() {
        return specific;
    }
    let _ = field;
    // Generic rules.
    match v {
        Value::Number(_) => {
            let i = v.as_i64()?;
            if i == -1 || i == 0xffff_ffff {
                return Some("–".into());
            }
            // Floats stored in 32-bit fields: large integers whose float
            // reading is an ordinary number.
            if (0x3000_0000..=0xffff_ffff).contains(&i) || (i < 0 && i >= i32::MIN as i64) {
                let f = f32::from_bits(i as u32);
                if f.is_finite() && (1e-3..1e7).contains(&f.abs()) {
                    let t = format!("{f:.3}");
                    return Some(t.trim_end_matches('0').trim_end_matches('.').to_owned());
                }
            }
            None
        }
        Value::Array(_) => as_bytes(v).and_then(|b| date_text(&b)),
        _ => None,
    }
}
