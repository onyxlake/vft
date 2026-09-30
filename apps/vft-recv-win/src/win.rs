use std::{
    ffi::c_void,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    ptr::NonNull,
    time::{Duration, Instant},
};
use vft_protocol::{Packet, PacketType, ProfileId};
use vft_receiver::{Error as ReceiverError, Limits, ReceiverSession};
use vft_visual::{Error as VisualError, GridConfig, LumaImage, VisualFrame};
use windows::{
    core::w,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};
type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Region {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}
impl Region {
    fn validate(self) -> AppResult<Self> {
        if self.width < 192
            || self.height < 108
            || self.width > 8192
            || self.height > 8192
            || self.x.checked_add(self.width).is_none()
            || self.y.checked_add(self.height).is_none()
        {
            return Err("invalid capture region; dimensions must be 192x108..8192x8192".into());
        }
        Ok(self)
    }
}
struct Options {
    rect: Option<Region>,
    sender_window: bool,
    output: PathBuf,
    profile: Option<ProfileId>,
    fps: u32,
    timeout: u64,
    stats: Option<PathBuf>,
    preview: Option<PathBuf>,
    max_file_size: u64,
}
fn options() -> AppResult<Option<Options>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|s| s == "--help" || s == "-h") {
        println!("vft-recv-win --rect X Y WIDTH HEIGHT --output FILE [--profile auto|robust|balanced]\n  [--fps 30] [--timeout 3600] [--stats FILE.csv] [--preview FILE.bmp] [--max-file-size BYTES]\nUse --sender-window instead of --rect for a local VFT sender window. The capture rectangle must contain only the entire visual frame.");
        return Ok(None);
    }
    let mut out = Options {
        rect: None,
        sender_window: false,
        output: PathBuf::new(),
        profile: None,
        fps: 30,
        timeout: 3600,
        stats: None,
        preview: None,
        max_file_size: 256 * 1024 * 1024,
    };
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        let mut value = || -> AppResult<&str> {
            let v = args.get(i).ok_or("missing option value")?;
            i += 1;
            Ok(v)
        };
        match arg.as_str() {
            "--rect" => {
                out.rect = Some(
                    Region {
                        x: value()?.parse()?,
                        y: value()?.parse()?,
                        width: value()?.parse()?,
                        height: value()?.parse()?,
                    }
                    .validate()?,
                )
            }
            "--sender-window" => out.sender_window = true,
            "--output" => out.output = PathBuf::from(value()?),
            "--profile" => {
                out.profile = match value()? {
                    "auto" => None,
                    "robust" => Some(ProfileId::Robust),
                    "balanced" => Some(ProfileId::Balanced),
                    _ => return Err("unknown profile".into()),
                }
            }
            "--fps" => out.fps = value()?.parse()?,
            "--timeout" => out.timeout = value()?.parse()?,
            "--stats" => out.stats = Some(PathBuf::from(value()?)),
            "--preview" => out.preview = Some(PathBuf::from(value()?)),
            "--max-file-size" => out.max_file_size = value()?.parse()?,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if out.output.as_os_str().is_empty()
        || out.rect.is_some() == out.sender_window
        || !(1..=120).contains(&out.fps)
        || out.timeout == 0
    {
        return Err(
            "supply --output and exactly one capture source, FPS 1..120 and positive timeout"
                .into(),
        );
    }
    if out.output.exists() {
        return Err("output already exists; refusing to overwrite".into());
    }
    if out.stats.as_ref().is_some_and(|p| p.exists())
        || out.preview.as_ref().is_some_and(|p| p.exists())
    {
        return Err("stats or preview already exists; choose a new path".into());
    }
    if out.stats.as_ref() == Some(&out.output)
        || out.preview.as_ref() == Some(&out.output)
        || (out.stats.is_some() && out.stats == out.preview)
    {
        return Err("output, stats and preview paths must differ".into());
    }
    Ok(Some(out))
}

