//! File, pixel and recovery adapters shared by the Linux applications.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use vft_protocol::{Digest, Manifest, Packet, ProfileId, Sha256};
use vft_receiver::{Limits, ReceiverSession};
use vft_sender::{ObjectSource, SenderConfig, SenderSession};
use vft_visual::{locate, GridConfig, ImageRect, LumaImage, VisualFrame};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct FileSource {
    file: File,
    length: u64,
    modified: SystemTime,
}
impl FileSource {
    fn unchanged(&self) -> std::io::Result<()> {
        let metadata = self.file.metadata()?;
        if metadata.len() != self.length || metadata.modified()? != self.modified {
            return Err(std::io::Error::other(
                "input file changed during transmission",
            ));
        }
        Ok(())
    }
}
impl ObjectSource for FileSource {
    fn read_exact_at(&mut self, offset: u64, buffer: &mut [u8]) -> std::result::Result<(), String> {
        self.unchanged()
            .and_then(|_| self.file.seek(SeekFrom::Start(offset)))
            .and_then(|_| self.file.read_exact(buffer))
            .map_err(|e| e.to_string())
    }
}
pub fn open_sender(path: &Path, profile: ProfileId) -> Result<SenderSession<FileSource>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err("input must be a regular file".into());
    }
    let mut source = FileSource {
        file,
        length: metadata.len(),
        modified: metadata.modified()?,
    };
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut remaining = source.length;
    while remaining > 0 {
        let n = remaining.min(buffer.len() as u64) as usize;
        source.file.read_exact(&mut buffer[..n])?;
        hash.update(&buffer[..n]);
        remaining -= n as u64;
    }
    source.unchanged()?;
    let sha256: [u8; 32] = hash.finalize().into();
    // Session identity is distinct from object identity. No cryptographic secrecy is claimed.
    let mut hash = Sha256::new();
    hash.update(sha256);
    hash.update(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_nanos()
            .to_le_bytes(),
    );
    hash.update(std::process::id().to_le_bytes());
    let grid = GridConfig::for_profile(profile);
    let symbol_size = grid.payload_capacity(profile)? as u32;
    let manifest = Manifest {
        session: hash.finalize()[..16].try_into()?,
        object: sha256[..16].try_into()?,
        filename: path
            .file_name()
            .ok_or("missing filename")?
            .to_string_lossy()
            .into_owned(),
        file_size: source.length,
        sha256,
        symbol_size,
        source_symbols: u32::try_from(source.length.div_ceil(u64::from(symbol_size)))?,
        profile,
    };
    Ok(SenderSession::new(
        manifest,
        source,
        SenderConfig {
            grid,
            manifest_interval: 30,
        },
    )?)
}

/// The only negotiated capture formats: one packed four-byte RGB plane, CPU mapped.
#[derive(Clone, Copy, Debug)]
pub enum PixelOrder {
    Rgb,
    Bgr,
}
/// Respect chunk offset, size and padded/negative stride. Reject truncated or excessive buffers
/// before allocation. The caller supplies the valid SPA chunk, not the whole mmap capacity.
pub fn packed_luma(
    data: &[u8],
    width: usize,
    height: usize,
    stride: i32,
    order: PixelOrder,
) -> Result<LumaImage> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("capture dimensions must be 1..8192".into());
    }
    let row = width * 4;
    let pitch = stride.unsigned_abs() as usize;
    if pitch < row
        || (height - 1)
            .checked_mul(pitch)
            .and_then(|n| n.checked_add(row))
            .is_none_or(|n| n > data.len())
    {
        return Err("truncated capture buffer or invalid stride".into());
    }
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let y = if stride < 0 { height - 1 - y } else { y };
        for p in data[y * pitch..y * pitch + row].chunks_exact(4) {
            let (r, b) = match order {
                PixelOrder::Rgb => (p[0], p[2]),
                PixelOrder::Bgr => (p[2], p[0]),
            };
            pixels
                .push(((u32::from(r) * 77 + u32::from(p[1]) * 150 + u32::from(b) * 29) >> 8) as u8);
        }
    }
    Ok(LumaImage {
        width,
        height,
        pixels,
    })
}

