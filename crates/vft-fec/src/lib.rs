//! Bounded systematic binary fountain code with deterministic dense repair equations.
#![no_std]
extern crate alloc;
use alloc::{vec, vec::Vec};
use core::fmt;
use vft_protocol::{Digest, Manifest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Parameters,
    SymbolSize,
    Incomplete,
    Inconsistent,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VFT FEC: {self:?}")
    }
}
impl core::error::Error for Error {}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub count: u32,
    pub symbol_size: usize,
    pub seed: u64,
}
impl Config {
    pub fn from_manifest(manifest: &Manifest, generation: u32) -> Result<Self, Error> {
        manifest.validate().map_err(|_| Error::Parameters)?;
        let count = manifest
            .generation_symbols(generation)
            .map_err(|_| Error::Parameters)?;
        let mut hash = Sha256::new();
        hash.update(manifest.encode().map_err(|_| Error::Parameters)?);
        hash.update(generation.to_le_bytes());
        let digest = hash.finalize();
        Ok(Self {
            count,
            symbol_size: manifest.symbol_size as usize,
            seed: u64::from_le_bytes(digest[..8].try_into().unwrap()),
        })
    }
    fn validate(&self) -> Result<(), Error> {
        if !(1..=64).contains(&self.count) || self.symbol_size == 0 || self.symbol_size > 65535 {
            return Err(Error::Parameters);
        }
        Ok(())
    }
    pub fn coefficients(&self, index: u32) -> u64 {
        if index < self.count {
            return 1u64 << index;
        }
        let mask = if self.count == 64 {
            u64::MAX
        } else {
            (1u64 << self.count) - 1
        };
        let value = splitmix64(self.seed ^ u64::from(index)) & mask;
        if value == 0 {
            1u64 << (index % self.count)
        } else {
            value
        }
    }
}

/// Precisely specified wrapping arithmetic; never uses the standard library RNG or hashers.
pub fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

pub trait FecEncoder {
    fn encode(&self, index: u32) -> Result<Vec<u8>, Error>;
}
pub trait FecDecoder {
    /// Returns true only when the equation increases rank.
    fn push(&mut self, index: u32, payload: &[u8]) -> Result<bool, Error>;
    fn rank(&self) -> u32;
    fn reconstruct(&self) -> Result<Vec<Vec<u8>>, Error>;
}

pub struct Encoder {
    config: Config,
    sources: Vec<Vec<u8>>,
}
impl Encoder {
    pub fn new(config: Config, sources: Vec<Vec<u8>>) -> Result<Self, Error> {
        config.validate()?;
        if sources.len() != config.count as usize
            || sources.iter().any(|s| s.len() != config.symbol_size)
        {
            return Err(Error::SymbolSize);
        }
        Ok(Self { config, sources })
    }
}
impl FecEncoder for Encoder {
    fn encode(&self, index: u32) -> Result<Vec<u8>, Error> {
        let mut output = vec![0; self.config.symbol_size];
        let coefficients = self.config.coefficients(index);
        for (i, source) in self.sources.iter().enumerate() {
            if coefficients & (1u64 << i) != 0 {
                xor(&mut output, source);
            }
        }
        Ok(output)
    }
}

