//! Global text table: all `textbin_<lang>/*.res` files merged by string id.
//!
//! The engine keeps one pointer array (`0x144a280`) indexed by id, filled from
//! each `.res` file at `first_id + i` (`FUN_00427650`). Code references strings
//! by id or by key (`FUN_00427a60` looks a key up).

use crate::res::ResFile;
use crate::vfs::Vfs;
use crate::error::Result;
use std::collections::HashMap;

#[derive(Default)]
pub struct TextDb {
    by_id: HashMap<u32, String>,
    by_key: HashMap<String, u32>,
}

impl TextDb {
    pub fn load(vfs: &Vfs, lang: &str) -> Result<Self> {
        let mut db = TextDb::default();
        for path in vfs.glob(&format!("textbin_{lang}"), "res") {
            let res = ResFile::parse(&vfs.read(path)?)?;
            for (i, e) in res.entries.iter().enumerate() {
                let id = res.first_id + i as u32;
                db.by_key.entry(e.key.to_uppercase()).or_insert(id);
                db.by_id.insert(id, e.text.clone());
            }
        }
        Ok(db)
    }

    pub fn get(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id).map(String::as_str)
    }

    /// Look up by key, e.g. `"_OPTIONEN_MESSAGE_FILTER_UEBERSCHRIFT+0"`.
    pub fn id(&self, key: &str) -> Option<u32> {
        self.by_key.get(&key.to_uppercase()).copied()
    }

    pub fn by_key(&self, key: &str) -> Option<&str> {
        self.id(key).and_then(|i| self.get(i))
    }

    /// Text for an engine pointer into the string table (`0x144a280 + 4·id`).
    pub fn from_table_address(&self, addr: u32) -> Option<&str> {
        addr.checked_sub(0x144a280).filter(|o| o % 4 == 0).and_then(|o| self.get(o / 4))
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}
