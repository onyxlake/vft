use crate::{Control, Result};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
pub const HEADER_LEN: usize = 128;
const MAGIC: &[u8; 8] = b"VFTGIT\r\n";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectFormat {
    Sha1,
    Sha256,
}
impl ObjectFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }
    pub fn oid_len(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
    pub(crate) fn validate_oid(self, oid: &str) -> Result<()> {
        if oid.len() != self.oid_len()
            || !oid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || oid.bytes().all(|b| b == b'0')
        {
            return Err(format!(
                "expected a full nonzero lowercase {} object ID",
                self.name()
            )
            .into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub format: ObjectFormat,
    pub base: String,
    pub target: String,
    pub bundle_len: u64,
    pub bundle_sha256: [u8; 32],
}
impl Header {
    pub fn encode(&self) -> Result<[u8; HEADER_LEN]> {
        self.format.validate_oid(&self.base)?;
        self.format.validate_oid(&self.target)?;
        if self.base == self.target
            || self.bundle_len == 0
            || self.bundle_len.checked_add(HEADER_LEN as u64).is_none()
        {
            return Err("empty delta or invalid bundle length".into());
        }
        let mut out = [0; HEADER_LEN];
        out[..8].copy_from_slice(MAGIC);
        out[8..10].copy_from_slice(&1u16.to_le_bytes());
        out[10..12].copy_from_slice(&(HEADER_LEN as u16).to_le_bytes());
        out[12] = if self.format == ObjectFormat::Sha1 {
            1
        } else {
            2
        };
        out[16..24].copy_from_slice(&self.bundle_len.to_le_bytes());
        for (start, oid) in [(24, &self.base), (56, &self.target)] {
            for (i, part) in oid.as_bytes().as_chunks::<2>().0.iter().enumerate() {
                out[start + i] = u8::from_str_radix(std::str::from_utf8(part)?, 16)?;
            }
        }
        out[88..120].copy_from_slice(&self.bundle_sha256);
        Ok(out)
    }
    pub fn decode(bytes: &[u8; HEADER_LEN], file_len: u64, limit: u64) -> Result<Self> {
        if &bytes[..8] != MAGIC
            || bytes[8..10] != 1u16.to_le_bytes()
            || bytes[10..12] != (HEADER_LEN as u16).to_le_bytes()
            || bytes[13..16] != [0; 3]
            || bytes[120..] != [0; 8]
        {
            return Err("invalid VFT Git magic, version, header size or reserved fields".into());
        }
        let format = match bytes[12] {
            1 => ObjectFormat::Sha1,
            2 => ObjectFormat::Sha256,
            _ => return Err("unsupported Git object format".into()),
        };
        let len = format.oid_len() / 2;
        if bytes[24 + len..56]
            .iter()
            .chain(&bytes[56 + len..88])
            .any(|b| *b != 0)
        {
            return Err("nonzero object ID padding".into());
        }
        let oid = |offset| {
            bytes[offset..offset + len]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        };
        let header = Self {
            format,
            base: oid(24),
            target: oid(56),
            bundle_len: u64::from_le_bytes(bytes[16..24].try_into()?),
            bundle_sha256: bytes[88..120].try_into()?,
        };
        header.encode()?;
        if file_len > limit || header.bundle_len.checked_add(HEADER_LEN as u64) != Some(file_len) {
            return Err("Git package length mismatch or size limit exceeded".into());
        }
        Ok(header)
    }
}
pub(crate) fn copy_hash(
    reader: &mut impl Read,
    writer: &mut impl Write,
    len: u64,
    control: &Control,
) -> Result<[u8; 32]> {
    let mut remaining = len;
    let mut buffer = [0; 65536];
    let mut hash = Sha256::new();
    while remaining > 0 {
        control.check()?;
        let n = remaining.min(buffer.len() as u64) as usize;
        reader.read_exact(&mut buffer[..n])?;
        writer.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        remaining -= n as u64;
    }
    Ok(hash.finalize().into())
}
