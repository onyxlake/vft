//! Version-zero wire format. No I/O or platform dependencies; all integers are little endian.
#![no_std]
extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::fmt;
pub use sha2::{Digest, Sha256};

pub const VERSION: u8 = 0;
pub const HEADER_LEN: usize = 52;
pub const CRC_LEN: usize = 4;
pub const MAX_PAYLOAD: usize = 65535;
pub const GENERATION_SIZE: u32 = 64;
pub type SessionId = [u8; 16];
pub type ObjectId = [u8; 16];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Manifest = 0,
    SourceSymbol = 1,
    RepairSymbol = 2,
    Control = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProfileId {
    Robust = 0,
    Balanced = 1,
}

impl TryFrom<u8> for ProfileId {
    type Error = Error;
    fn try_from(value: u8) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Robust),
            1 => Ok(Self::Balanced),
            _ => Err(Error::Unsupported),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SymbolId {
    pub generation: u32,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub session: SessionId,
    pub object: ObjectId,
    pub kind: PacketType,
    pub symbol: SymbolId,
    pub profile: ProfileId,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated,
    Magic,
    Unsupported,
    Length,
    Crc,
    InvalidManifest,
    Utf8,
    Overflow,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VFT protocol: {self:?}")
    }
}
impl core::error::Error for Error {}

pub fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0x82f63b78 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

impl Packet {
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        if self.payload.len() > MAX_PAYLOAD {
            return Err(Error::Length);
        }
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len() + CRC_LEN);
        out.extend_from_slice(b"VFT0");
        out.extend_from_slice(&[VERSION, self.kind as u8, self.profile as u8, 0]);
        out.extend_from_slice(&self.session);
        out.extend_from_slice(&self.object);
        out.extend_from_slice(&self.symbol.generation.to_le_bytes());
        out.extend_from_slice(&self.symbol.index.to_le_bytes());
        out.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.payload);
        let crc = crc32c(&out);
        out.extend_from_slice(&crc.to_le_bytes());
        Ok(out)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_LEN + CRC_LEN {
            return Err(Error::Truncated);
        }
        if &bytes[..4] != b"VFT0" {
            return Err(Error::Magic);
        }
        if bytes[4] != VERSION || bytes[7] != 0 {
            return Err(Error::Unsupported);
        }
        let kind = match bytes[5] {
            0 => PacketType::Manifest,
            1 => PacketType::SourceSymbol,
            2 => PacketType::RepairSymbol,
            3 => PacketType::Control,
            _ => return Err(Error::Unsupported),
        };
        let profile = ProfileId::try_from(bytes[6])?;
        let length = u32::from_le_bytes(bytes[48..52].try_into().unwrap()) as usize;
        if length > MAX_PAYLOAD || bytes.len() != HEADER_LEN + length + CRC_LEN {
            return Err(Error::Length);
        }
        let end = bytes.len() - CRC_LEN;
        if crc32c(&bytes[..end]) != u32::from_le_bytes(bytes[end..].try_into().unwrap()) {
            return Err(Error::Crc);
        }
        Ok(Self {
            session: bytes[8..24].try_into().unwrap(),
            object: bytes[24..40].try_into().unwrap(),
            kind,
            symbol: SymbolId {
                generation: u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
                index: u32::from_le_bytes(bytes[44..48].try_into().unwrap()),
            },
            profile,
            payload: bytes[HEADER_LEN..end].to_vec(),
        })
    }
}

/// Fixed 64-symbol generations bound the encoder and Gaussian elimination cost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub session: SessionId,
    pub object: ObjectId,
    pub filename: String,
    pub file_size: u64,
    pub sha256: [u8; 32],
    pub symbol_size: u32,
    pub source_symbols: u32,
    pub profile: ProfileId,
}

