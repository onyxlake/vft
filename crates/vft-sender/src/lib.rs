//! Simplex scheduling with bounded generation caching and a platform-neutral random access source.
#![no_std]
extern crate alloc;
use alloc::{string::String, vec, vec::Vec};
use core::fmt;
use vft_fec::{Config, Encoder, FecEncoder};
use vft_protocol::repair_request::RepairRequest;
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
    Request(vft_protocol::repair_request::Error),
    Busy,
    RequestConflict,
    ManualLimit,
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
    pub automatic_repairs: u64,
    pub manual_repairs: u64,
    pub generation_loads: u64,
    pub source_read_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyResult {
    Applied,
    Duplicate,
}
struct ManualBatch {
    remaining: Vec<(u32, u32)>,
    cursor: usize,
    consecutive: u32,
}

pub struct SenderSession<S: ObjectSource> {
    manifest: Manifest,
    source: S,
    config: SenderConfig,
    progress: Progress,
    cache: Option<(u32, Encoder)>,
    next_repairs: Option<Vec<u64>>,
    manual: Option<ManualBatch>,
    manual_since_ordinary: u32,
    recent_requests: Vec<RepairRequest>,
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
            next_repairs: None,
            manual: None,
            manual_since_ordinary: 0,
            recent_requests: Vec::new(),
        })
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn progress(&self) -> Progress {
        self.progress
    }
    pub fn manual_remaining(&self) -> u32 {
        self.manual
            .as_ref()
            .map_or(0, |b| b.remaining.iter().map(|(_, n)| *n).sum())
    }
    pub fn cancel_manual(&mut self) {
        self.manual = None;
    }
    fn automatic_index(&self, generation: u32) -> Result<u64, Error> {
        let counter = self.progress.automatic_repairs;
        let cycle = 64 * u64::from(self.manifest.generations());
        let round = counter / cycle;
        let within = counter % cycle;
        let start = u64::from(generation) * 64;
        let used = round
            .checked_mul(64)
            .and_then(|n| n.checked_add(within.saturating_sub(start).min(64)))
            .ok_or(Error::Exhausted)?;
        u64::from(self.manifest.generation_symbols(generation)?)
            .checked_add(used)
            .ok_or(Error::Exhausted)
    }
    /// Validate and reserve all small metadata before replacing any session state.
    pub fn apply_request(&mut self, request: RepairRequest) -> Result<ApplyResult, Error> {
        request.validate(&self.manifest).map_err(Error::Request)?;
        if let Some(id) = request.id {
            if let Some(old) = self.recent_requests.iter().find(|r| r.id == Some(id)) {
                return if old.entries() == request.entries() {
                    Ok(ApplyResult::Duplicate)
                } else {
                    Err(Error::RequestConflict)
                };
            }
        }
        if self.manual.is_some() {
            return Err(Error::Busy);
        }
        // Includes cursor array, bounded recent request contents, and active batch overhead.
        let count = self.manifest.generations() as usize;
        let bytes = count
            .checked_mul(core::mem::size_of::<u64>())
            .and_then(|n| {
                n.checked_add(
                    256 * 64 * core::mem::size_of::<vft_protocol::repair_request::Target>() + 16384,
                )
            })
            .ok_or(Error::ManualLimit)?;
        if bytes > 8 * 1024 * 1024 {
            return Err(Error::ManualLimit);
        }
        let cursors = if self.next_repairs.is_none() {
            let mut values = Vec::new();
            values
                .try_reserve_exact(count)
                .map_err(|_| Error::ManualLimit)?;
            for generation in 0..self.manifest.generations() {
                values.push(self.automatic_index(generation)?);
            }
            Some(values)
        } else {
            None
        };
        let mut remaining = Vec::new();
        remaining
            .try_reserve_exact(request.targets.len())
            .map_err(|_| Error::ManualLimit)?;
        for target in &request.targets {
            let next = self.next_repairs.as_ref().or(cursors.as_ref()).unwrap()
                [target.generation as usize];
            if next
                .checked_add(u64::from(target.budget().map_err(Error::Request)?))
                .is_none_or(|n| n > u64::from(u32::MAX) + 1)
            {
                return Err(Error::Exhausted);
            }
            remaining.push((target.generation, target.budget().map_err(Error::Request)?));
        }
        if request.id.is_some() {
            self.recent_requests
                .try_reserve(1)
                .map_err(|_| Error::ManualLimit)?;
        }
        if let Some(cursors) = cursors {
            self.next_repairs = Some(cursors);
        }
        self.manual = Some(ManualBatch {
            remaining,
            cursor: 0,
            consecutive: 0,
        });
        if request.id.is_some() {
            if self.recent_requests.len() == 64 {
                self.recent_requests.remove(0);
            }
            self.recent_requests.push(request);
        }
        Ok(ApplyResult::Applied)
    }
    pub fn next_packet(&mut self) -> Result<Packet, Error> {
        let next_frame = self
            .progress
            .frames
            .checked_add(1)
            .ok_or(Error::Exhausted)?;
        let mut manual_packet = false;
        let (kind, symbol, payload) = if self.manifest.source_symbols == 0
            || self
                .progress
                .frames
                .is_multiple_of(self.config.manifest_interval)
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
            let (kind, generation, index) = if let Some(batch) = self
                .manual
                .as_ref()
                .filter(|_| self.manual_since_ordinary < 32)
            {
                manual_packet = true;
                let generation = batch.remaining[batch.cursor].0;
                let index = self.next_repairs.as_ref().unwrap()[generation as usize];
                (
                    PacketType::RepairSymbol,
                    generation,
                    u32::try_from(index).map_err(|_| Error::Exhausted)?,
                )
            } else if self.progress.source_symbols < u64::from(self.manifest.source_symbols) {
                let global = self.progress.source_symbols as u32;
                (
                    PacketType::SourceSymbol,
                    global / GENERATION_SIZE,
                    global % GENERATION_SIZE,
                )
            } else {
                // Send one bounded generation of repairs before seeking to the next generation.
                let counter = self.progress.automatic_repairs;
                let generations = u64::from(self.manifest.generations());
                let generation = ((counter / 64) % generations) as u32;
                let index = if let Some(cursors) = &self.next_repairs {
                    cursors[generation as usize]
                } else {
                    self.automatic_index(generation)?
                };
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
        self.progress.frames = next_frame;
        match kind {
            PacketType::Manifest => self.progress.manifests += 1,
            PacketType::SourceSymbol => {
                self.progress.source_symbols += 1;
                self.manual_since_ordinary = 0;
            }
            PacketType::RepairSymbol => {
                self.progress.repair_symbols += 1;
                if let Some(cursors) = &mut self.next_repairs {
                    cursors[symbol.generation as usize] += 1;
                }
                if manual_packet {
                    self.progress.manual_repairs += 1;
                    self.manual_since_ordinary += 1;
                    let batch = self.manual.as_mut().unwrap();
                    batch.remaining[batch.cursor].1 -= 1;
                    batch.consecutive += 1;
                    if batch.remaining.iter().all(|(_, n)| *n == 0) {
                        self.manual = None;
                    } else if batch.consecutive == 8 || batch.remaining[batch.cursor].1 == 0 {
                        loop {
                            batch.cursor = (batch.cursor + 1) % batch.remaining.len();
                            if batch.remaining[batch.cursor].1 > 0 {
                                break;
                            }
                        }
                        batch.consecutive = 0;
                    }
                } else {
                    self.progress.automatic_repairs += 1;
                    self.manual_since_ordinary = 0;
                }
            }
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
            self.progress.source_read_bytes += length as u64;
            sources.push(source);
        }
        self.cache = Some((generation, Encoder::new(config, sources)?));
        self.progress.generation_loads += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vft_protocol::{Digest, ProfileId, Sha256};
    fn fixture(data: &[u8]) -> (Manifest, SenderConfig) {
        let m = Manifest {
            session: [1; 16],
            object: [2; 16],
            filename: String::from("test.bin"),
            file_size: data.len() as u64,
            sha256: Sha256::digest(data).into(),
            symbol_size: 1,
            source_symbols: data.len() as u32,
            profile: ProfileId::Balanced,
        };
        let c = SenderConfig {
            grid: GridConfig::for_profile(m.profile),
            manifest_interval: 30,
        };
        (m, c)
    }
    #[test]
    fn baseline_sequence_and_manual_fairness_unique_indices() {
        let data = [7; 130];
        let (m, c) = fixture(&data);
        let mut baseline = SenderSession::new(m.clone(), &data[..], c).unwrap();
        for _ in 0..500 {
            let before = baseline.progress();
            let p = baseline.next_packet().unwrap();
            if p.kind == PacketType::RepairSymbol {
                let n = before.repair_symbols;
                assert_eq!(p.symbol.generation, ((n / 64) % 3) as u32);
                assert_eq!(
                    u64::from(p.symbol.index),
                    u64::from(m.generation_symbols(p.symbol.generation).unwrap())
                        + (n / 192) * 64
                        + n % 64
                );
            }
        }
        let mut sender = SenderSession::new(m.clone(), &data[..], c).unwrap();
        let mut seen = alloc::collections::BTreeSet::new();
        let mut sources = alloc::collections::BTreeSet::new();
        let mut ordinary_groups = alloc::collections::BTreeSet::new();
        for frame in 0..12000 {
            if sender.manual_remaining() == 0 {
                let r = RepairRequest::parse("1:64,2:64,3:2", &m).unwrap();
                sender.apply_request(r).unwrap();
            }
            let automatic_before = sender.progress().automatic_repairs;
            let p = sender.next_packet().unwrap();
            if sender.progress().automatic_repairs > automatic_before {
                ordinary_groups.insert(p.symbol.generation);
            }
            assert_eq!(p.kind == PacketType::Manifest, frame % 30 == 0);
            match p.kind {
                PacketType::SourceSymbol => assert!(sources.insert(p.symbol)),
                PacketType::RepairSymbol => assert!(seen.insert(p.symbol)),
                _ => {}
            };
        }
        assert_eq!(sources.len(), 130);
        assert!(sender.progress().automatic_repairs > 0);
        assert_eq!(ordinary_groups.len(), 3);
        for generation in 0..3 {
            assert!(seen.iter().any(|s| s.generation == generation));
        }
        sender.cancel_manual();
        assert_eq!(sender.manual_remaining(), 0);
    }
    #[test]
    fn idempotency_busy_cancel_and_exhaustion() {
        let data = [9; 65];
        let (m, c) = fixture(&data);
        let mut s = SenderSession::new(m.clone(), &data[..], c).unwrap();
        let r = RepairRequest {
            id: Some(1),
            targets: vec![vft_protocol::repair_request::Target {
                generation: 0,
                deficit: Some(3),
            }],
        };
        assert_eq!(s.apply_request(r.clone()).unwrap(), ApplyResult::Applied);
        assert_eq!(s.apply_request(r.clone()).unwrap(), ApplyResult::Duplicate);
        let mut different = r.clone();
        different.targets[0].deficit = Some(4);
        assert!(matches!(
            s.apply_request(different),
            Err(Error::RequestConflict)
        ));
        assert!(matches!(
            s.apply_request(RepairRequest::parse("2", &m).unwrap()),
            Err(Error::Busy)
        ));
        s.next_packet().unwrap();
        s.next_packet().unwrap();
        s.cancel_manual();
        assert_eq!(s.apply_request(r).unwrap(), ApplyResult::Duplicate);
        let p = s.next_packet().unwrap();
        assert_eq!(p.kind, PacketType::SourceSymbol);
        assert_eq!(p.symbol.index, 0);
        s.next_repairs.as_mut().unwrap()[0] = u64::from(u32::MAX);
        assert!(matches!(
            s.apply_request(RepairRequest::parse("1", &m).unwrap()),
            Err(Error::Exhausted)
        ));
        assert_eq!(s.manual_remaining(), 0);
        s.progress.source_symbols = 65;
        assert_eq!(s.next_packet().unwrap().symbol.index, u32::MAX);
        assert!(matches!(s.next_packet(), Err(Error::Exhausted)));
    }
    #[test]
    fn failed_manual_read_preserves_budget_and_index() {
        struct Flaky {
            fail: bool,
        }
        impl ObjectSource for Flaky {
            fn read_exact_at(&mut self, _: u64, b: &mut [u8]) -> Result<(), String> {
                if self.fail {
                    self.fail = false;
                    return Err(String::from("injected"));
                }
                b.fill(9);
                Ok(())
            }
        }
        let (m, c) = fixture(&[9; 65]);
        let mut s = SenderSession::new(m.clone(), Flaky { fail: true }, c).unwrap();
        s.apply_request(RepairRequest::parse("2:1", &m).unwrap())
            .unwrap();
        s.next_packet().unwrap();
        assert!(matches!(s.next_packet(), Err(Error::Source(_))));
        assert_eq!(s.manual_remaining(), 8);
        assert_eq!(s.progress().frames, 1);
        let p = s.next_packet().unwrap();
        assert_eq!(
            p.symbol,
            SymbolId {
                generation: 1,
                index: 1
            }
        );
        assert_eq!(s.manual_remaining(), 7);
    }
    #[test]
    fn lazy_cursor_initialization_after_partial_automatic_round() {
        let data = [9; 130];
        let (m, c) = fixture(&data);
        let mut s = SenderSession::new(m.clone(), &data[..], c).unwrap();
        let mut seen = alloc::collections::BTreeSet::new();
        while s.progress().automatic_repairs < 215 {
            let p = s.next_packet().unwrap();
            if p.kind == PacketType::RepairSymbol {
                assert!(seen.insert(p.symbol));
            }
        }
        s.apply_request(RepairRequest::parse("1,2,3", &m).unwrap())
            .unwrap();
        for _ in 0..300 {
            let p = s.next_packet().unwrap();
            if p.kind == PacketType::RepairSymbol {
                assert!(seen.insert(p.symbol));
            }
        }
    }
    #[test]
    fn manual_metadata_limit_leaves_automatic_transmission_available() {
        let data = [9; 64];
        let (mut m, c) = fixture(&data);
        m.file_size = 1_000_000_000;
        m.source_symbols = 1_000_000_000;
        let mut s = SenderSession::new(m.clone(), &data[..], c).unwrap();
        assert!(matches!(
            s.apply_request(RepairRequest::parse("1", &m).unwrap()),
            Err(Error::ManualLimit)
        ));
        assert!(s.next_repairs.is_none());
        assert_eq!(s.next_packet().unwrap().kind, PacketType::Manifest);
        assert_eq!(s.next_packet().unwrap().kind, PacketType::SourceSymbol);
    }
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