fn local_region() -> AppResult<Region> {
    // DPI awareness is set before calling ClientToScreen so coordinates are physical pixels.
    unsafe {
        let hwnd = FindWindowW(w!("VFTSenderWindow"), None)?;
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)?;
        let mut point = POINT::default();
        if !ClientToScreen(hwnd, &mut point).as_bool() {
            return Err("ClientToScreen failed".into());
        }
        Region {
            x: point.x,
            y: point.y,
            width: rect.right,
            height: rect.bottom,
        }
        .validate()
    }
}
struct Capture {
    screen: HDC,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    pixels: NonNull<u8>,
    region: Region,
}
impl Capture {
    fn new(region: Region) -> AppResult<Self> {
        region.validate()?;
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err("desktop DC is unavailable".into());
            }
            let dc = CreateCompatibleDC(Some(screen));
            if dc.is_invalid() {
                ReleaseDC(None, screen);
                return Err("CreateCompatibleDC failed".into());
            }
            let info = bitmap_info(region.width, region.height);
            let mut ptr: *mut c_void = std::ptr::null_mut();
            let bitmap =
                match CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut ptr, None, 0) {
                    Ok(bitmap) => bitmap,
                    Err(error) => {
                        let _ = DeleteDC(dc);
                        ReleaseDC(None, screen);
                        return Err(error.into());
                    }
                };
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            let Some(pixels) = NonNull::new(ptr.cast::<u8>()) else {
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                ReleaseDC(None, screen);
                return Err("DIB pixel buffer is null".into());
            };
            if old.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                ReleaseDC(None, screen);
                return Err("SelectObject failed".into());
            }
            Ok(Self {
                screen,
                dc,
                bitmap,
                old,
                pixels,
                region,
            })
        }
    }
    fn grab(&self) -> AppResult<LumaImage> {
        let r = self.region;
        unsafe {
            BitBlt(
                self.dc,
                0,
                0,
                r.width,
                r.height,
                Some(self.screen),
                r.x,
                r.y,
                SRCCOPY | CAPTUREBLT,
            )?;
            if !GdiFlush().as_bool() {
                return Err("GdiFlush failed".into());
            }
            // The selected top-down 32-bit DIB owns exactly width*height*4 initialized bytes.
            let bytes = std::slice::from_raw_parts(
                self.pixels.as_ptr(),
                r.width as usize * r.height as usize * 4,
            );
            let pixels = bytes
                .chunks_exact(4)
                .map(|p| {
                    ((u32::from(p[0]) * 29 + u32::from(p[1]) * 150 + u32::from(p[2]) * 77) >> 8)
                        as u8
                })
                .collect();
            Ok(LumaImage {
                width: r.width as usize,
                height: r.height as usize,
                pixels,
            })
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        // Restore the previous bitmap before deleting either GDI object.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
            ReleaseDC(None, self.screen);
        }
    }
}
fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn decode(image: &LumaImage, profile: Option<ProfileId>) -> Result<Packet, VisualError> {
    let mut error = VisualError::Sync;
    for candidate in [ProfileId::Balanced, ProfileId::Robust] {
        if profile.is_some_and(|p| p != candidate) {
            continue;
        }
        match VisualFrame::sample(image, GridConfig::for_profile(candidate))
            .and_then(|frame| frame.decode())
        {
            Ok(packet) if packet.profile == candidate => return Ok(packet),
            Ok(_) => error = VisualError::Metadata,
            Err(e) => error = e,
        }
    }
    Err(error)
}
fn preview(image: &LumaImage, path: &PathBuf) -> AppResult<()> {
    let data: Vec<u8> = image.pixels.iter().flat_map(|&v| [v, v, v, 0]).collect();
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(b"BM")?;
    file.write_all(&(54u32 + data.len() as u32).to_le_bytes())?;
    file.write_all(&[0; 4])?;
    file.write_all(&54u32.to_le_bytes())?;
    file.write_all(&40u32.to_le_bytes())?;
    file.write_all(&(image.width as i32).to_le_bytes())?;
    file.write_all(&(-(image.height as i32)).to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&32u16.to_le_bytes())?;
    file.write_all(&[0; 24])?;
    file.write_all(&data)?;
    Ok(())
}

