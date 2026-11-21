//! Read-only view of everything parsed from a save, as one generic tree.
//!
//! Every parsed structure derives `serde::Serialize`; this module serializes
//! them into a `serde_json::Value` so a viewer can show all fields without
//! knowing their types. Field names are those of the Rust structs (`fXX` =
//! in-memory offset of a not yet named field).

use crate::save::SaveFile;
use crate::scene::Scene;
use crate::world::WorldFile;
use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct Explored {
    pub path: PathBuf,
    pub root: Value,
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
    Ok(Explored { path: path.to_owned(), root: Value::Object(root) })
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
        ("header", "f60") => "owner profession, ?, ?",
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
        ("characters/list", "f30") => "first name",
        ("characters/list", "f40") => "last name",
        ("characters/list", "f50") => "family / dynasty id",
        ("characters/list", "f54") => "coat of arms",
        ("characters/list", "f58") => "generation",
        ("characters/list", "f5c") => "family: spouse, ?, children (object ids)",
        ("characters/list", "f80") => "appearance",
        ("characters/list", "f164") => "profession, ?, ?",
        // Dynasties.
        ("dynasties", "id") => "dynasty id",
        ("dynasties", "f02") => "name",
        // Objects and buildings.
        ("objects", "f02") => "object id",
        ("buildings", "proto") => "prototype (A_Geb.dat)",
        ("buildings", "f01") => "object id",
        _ => return None,
    })
}