#[derive(Clone)]
struct Row {
    mask: u64,
    data: Vec<u8>,
}
pub struct Decoder {
    config: Config,
    rows: Vec<Option<Row>>,
    rank: u32,
}
impl Decoder {
    pub fn new(config: Config) -> Result<Self, Error> {
        config.validate()?;
        Ok(Self {
            config,
            rows: vec![None; config.count as usize],
            rank: 0,
        })
    }
}
impl FecDecoder for Decoder {
    fn push(&mut self, index: u32, payload: &[u8]) -> Result<bool, Error> {
        if payload.len() != self.config.symbol_size {
            return Err(Error::SymbolSize);
        }
        let mut row = Row {
            mask: self.config.coefficients(index),
            data: payload.to_vec(),
        };
        for i in 0..self.config.count as usize {
            if row.mask & (1u64 << i) == 0 {
                continue;
            }
            if let Some(pivot) = &self.rows[i] {
                row.mask ^= pivot.mask;
                xor(&mut row.data, &pivot.data);
            } else {
                self.rows[i] = Some(row);
                self.rank += 1;
                return Ok(true);
            }
        }
        if row.data.iter().any(|&byte| byte != 0) {
            return Err(Error::Inconsistent);
        }
        Ok(false)
    }
    fn rank(&self) -> u32 {
        self.rank
    }
    fn reconstruct(&self) -> Result<Vec<Vec<u8>>, Error> {
        if self.rank != self.config.count {
            return Err(Error::Incomplete);
        }
        let mut sources = vec![vec![0; self.config.symbol_size]; self.config.count as usize];
        for i in (0..self.config.count as usize).rev() {
            let row = self.rows[i].as_ref().ok_or(Error::Incomplete)?;
            let mut data = row.data.clone();
            for (j, source) in sources.iter().enumerate().skip(i + 1) {
                if row.mask & (1u64 << j) != 0 {
                    xor(&mut data, source);
                }
            }
            sources[i] = data;
        }
        Ok(sources)
    }
}
fn xor(target: &mut [u8], source: &[u8]) {
    for (dst, &src) in target.iter_mut().zip(source) {
        *dst ^= src;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sources(count: u32) -> Vec<Vec<u8>> {
        (0..count)
            .map(|i| (0..37).map(|j| (i * 37 + j) as u8).collect())
            .collect()
    }
    #[test]
    fn repair_golden() {
        assert_eq!(splitmix64(0), 0xe220a8397b1dcdaf);
        let cfg = Config {
            count: 8,
            symbol_size: 37,
            seed: 0x123456789abcdef0,
        };
        let encoder = Encoder::new(cfg, sources(8)).unwrap();
        let bytes: Vec<u8> = (8..24).flat_map(|id| encoder.encode(id).unwrap()).collect();
        assert_eq!(bytes, include_bytes!("../../../tests/golden/repair-v0.bin"));
    }
    #[test]
    fn loss_duplicates_and_reordering() {
        for count in [1, 8, 63, 64] {
            for loss in [10, 20] {
                let cfg = Config {
                    count,
                    symbol_size: 37,
                    seed: 42,
                };
                let original = sources(count);
                let encoder = Encoder::new(cfg, original.clone()).unwrap();
                let mut decoder = Decoder::new(cfg).unwrap();
                let mut ids: Vec<u32> = (0..count + 64)
                    .filter(|&i| splitmix64(u64::from(i)) % 100 >= loss)
                    .collect();
                ids.sort_by_key(|&i| splitmix64(u64::from(i) + 99));
                for id in ids {
                    let payload = encoder.encode(id).unwrap();
                    decoder.push(id, &payload).unwrap();
                    assert!(!decoder.push(id, &payload).unwrap());
                }
                assert_eq!(decoder.reconstruct().unwrap(), original);
            }
        }
    }
    #[test]
    fn repair_only_late_join() {
        let cfg = Config {
            count: 64,
            symbol_size: 37,
            seed: 9,
        };
        let original = sources(64);
        let encoder = Encoder::new(cfg, original.clone()).unwrap();
        let mut decoder = Decoder::new(cfg).unwrap();
        for id in 400..510 {
            decoder.push(id, &encoder.encode(id).unwrap()).unwrap();
        }
        assert_eq!(decoder.reconstruct().unwrap(), original);
    }
    #[test]
    fn rejects_inconsistent_equations_and_invalid_sizes() {
        let cfg = Config {
            count: 1,
            symbol_size: 2,
            seed: 0,
        };
        let mut decoder = Decoder::new(cfg).unwrap();
        assert_eq!(decoder.reconstruct(), Err(Error::Incomplete));
        decoder.push(0, &[1, 2]).unwrap();
        assert_eq!(decoder.push(0, &[1, 3]), Err(Error::Inconsistent));
        assert_eq!(decoder.push(0, &[1]), Err(Error::SymbolSize));
    }
}
