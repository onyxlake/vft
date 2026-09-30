//! Simplex scheduling with bounded generation caching and a platform-neutral random access source.
#![no_std]
extern crate alloc;
use alloc::{string::String, vec, vec::Vec};
use core::fmt;
use vft_fec::{Config, Encoder, FecEncoder};
use vft_protocol::{Manifest, Packet, PacketType, SymbolId, GENERATION_SIZE};
use vft_visual::{GridConfig, VisualFrame};

pub trait ObjectSource {
    fn read_exact_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String>;
}
impl ObjectSource for &[u8] {
    fn read_exact_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
        let start = usize::try_from(offset).map_err(|_| String::from("offset overflow"))?;
        let end = start
            .checked_add(buffer.len())
            .ok_or_else(|| String::from("offset overflow"))?;
        let data = self
            .get(start..end)
            .ok_or_else(|| String::from("source truncated"))?;
        buffer.copy_from_slice(data);
        Ok(())
    }
}
#[derive(Debug)]
pub enum Error {
    Protocol(vft_protocol::Error),
    Fec(vft_fec::Error),
    Visual(vft_visual::Error),
    Source(String),
    Configuration,
    Exhausted,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VFT sender: {self:?}")
    }
}
impl core::error::Error for Error {}
impl From<vft_protocol::Error> for Error {
    fn from(e: vft_protocol::Error) -> Self {
        Self::Protocol(e)
    }
}
impl From<vft_fec::Error> for Error {
    fn from(e: vft_fec::Error) -> Self {
        Self::Fec(e)
    }
}
impl From<vft_visual::Error> for Error {
    fn from(e: vft_visual::Error) -> Self {
        Self::Visual(e)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SenderConfig {
    pub grid: GridConfig,
    pub manifest_interval: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub frames: u64,
    pub manifests: u64,
    pub source_symbols: u64,
    pub repair_symbols: u64,
}

pub struct SenderSession<S: ObjectSource> {
    manifest: Manifest,
    source: S,
    config: SenderConfig,
    progress: Progress,
    cache: Option<(u32, Encoder)>,
}
impl<S: ObjectSource> SenderSession<S> {
    pub fn new(manifest: Manifest, source: S, config: SenderConfig) -> Result<Self, Error> {
        manifest.validate()?;
        if config.manifest_interval < 2
            || manifest.symbol_size as usize > config.grid.payload_capacity(manifest.profile)?
            || manifest.encode()?.len() > config.grid.payload_capacity(manifest.profile)?
        {
            return Err(Error::Configuration);
        }
        Ok(Self {
            manifest,
            source,
            config,
            progress: Progress::default(),
            cache: None,
        })
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn progress(&self) -> Progress {
        self.progress
    }
    pub fn next_packet(&mut self) -> Result<Packet, Error> {
        let (kind, symbol, payload) = if self.manifest.source_symbols == 0
            || self.progress.frames % self.config.manifest_interval == 0
        {
            (
                PacketType::Manifest,
                SymbolId {
                    generation: 0,
                    index: 0,
                },
                self.manifest.encode()?,
            )
        } else {
            let (kind, generation, index) =
                if self.progress.source_symbols < u64::from(self.manifest.source_symbols) {
                    let global = self.progress.source_symbols as u32;
                    (
                        PacketType::SourceSymbol,
                        global / GENERATION_SIZE,
                        global % GENERATION_SIZE,
                    )
                } else {
                    // Send one bounded generation of repairs before seeking to the next generation.
                    let counter = self.progress.repair_symbols;
                    let generations = u64::from(self.manifest.generations());
                    let generation = ((counter / 64) % generations) as u32;
                    let round = counter / (64 * generations);
                    let index = u64::from(self.manifest.generation_symbols(generation)?)
                        + round * 64
                        + counter % 64;
                    (
                        PacketType::RepairSymbol,
                        generation,
                        u32::try_from(index).map_err(|_| Error::Exhausted)?,
                    )
                };
            self.load_generation(generation)?;
            let payload = self
                .cache
                .as_ref()
                .ok_or(Error::Configuration)?
                .1
                .encode(index)?;
            (kind, SymbolId { generation, index }, payload)
        };
        self.progress.frames = self
            .progress
            .frames
            .checked_add(1)
            .ok_or(Error::Exhausted)?;
        match kind {
            PacketType::Manifest => self.progress.manifests += 1,
            PacketType::SourceSymbol => self.progress.source_symbols += 1,
            PacketType::RepairSymbol => self.progress.repair_symbols += 1,
            PacketType::Control => {}
        }
        Ok(Packet {
            session: self.manifest.session,
            object: self.manifest.object,
            kind,
            symbol,
            profile: self.manifest.profile,
            payload,
        })
    }
    pub fn next_frame(&mut self) -> Result<VisualFrame, Error> {
        let packet = self.next_packet()?;
        Ok(VisualFrame::encode(&packet, self.config.grid)?)
    }
    fn load_generation(&mut self, generation: u32) -> Result<(), Error> {
        if self.cache.as_ref().is_some_and(|(id, _)| *id == generation) {
            return Ok(());
        }
        // Release the old generation before loading the next one.
        self.cache = None;
        let config = Config::from_manifest(&self.manifest, generation)?;
        let mut sources: Vec<Vec<u8>> = Vec::with_capacity(config.count as usize);
        for index in 0..config.count {
            let global = generation * GENERATION_SIZE + index;
            let offset = self.manifest.symbol_offset(global)?;
            let length = (self.manifest.file_size - offset)
                .min(u64::from(self.manifest.symbol_size)) as usize;
            let mut source = vec![0; config.symbol_size];
            self.source
                .read_exact_at(offset, &mut source[..length])
                .map_err(Error::Source)?;
            sources.push(source);
        }
        self.cache = Some((generation, Encoder::new(config, sources)?));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vft_protocol::{Digest, ProfileId, Sha256};
    #[test]
    fn schedules_manifest_sources_and_unbounded_repairs() {
        let data = [9u8; 65];
        let manifest = Manifest {
            session: [1; 16],
            object: [2; 16],
            filename: String::from("test.bin"),
            file_size: 65,
            sha256: Sha256::digest(data).into(),
            symbol_size: 1,
            source_symbols: 65,
            profile: ProfileId::Balanced,
        };
        let config = SenderConfig {
            grid: GridConfig::for_profile(manifest.profile),
            manifest_interval: 8,
        };
        let mut sender = SenderSession::new(manifest, &data[..], config).unwrap();
        for i in 0..300 {
            let packet = sender.next_packet().unwrap();
            assert_eq!(packet.kind == PacketType::Manifest, i % 8 == 0);
            if packet.kind == PacketType::SourceSymbol {
                assert!(packet.symbol.index < 64);
            }
        }
        assert_eq!(sender.progress().source_symbols, 65);
        assert!(sender.progress().repair_symbols > 100);
    }
    #[test]
    fn truncated_sources_fail_without_advancing() {
        let manifest = Manifest {
            session: [1; 16],
            object: [2; 16],
            filename: String::from("test.bin"),
            file_size: 1,
            sha256: [0; 32],
            symbol_size: 1,
            source_symbols: 1,
            profile: ProfileId::Robust,
        };
        let config = SenderConfig {
            grid: GridConfig::for_profile(manifest.profile),
            manifest_interval: 8,
        };
        let mut sender = SenderSession::new(manifest, &[][..], config).unwrap();
        sender.next_packet().unwrap();
        assert!(matches!(sender.next_packet(), Err(Error::Source(_))));
        assert_eq!(sender.progress().frames, 1);
    }
}
