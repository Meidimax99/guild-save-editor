//! Little-endian cursor over a byte slice, the common building block for every parser.

use crate::error::{Error, Result};
use glam::{Vec2, Vec3};

#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::Eof { offset: self.pos, wanted: n });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    pub fn peek_u8(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn vec2(&mut self) -> Result<Vec2> {
        Ok(Vec2::new(self.f32()?, self.f32()?))
    }

    pub fn vec3(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    /// NUL-terminated string, decoded as Windows-1252 (the game's code page).
    pub fn cstr(&mut self) -> Result<String> {
        let rest = &self.data[self.pos.min(self.data.len())..];
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(Error::Eof { offset: self.pos, wanted: rest.len() + 1 })?;
        let s = decode_cp1252(&rest[..end]);
        self.pos += end + 1;
        Ok(s)
    }

    /// Fixed-size, NUL-padded string field.
    pub fn fixed_str(&mut self, n: usize) -> Result<String> {
        Ok(fixed_cstr(self.bytes(n)?))
    }
}

pub fn decode_cp1252(b: &[u8]) -> String {
    encoding_rs::WINDOWS_1252.decode_without_bom_handling(b).0.into_owned()
}

/// Decode a NUL-padded byte field up to its first NUL.
pub fn fixed_cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    decode_cp1252(&b[..end])
}

/// Serialize a fixed-size byte array of any length (serde only covers ≤ 32).
pub fn ser_bytes<S: serde::Serializer, const N: usize>(b: &[u8; N], s: S) -> std::result::Result<S::Ok, S::Error> {
    s.collect_seq(b.iter())
}
