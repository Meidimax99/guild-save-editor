//! `.res` — localized string tables (`Resources/textbin_<lang>.bin`).
//!
//! Loader: `FUN_00427650` (`ReloadTextFile`). Layout, all little-endian:
//!
//! ```text
//! u32           count
//! u32           first_id      global string id of entry 0
//! u32           end_id        exclusive upper bound of the id range reserved by this file
//! u32[count]    offsets       into the blob
//! [u8; 80]      keys[count]   NUL-padded key, e.g. "_GELDLEIHE_GELDWECHSEL_BESITZER+0"
//! u8[count]     flags         0xFF in every shipped file
//! u32           blob_len
//! u8[blob_len]  blob          NUL-terminated Windows-1252 strings
//! ```
//!
//! The engine copies every file into one global table indexed by `first_id + i`, so
//! string ids are stable across files.

use crate::reader::{Reader, fixed_cstr};
use crate::error::Result;

pub const KEY_LEN: usize = 80;

#[derive(Debug, Clone)]
pub struct ResEntry {
    pub key: String,
    pub flags: u8,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ResFile {
    pub first_id: u32,
    pub end_id: u32,
    pub entries: Vec<ResEntry>,
}

impl ResFile {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let count = r.u32()? as usize;
        let first_id = r.u32()?;
        let end_id = r.u32()?;
        let offsets = (0..count).map(|_| r.u32()).collect::<Result<Vec<_>>>()?;
        let keys = (0..count)
            .map(|_| r.bytes(KEY_LEN).map(fixed_cstr))
            .collect::<Result<Vec<_>>>()?;
        let flags = r.bytes(count)?.to_vec();
        let blob_len = r.u32()? as usize;
        let blob = r.bytes(blob_len)?;
        let entries = (0..count)
            .map(|i| {
                let mut br = Reader::new(blob);
                br.set_pos(offsets[i] as usize);
                Ok(ResEntry { key: keys[i].clone(), flags: flags[i], text: br.cstr()? })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { first_id, end_id, entries })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|e| e.key == key).map(|e| e.text.as_str())
    }
}