pub fn save_verified(receiver: &ReceiverSession, path: &Path) -> Result<()> {
    let chunks = receiver.verified_chunks()?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    for chunk in chunks {
        file.write_all(chunk)?;
    }
    file.sync_all()?;
    Ok(())
}
pub fn save_preview(image: &LumaImage, path: &Path) -> Result<()> {
    image.validate()?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    // Top-down 32-bit BMP; no second full-frame RGB allocation.
    file.write_all(b"BM")?;
    file.write_all(&(54u32 + (image.pixels.len() * 4) as u32).to_le_bytes())?;
    file.write_all(&[0; 4])?;
    file.write_all(&54u32.to_le_bytes())?;
    file.write_all(&40u32.to_le_bytes())?;
    file.write_all(&(image.width as i32).to_le_bytes())?;
    file.write_all(&(-(image.height as i32)).to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&32u16.to_le_bytes())?;
    file.write_all(&[0; 24])?;
    let mut row = Vec::with_capacity(image.width * 4);
    for line in image.pixels.chunks_exact(image.width) {
        row.clear();
        for &v in line {
            row.extend_from_slice(&[v, v, v, 0]);
        }
        file.write_all(&row)?;
    }
    file.sync_all()?;
    Ok(())
}

pub struct Decoder {
    pub receiver: ReceiverSession,
    pub lock: Option<ImageRect>,
    pub locks: u64,
    pub scans: u64,
    pub invalid: u64,
    profile: Option<ProfileId>,
    manual: bool,
    search: Option<ImageRect>,
    failed_since: Option<Instant>,
    last_scan: Option<Instant>,
    size: (usize, usize),
}
impl Decoder {
    pub fn new(
        profile: Option<ProfileId>,
        rect: Option<ImageRect>,
        search: Option<ImageRect>,
        max_file_size: u64,
    ) -> Self {
        Self {
            receiver: ReceiverSession::new(Limits {
                max_file_size,
                ..Limits::default()
            }),
            lock: rect,
            locks: 0,
            scans: 0,
            invalid: 0,
            profile,
            manual: rect.is_some(),
            search,
            failed_since: None,
            last_scan: None,
            size: (0, 0),
        }
    }
    fn accepts(&self, p: &Packet) -> bool {
        self.receiver
            .manifest()
            .is_none_or(|m| p.session == m.session && p.object == m.object)
    }
    pub fn push(&mut self, image: &LumaImage, now: Instant) -> Result<Option<ImageRect>> {
        image.validate()?;
        if self.size != (image.width, image.height) {
            self.size = (image.width, image.height);
            if !self.manual {
                self.lock = None;
                self.last_scan = None;
            }
        }
        let decoded = if let Some(rect) = self.lock {
            if self.manual
                && (rect
                    .x
                    .checked_add(rect.width)
                    .is_none_or(|n| n > image.width)
                    || rect
                        .y
                        .checked_add(rect.height)
                        .is_none_or(|n| n > image.height))
            {
                return Err("manual capture rectangle is outside the selected stream".into());
            }
            [ProfileId::Balanced, ProfileId::Robust]
                .into_iter()
                .filter(|p| self.profile.is_none_or(|s| s == *p))
                .find_map(|p| {
                    VisualFrame::sample_rect(image, GridConfig::for_profile(p), rect)
                        .and_then(|f| f.decode())
                        .ok()
                        .filter(|packet| packet.profile == p && self.accepts(packet))
                })
                .map(|p| (rect, p))
        } else {
            if self
                .last_scan
                .is_some_and(|t| now.duration_since(t) < Duration::from_millis(500))
            {
                return Ok(None);
            }
            self.last_scan = Some(now);
            self.scans += 1;
            let search_image;
            let target = if let Some(rect) = self.search {
                search_image = image.crop(rect)?;
                &search_image
            } else {
                image
            };
            let found = locate(target, self.profile, |p| self.accepts(p))?;
            found.map(|f| {
                let mut rect = f.rect;
                if let Some(search) = self.search {
                    rect.x += search.x;
                    rect.y += search.y;
                }
                self.lock = Some(rect);
                self.locks += 1;
                (rect, f.packet)
            })
        };
        let Some((rect, packet)) = decoded else {
            self.invalid += 1;
            let since = self.failed_since.get_or_insert(now);
            if !self.manual && now.duration_since(*since) >= Duration::from_millis(750) {
                self.lock = None;
            }
            return Ok(None);
        };
        self.failed_since = None;
        match self.receiver.push_packet(&packet.encode()?) {
            Ok(_) => {}
            Err(vft_receiver::Error::HashMismatch) => {
                return Err("reconstructed object failed SHA-256 verification".into())
            }
            Err(vft_receiver::Error::Limit) => {
                return Err("manifest exceeds receiver resource limits".into())
            }
            Err(_) => {
                self.invalid += 1;
                return Ok(None);
            }
        }
        if let Some(m) = self.receiver.manifest() {
            self.profile = Some(m.profile);
        }
        Ok(Some(rect))
    }
}