impl Manifest {
    pub fn validate(&self) -> Result<(), Error> {
        if self.filename.is_empty()
            || self.filename.len() > 255
            || self.filename.contains(['\0', '/', '\\', ':'])
            || self.filename == "."
            || self.filename == ".."
            || self.symbol_size == 0
            || self.symbol_size as usize > MAX_PAYLOAD
        {
            return Err(Error::InvalidManifest);
        }
        let count = self.file_size.div_ceil(u64::from(self.symbol_size));
        if count > u64::from(u32::MAX) || count != u64::from(self.source_symbols) {
            return Err(Error::InvalidManifest);
        }
        Ok(())
    }
    pub fn generations(&self) -> u32 {
        self.source_symbols.div_ceil(GENERATION_SIZE)
    }
    pub fn generation_symbols(&self, generation: u32) -> Result<u32, Error> {
        if generation >= self.generations() {
            return Err(Error::Length);
        }
        Ok((self.source_symbols - generation * GENERATION_SIZE).min(GENERATION_SIZE))
    }
    pub fn symbol_offset(&self, global_index: u32) -> Result<u64, Error> {
        if global_index >= self.source_symbols {
            return Err(Error::Length);
        }
        Ok(u64::from(global_index) * u64::from(self.symbol_size))
    }
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;
        let mut out = Vec::new();
        // FEC 0 = deterministic binary fountain; compression 0 = none.
        out.extend_from_slice(&[VERSION, 0, self.profile as u8, 0]);
        out.extend_from_slice(&self.session);
        out.extend_from_slice(&self.object);
        out.extend_from_slice(&self.file_size.to_le_bytes());
        out.extend_from_slice(&self.sha256);
        out.extend_from_slice(&self.symbol_size.to_le_bytes());
        out.extend_from_slice(&self.source_symbols.to_le_bytes());
        out.extend_from_slice(&(self.filename.len() as u16).to_le_bytes());
        out.extend_from_slice(self.filename.as_bytes());
        Ok(out)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < 86 {
            return Err(Error::Truncated);
        }
        if bytes[0] != VERSION || bytes[1] != 0 || bytes[3] != 0 {
            return Err(Error::Unsupported);
        }
        let len = u16::from_le_bytes(bytes[84..86].try_into().unwrap()) as usize;
        if bytes.len() != 86 + len {
            return Err(Error::Length);
        }
        let manifest = Self {
            session: bytes[4..20].try_into().unwrap(),
            object: bytes[20..36].try_into().unwrap(),
            file_size: u64::from_le_bytes(bytes[36..44].try_into().unwrap()),
            sha256: bytes[44..76].try_into().unwrap(),
            symbol_size: u32::from_le_bytes(bytes[76..80].try_into().unwrap()),
            source_symbols: u32::from_le_bytes(bytes[80..84].try_into().unwrap()),
            filename: String::from(core::str::from_utf8(&bytes[86..]).map_err(|_| Error::Utf8)?),
            profile: ProfileId::try_from(bytes[2])?,
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    #[test]
    fn checksums_known_answers() {
        assert_eq!(crc32c(b"123456789"), 0xe3069283);
        assert_eq!(
            Sha256::digest(b"abc").as_slice(),
            &[
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
    }
    #[test]
    fn packet_golden_and_reject_corruption() {
        let packet = Packet {
            session: [0x11; 16],
            object: [0x22; 16],
            kind: PacketType::SourceSymbol,
            symbol: SymbolId {
                generation: 0x01020304,
                index: 7,
            },
            profile: ProfileId::Balanced,
            payload: vec![0, 1, 0xff],
        };
        let golden = include_bytes!("../../../tests/golden/packet-v0.bin");
        assert_eq!(packet.encode().unwrap(), golden);
        assert_eq!(Packet::decode(golden).unwrap(), packet);
        for i in 0..golden.len() {
            let mut bad = golden.to_vec();
            bad[i] ^= 0x80;
            assert!(Packet::decode(&bad).is_err(), "offset {i}");
        }
        for end in 0..golden.len() {
            assert!(Packet::decode(&golden[..end]).is_err());
        }
    }
    #[test]
    fn manifest_golden() {
        let manifest = Manifest {
            session: [0x11; 16],
            object: [0x22; 16],
            filename: String::from("abc.bin"),
            file_size: 3,
            sha256: Sha256::digest(b"abc").into(),
            symbol_size: 2,
            source_symbols: 2,
            profile: ProfileId::Balanced,
        };
        let golden = include_bytes!("../../../tests/golden/manifest-v0.bin");
        assert_eq!(manifest.encode().unwrap(), golden);
        assert_eq!(Manifest::decode(golden).unwrap(), manifest);
        assert_eq!(manifest.symbol_offset(1), Ok(2));
        let mut bad = manifest.clone();
        bad.source_symbols = 3;
        assert!(bad.validate().is_err());
        bad = manifest;
        bad.filename = String::from("../evil");
        assert!(bad.validate().is_err());
    }
}
