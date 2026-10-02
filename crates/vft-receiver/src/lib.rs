//! Platform-neutral discovery, bounded duplicate tracking, FEC reconstruction and SHA-256 verification.
#![no_std]
extern crate alloc;
use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    vec::Vec,
};
use core::fmt;
use vft_fec::{Config, Decoder, FecDecoder};
use vft_protocol::{Digest, Manifest, Packet, PacketType, Sha256, SymbolId, GENERATION_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HashStatus {
    #[default]
    Pending,
    Verified,
    Failed,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub received_packets: u64,
    pub valid_symbols: u64,
    pub duplicate_symbols: u64,
    pub dependent_symbols: u64,
    pub invalid_packets: u64,
    pub ignored_packets: u64,
    pub rank_symbols: u64,
    pub recovered_bytes: u64,
    pub completed_generations: u32,
    pub hash: HashStatus,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_file_size: u64,
    pub max_source_symbols: u32,
    pub duplicate_window: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_size: 256 * 1024 * 1024,
            max_source_symbols: 1_000_000,
            duplicate_window: 4096,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Manifest,
    Symbol,
    Duplicate,
    Ignored,
    Complete,
}
#[derive(Debug)]
pub enum Error {
    Protocol(vft_protocol::Error),
    Fec(vft_fec::Error),
    Limit,
    ConflictingManifest,
    Symbol,
    HashMismatch,
    Incomplete,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VFT receiver: {self:?}")
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

pub struct ReceiverSession {
    limits: Limits,
    manifest: Option<Manifest>,
    decoders: BTreeMap<u32, Decoder>,
    recovered: BTreeMap<u32, Vec<u8>>,
    seen: BTreeSet<SymbolId>,
    recent: VecDeque<SymbolId>,
    progress: Progress,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenerationProgress {
    pub generation: u32,
    pub source_count: u32,
    pub rank: u32,
    pub complete: bool,
}
impl Default for ReceiverSession {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}
impl ReceiverSession {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            manifest: None,
            decoders: BTreeMap::new(),
            recovered: BTreeMap::new(),
            seen: BTreeSet::new(),
            recent: VecDeque::new(),
            progress: Progress::default(),
        }
    }
    pub fn manifest(&self) -> Option<&Manifest> {
        self.manifest.as_ref()
    }
    pub fn progress(&self) -> Progress {
        self.progress
    }
    /// Lazy read-only progress, including unseen generations without allocating decoders.
    pub fn generation_progress(&self) -> impl Iterator<Item = GenerationProgress> + '_ {
        (0..self.manifest.as_ref().map_or(0, Manifest::generations)).map(|generation| {
            let source_count = self
                .manifest
                .as_ref()
                .unwrap()
                .generation_symbols(generation)
                .unwrap();
            let complete = self.recovered.contains_key(&generation);
            let rank = if complete {
                source_count
            } else {
                self.decoders.get(&generation).map_or(0, |d| d.rank())
            };
            GenerationProgress {
                generation,
                source_count,
                rank,
                complete,
            }
        })
    }
    pub fn is_complete(&self) -> bool {
        self.progress.hash == HashStatus::Verified
    }
    /// Accept wire bytes so CRC validation cannot be accidentally skipped.
    pub fn push_packet(&mut self, bytes: &[u8]) -> Result<Event, Error> {
        self.progress.received_packets += 1;
        let result = Packet::decode(bytes)
            .map_err(Error::from)
            .and_then(|packet| self.push_validated(packet));
        if result.is_err() {
            self.progress.invalid_packets += 1;
        }
        result
    }
    fn push_validated(&mut self, packet: Packet) -> Result<Event, Error> {
        if let Some(manifest) = &self.manifest {
            if packet.session != manifest.session || packet.object != manifest.object {
                self.progress.ignored_packets += 1;
                return Ok(Event::Ignored);
            }
        }
        if packet.kind == PacketType::Manifest {
            let manifest = Manifest::decode(&packet.payload)?;
            if manifest.session != packet.session
                || manifest.object != packet.object
                || manifest.profile != packet.profile
                || packet.symbol
                    != (SymbolId {
                        generation: 0,
                        index: 0,
                    })
            {
                return Err(Error::ConflictingManifest);
            }
            if let Some(existing) = &self.manifest {
                if existing != &manifest {
                    return Err(Error::ConflictingManifest);
                }
                return Ok(Event::Manifest);
            }
            if manifest.file_size > self.limits.max_file_size
                || manifest.source_symbols > self.limits.max_source_symbols
            {
                return Err(Error::Limit);
            }
            self.manifest = Some(manifest);
            self.verify_if_ready()?;
            return Ok(if self.is_complete() {
                Event::Complete
            } else {
                Event::Manifest
            });
        }
        let Some(manifest) = &self.manifest else {
            self.progress.ignored_packets += 1;
            return Ok(Event::Ignored);
        };
        if packet.kind == PacketType::Control {
            self.progress.ignored_packets += 1;
            return Ok(Event::Ignored);
        }
        let count = manifest.generation_symbols(packet.symbol.generation)?;
        if packet.profile != manifest.profile
            || packet.payload.len() != manifest.symbol_size as usize
            || (packet.kind == PacketType::SourceSymbol && packet.symbol.index >= count)
            || (packet.kind == PacketType::RepairSymbol && packet.symbol.index < count)
        {
            return Err(Error::Symbol);
        }
        if self.seen.contains(&packet.symbol) {
            self.progress.duplicate_symbols += 1;
            return Ok(Event::Duplicate);
        }
        if self.recovered.contains_key(&packet.symbol.generation) {
            self.progress.ignored_packets += 1;
            self.remember(packet.symbol);
            return Ok(Event::Ignored);
        }
        let generation = packet.symbol.generation;
        if let alloc::collections::btree_map::Entry::Vacant(entry) = self.decoders.entry(generation)
        {
            entry.insert(Decoder::new(Config::from_manifest(manifest, generation)?)?);
        }
        let decoder = self.decoders.get_mut(&generation).ok_or(Error::Symbol)?;
        let innovative = decoder.push(packet.symbol.index, &packet.payload)?;
        self.progress.valid_symbols += 1;
        if innovative {
            self.progress.rank_symbols += 1;
        } else {
            self.progress.dependent_symbols += 1;
        }
        if decoder.rank() == count {
            let symbols = decoder.reconstruct()?;
            let mut data: Vec<u8> = symbols.into_iter().flatten().collect();
            let offset = u64::from(generation)
                * u64::from(GENERATION_SIZE)
                * u64::from(manifest.symbol_size);
            data.truncate((manifest.file_size - offset).min(data.len() as u64) as usize);
            self.progress.recovered_bytes += data.len() as u64;
            self.progress.completed_generations += 1;
            self.recovered.insert(generation, data);
            self.decoders.remove(&generation);
        }
        self.remember(packet.symbol);
        self.verify_if_ready()?;
        Ok(if self.is_complete() {
            Event::Complete
        } else {
            Event::Symbol
        })
    }
    fn remember(&mut self, symbol: SymbolId) {
        if self.limits.duplicate_window == 0 {
            return;
        }
        if self.recent.len() == self.limits.duplicate_window {
            if let Some(old) = self.recent.pop_front() {
                self.seen.remove(&old);
            }
        }
        self.seen.insert(symbol);
        self.recent.push_back(symbol);
    }
    fn verify_if_ready(&mut self) -> Result<(), Error> {
        if self.progress.hash == HashStatus::Failed {
            return Err(Error::HashMismatch);
        }
        if self.is_complete() {
            return Ok(());
        }
        let Some(manifest) = &self.manifest else {
            return Ok(());
        };
        if self.progress.completed_generations != manifest.generations() {
            return Ok(());
        }
        let mut hash = Sha256::new();
        for data in self.recovered.values() {
            hash.update(data);
        }
        let digest: [u8; 32] = hash.finalize().into();
        if digest != manifest.sha256 {
            self.progress.hash = HashStatus::Failed;
            return Err(Error::HashMismatch);
        }
        self.progress.hash = HashStatus::Verified;
        Ok(())
    }
    /// Ordered verified chunks; applications can stream these to a file without another full copy.
    pub fn verified_chunks(&self) -> Result<impl Iterator<Item = &[u8]>, Error> {
        if !self.is_complete() {
            return Err(Error::Incomplete);
        }
        Ok(self.recovered.values().map(Vec::as_slice))
    }
    pub fn reconstruct(&self) -> Result<Vec<u8>, Error> {
        Ok(self.verified_chunks()?.flatten().copied().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::String, vec};
    use vft_protocol::ProfileId;
    fn manifest_packet(sha256: [u8; 32], file_size: u64) -> Packet {
        let manifest = Manifest {
            session: [1; 16],
            object: [2; 16],
            filename: String::from("test.bin"),
            file_size,
            sha256,
            symbol_size: 1,
            source_symbols: file_size as u32,
            profile: ProfileId::Robust,
        };
        Packet {
            session: manifest.session,
            object: manifest.object,
            profile: manifest.profile,
            kind: PacketType::Manifest,
            symbol: SymbolId {
                generation: 0,
                index: 0,
            },
            payload: manifest.encode().unwrap(),
        }
    }
    #[test]
    fn empty_file_and_crc() {
        let packet = manifest_packet(Sha256::digest([]).into(), 0);
        let mut receiver = ReceiverSession::default();
        let mut bytes = packet.encode().unwrap();
        bytes[20] ^= 1;
        assert!(matches!(
            receiver.push_packet(&bytes),
            Err(Error::Protocol(vft_protocol::Error::Crc))
        ));
        assert_eq!(
            receiver.push_packet(&packet.encode().unwrap()).unwrap(),
            Event::Complete
        );
        assert_eq!(receiver.reconstruct().unwrap(), vec![]);
        assert_eq!(receiver.generation_progress().count(), 0);
    }
    #[test]
    fn generation_progress_includes_unseen_short_and_completed_groups() {
        let mut r = ReceiverSession::default();
        assert_eq!(r.generation_progress().count(), 0);
        let p = manifest_packet(Sha256::digest([7; 65]).into(), 65);
        r.push_packet(&p.encode().unwrap()).unwrap();
        let groups: Vec<_> = r.generation_progress().collect();
        assert_eq!(
            groups[0],
            GenerationProgress {
                generation: 0,
                source_count: 64,
                rank: 0,
                complete: false
            }
        );
        assert_eq!(groups[1].source_count, 1);
        assert!(r.decoders.is_empty());
        let source = Packet {
            kind: PacketType::SourceSymbol,
            symbol: SymbolId {
                generation: 1,
                index: 0,
            },
            payload: vec![7],
            ..p
        };
        r.push_packet(&source.encode().unwrap()).unwrap();
        assert_eq!(
            r.generation_progress().nth(1).unwrap(),
            GenerationProgress {
                generation: 1,
                source_count: 1,
                rank: 1,
                complete: true
            }
        );
    }
    #[test]
    fn hash_mismatch_never_completes() {
        let packet = manifest_packet([0; 32], 1);
        let mut receiver = ReceiverSession::default();
        receiver.push_packet(&packet.encode().unwrap()).unwrap();
        let source = Packet {
            kind: PacketType::SourceSymbol,
            payload: vec![7],
            ..packet
        };
        assert!(matches!(
            receiver.push_packet(&source.encode().unwrap()),
            Err(Error::HashMismatch)
        ));
        assert!(!receiver.is_complete());
        assert!(receiver.reconstruct().is_err());
    }
    #[test]
    fn limits_and_conflicting_manifest() {
        let packet = manifest_packet([0; 32], 2);
        let mut receiver = ReceiverSession::new(Limits {
            max_file_size: 1,
            ..Limits::default()
        });
        assert!(matches!(
            receiver.push_packet(&packet.encode().unwrap()),
            Err(Error::Limit)
        ));
        let mut receiver = ReceiverSession::default();
        receiver.push_packet(&packet.encode().unwrap()).unwrap();
        let changed = manifest_packet([1; 32], 2);
        assert!(matches!(
            receiver.push_packet(&changed.encode().unwrap()),
            Err(Error::ConflictingManifest)
        ));
        assert_eq!(receiver.manifest().unwrap().sha256, [0; 32]);
    }
}