#[derive(Default)]
struct Metrics {
    captures: u64,
    invalid_frames: u64,
    crc_failures: u64,
    valid_frames: u64,
    unique_frames: u64,
    source_frames: u64,
    repair_frames: u64,
}
pub fn run() -> AppResult<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let mut receiver = ReceiverSession::new(Limits {
        max_file_size: options.max_file_size,
        ..Limits::default()
    });
    let start = Instant::now();
    let mut last_report = start;
    let mut capture: Option<Capture> = None;
    let mut selected = options.profile;
    let mut metrics = Metrics::default();
    let mut last_packet = None;
    let mut preview_saved = false;
    let interval = Duration::from_secs_f64(1.0 / f64::from(options.fps));
    loop {
        let iteration = Instant::now();
        if start.elapsed().as_secs() >= options.timeout {
            save_stats(&options, &receiver, &metrics, start.elapsed())?;
            return Err(format!(
                "capture timed out; progress={:?}, invalid_frames={}",
                receiver.progress(),
                metrics.invalid_frames
            )
            .into());
        }
        let region = if options.sender_window {
            local_region()?
        } else {
            options.rect.ok_or("missing region")?
        };
        if capture.as_ref().is_none_or(|c| c.region != region) {
            capture = Some(Capture::new(region)?);
        }
        let image = capture.as_ref().ok_or("missing capture")?.grab()?;
        metrics.captures += 1;
        if !preview_saved {
            if let Some(path) = &options.preview {
                preview(&image, path)?;
            }
            preview_saved = true;
        }
        match decode(&image, selected) {
            Ok(packet) => {
                metrics.valid_frames += 1;
                match packet.kind {
                    PacketType::SourceSymbol => metrics.source_frames += 1,
                    PacketType::RepairSymbol => metrics.repair_frames += 1,
                    _ => {}
                }
                let wire = packet.encode()?;
                if last_packet.as_ref() != Some(&wire) {
                    metrics.unique_frames += 1;
                    last_packet = Some(wire.clone());
                }
                match receiver.push_packet(&wire) {
                    Ok(_) => {}
                    Err(ReceiverError::HashMismatch) => {
                        save_stats(&options, &receiver, &metrics, start.elapsed())?;
                        return Err("reconstructed object failed SHA-256 verification".into());
                    }
                    Err(error) => eprintln!("Rejected packet: {error}"),
                }
                if let Some(manifest) = receiver.manifest() {
                    if selected.is_none() {
                        println!(
                            "filename={} size={} session={:02x?} profile={:?}",
                            manifest.filename,
                            manifest.file_size,
                            manifest.session,
                            manifest.profile
                        );
                    }
                    selected = Some(manifest.profile);
                }
            }
            Err(error) => {
                metrics.invalid_frames += 1;
                if matches!(error, VisualError::Protocol(vft_protocol::Error::Crc)) {
                    metrics.crc_failures += 1;
                }
            }
        }
        if receiver.is_complete() {
            save_verified(&receiver, &options.output)?;
            save_stats(&options, &receiver, &metrics, start.elapsed())?;
            let elapsed = start.elapsed().as_secs_f64();
            println!("COMPLETE sha256=verified bytes={} elapsed={elapsed:.3}s goodput={:.1} KiB/s progress={:?} invalid_frames={}",
                receiver.progress().recovered_bytes,receiver.progress().recovered_bytes as f64/elapsed/1024.0,receiver.progress(),metrics.invalid_frames);
            return Ok(());
        }
        if last_report.elapsed() >= Duration::from_secs(1) {
            println!(
                "progress={:?} captures={} valid_frames={} invalid_frames={}",
                receiver.progress(),
                metrics.captures,
                metrics.valid_frames,
                metrics.invalid_frames
            );
            last_report = Instant::now();
        }
        std::thread::sleep(interval.saturating_sub(iteration.elapsed()));
    }
}
fn save_verified(receiver: &ReceiverSession, path: &PathBuf) -> AppResult<()> {
    let chunks = receiver.verified_chunks()?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    for chunk in chunks {
        file.write_all(chunk)?;
    }
    file.sync_all()?;
    Ok(())
}
fn save_stats(
    options: &Options,
    receiver: &ReceiverSession,
    metrics: &Metrics,
    elapsed: Duration,
) -> AppResult<()> {
    let Some(path) = &options.stats else {
        return Ok(());
    };
    let p = receiver.progress();
    let seconds = elapsed.as_secs_f64();
    let profile = receiver.manifest().map(|m| m.profile as u8).unwrap_or(255);
    let size = receiver.manifest().map(|m| m.file_size).unwrap_or(0);
    let sources = receiver.manifest().map(|m| m.source_symbols).unwrap_or(0);
    let verified_bytes = if receiver.is_complete() { size } else { 0 };
    let overhead = if sources > 0 {
        p.valid_symbols as f64 / f64::from(sources) - 1.0
    } else {
        0.0
    };
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(file,"profile,file_bytes,elapsed_seconds,captures,valid_frames,unique_frames,invalid_frames,crc_failures,valid_symbols,duplicate_symbols,dependent_symbols,recovered_bytes,hash_verified,verified_goodput_bytes_per_second,unique_frame_rate,packet_error_rate,fec_overhead,source_frames,repair_frames")?;
    writeln!(file,"{profile},{size},{seconds:.6},{},{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:.6},{overhead:.6},{},{}",
        metrics.captures,metrics.valid_frames,metrics.unique_frames,metrics.invalid_frames,metrics.crc_failures,
        p.valid_symbols,p.duplicate_symbols,p.dependent_symbols,p.recovered_bytes,receiver.is_complete(),
        verified_bytes as f64/seconds,metrics.unique_frames as f64/seconds,metrics.invalid_frames as f64/metrics.captures.max(1) as f64,
        metrics.source_frames,metrics.repair_frames)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn statistics_cannot_overwrite_a_recovered_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("vft-stats-{}-{nonce}.bin", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(b"verified file bytes").unwrap();
        drop(file);
        let options = Options {
            rect: None,
            sender_window: true,
            output: path.clone(),
            profile: None,
            fps: 30,
            timeout: 1,
            stats: Some(path.clone()),
            preview: None,
            max_file_size: 1,
        };
        let result = save_stats(
            &options,
            &ReceiverSession::default(),
            &Metrics::default(),
            Duration::from_secs(1),
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"verified file bytes");
        std::fs::remove_file(path).unwrap();
    }
}
