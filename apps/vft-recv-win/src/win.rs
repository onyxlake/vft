use crate::output::OutputDestination;
use std::io::IsTerminal;
use std::sync::{
    atomic::AtomicBool,
    mpsc::{self, Receiver},
    Arc, Mutex,
};
use std::{
    ffi::c_void,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    ptr::NonNull,
    time::{Duration, Instant},
};
use vft_protocol::{Packet, PacketType, ProfileId};
use vft_receiver::{Error as ReceiverError, Limits, ReceiverSession};
use vft_tui::{
    terminal::{random_id, receiver_ui, ReceiverCommand},
    Lifecycle, SharedView, Stop, View,
};
use vft_visual::{locate, Error as VisualError, GridConfig, LumaImage, VisualFrame};
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
    tui: Option<bool>,
    rect: Option<Region>,
    sender_window: bool,
    auto: bool,
    search_rect: Option<Region>,
    output: OutputDestination,
    profile: Option<ProfileId>,
    fps: u32,
    timeout: u64,
    stats: Option<PathBuf>,
    preview: Option<PathBuf>,
    max_file_size: u64,
}
fn options() -> AppResult<Option<Options>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    parse_options(&args)
}
fn parse_options(args: &[String]) -> AppResult<Option<Options>> {
    if args.is_empty() || args.iter().any(|s| s == "--help" || s == "-h") {
        println!("vft-recv-win (--auto | --rect X Y WIDTH HEIGHT | --sender-window)\n  [--output FILE] [--output-dir DIR]\n  [--profile auto|robust|balanced] [--fps 30] [--timeout 3600]\n  [--stats FILE.csv] [--preview FILE.bmp] [--max-file-size BYTES]\n  [--search-rect X Y WIDTH HEIGHT] (only with --auto)\n  [--tui | --no-tui] (interactive terminals default to TUI)\nWithout --output, save the metadata filename in --output-dir (default: current directory).\nThe directory must exist. --output uses its exact path and overrides --output-dir.\nOutput, preview and statistics must be new, distinct files.\n--auto locates a visible VFT frame on the desktop and reacquires after movement.\n--rect must contain only the entire visual frame. --sender-window follows a local sender.");
        return Ok(None);
    }
    let mut out = Options {
        tui: None,
        rect: None,
        sender_window: false,
        auto: false,
        search_rect: None,
        output: OutputDestination::default(),
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
            "--tui" => vft_tui::mode_flag(&mut out.tui, true)?,
            "--no-tui" => vft_tui::mode_flag(&mut out.tui, false)?,
            "--rect" | "--search-rect" => {
                let region = Region {
                    x: value()?.parse()?,
                    y: value()?.parse()?,
                    width: value()?.parse()?,
                    height: value()?.parse()?,
                }
                .validate()?;
                if arg == "--rect" {
                    out.rect = Some(region);
                } else {
                    out.search_rect = Some(region);
                }
            }
            "--sender-window" => out.sender_window = true,
            "--auto" => out.auto = true,
            "--output" => out.output.file = Some(PathBuf::from(value()?)),
            "--output-dir" => out.output.directory = PathBuf::from(value()?),
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
    if usize::from(out.rect.is_some()) + usize::from(out.sender_window) + usize::from(out.auto) != 1
        || (out.search_rect.is_some() && !out.auto)
        || !(1..=120).contains(&out.fps)
        || out.timeout == 0
    {
        return Err(
            "supply exactly one of --auto/--rect/--sender-window, FPS 1..120 and positive timeout; --search-rect requires --auto"
                .into(),
        );
    }
    out.output
        .validate(out.preview.as_deref(), out.stats.as_deref())?;
    Ok(Some(out))
}

fn desktop_region() -> AppResult<Region> {
    // Virtual desktop origin can be negative when a monitor is left/above the primary.
    unsafe {
        Region {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
        .validate()
    }
}

fn local_region() -> AppResult<Region> {
    // DPI awareness is set before calling ClientToScreen so coordinates are physical pixels.
    unsafe {
        let hwnd = FindWindowW(w!("VFTSenderWindow"), None).map_err(|e|format!(
            "No local VFT sender window was found; start the sender or use --auto/--rect. Windows diagnostic: {e}"
        ))?;
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
    location_scans: u64,
    location_locks: u64,
}
pub fn run() -> AppResult<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    let tui = vft_tui::choose_mode(
        options.tui,
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )?;
    if !tui {
        return run_capture(options, None);
    }
    let shared = Arc::new(Mutex::new(View {
        title: String::from("VFT Receive"),
        ..Default::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    let _panic_log = vft_tui::PanicLog::install(shared.clone());
    let (tx, rx) = mpsc::sync_channel(32);
    let worker_view = shared.clone();
    let worker_stop = cancel.clone();
    // Capture/ReceiverSession and all GDI handles are created and destroyed inside this worker.
    let worker = std::thread::spawn(move || {
        let context = ReceiveUi {
            shared: worker_view.clone(),
            cancel: worker_stop,
            commands: rx,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_capture(options, Some(&context))
                .map_err(|e| format!("Capture/recovery failed: {e}"))
        }))
        .unwrap_or_else(|_| Err(String::from("Capture/recovery worker panicked")));
        let mut view = worker_view.lock().unwrap();
        view.worker_done = true;
        if let Err(e) = &result {
            view.state = Lifecycle::Failed;
            view.result = e.clone();
            view.log(e);
        }
        result
    });
    let ui_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        receiver_ui(shared.clone(), cancel.clone(), tx)
    }))
    .unwrap_or_else(|_| Err(std::io::Error::other("Receiver TUI panicked")));
    vft_tui::stop(&cancel);
    let result = worker
        .join()
        .map_err(|_| "Capture worker could not be joined")?;
    ui_result?;
    if result.is_ok() {
        println!("{}", vft_tui::safe_text(&shared.lock().unwrap().result));
    }
    result.map_err(Into::into)
}
struct ReceiveUi {
    shared: SharedView,
    cancel: Stop,
    commands: Receiver<ReceiverCommand>,
}
fn report(ui: Option<&ReceiveUi>, message: String) {
    if let Some(ui) = ui {
        ui.shared.lock().unwrap().log(&message);
    } else {
        println!("{message}");
    }
}
fn warn(ui: Option<&ReceiveUi>, message: String) {
    if let Some(ui) = ui {
        ui.shared.lock().unwrap().log(&message);
    } else {
        eprintln!("{message}");
    }
}
fn run_capture(mut options: Options, ui: Option<&ReceiveUi>) -> AppResult<()> {
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
    let mut search_capture: Option<Capture> = None;
    let mut failures_since: Option<Instant> = None;
    let mut selected = options.profile;
    let mut metrics = Metrics::default();
    let mut last_packet = None;
    let mut preview_saved = false;
    let interval = Duration::from_secs_f64(1.0 / f64::from(options.fps));
    let mut page = 0;
    let mut snapshot_time = start;
    let mut previous = (0u64, 0u64, 0u64);
    loop {
        let iteration = Instant::now();
        if let Some(ui) = ui {
            if vft_tui::stopped(&ui.cancel) {
                ui.shared.lock().unwrap().state = Lifecycle::Stopping;
                save_stats(&options, &receiver, &metrics, start.elapsed())?;
                return Err("Capture stopped before verified completion".into());
            }
            while let Ok(command) = ui.commands.try_recv() {
                match command {
                    ReceiverCommand::Page(p) => page = p,
                    ReceiverCommand::Generate(selected) => {
                        let generated = format!(
                            "Unix {:.3} s",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)?
                                .as_secs_f64()
                        );
                        match vft_tui::generate_snapshot(&receiver, &selected, random_id, generated)
                        {
                            Ok(request) => ui.shared.lock().unwrap().request = Some(request),
                            Err(e) => ui.shared.lock().unwrap().log(&e),
                        }
                    }
                }
            }
            if snapshot_time.elapsed() >= Duration::from_millis(200) {
                let dt = snapshot_time.elapsed().as_secs_f64();
                let progress = receiver.progress();
                let mut v = ui.shared.lock().unwrap();
                v.state = Lifecycle::Running;
                v.manifest = receiver.manifest().cloned();
                vft_tui::update_groups(&mut v, &receiver, page);
                let total = receiver.manifest().map_or(0, |m| m.generations());
                let sources = receiver.manifest().map_or(0, |m| m.source_symbols);
                v.title = receiver.manifest().map_or(
                    String::from("VFT Receive - waiting for manifest"),
                    |m| {
                        format!(
                            "VFT Receive - session: {}",
                            m.session
                                .iter()
                                .map(|b| format!("{b:02X}"))
                                .collect::<String>()
                        )
                    },
                );
                v.details = vec![
                    format!(
                        "Recovered: {} / {} groups   Rank: {} / {}   SHA: {:?}",
                        progress.completed_generations,
                        total,
                        progress.rank_symbols,
                        sources,
                        progress.hash
                    ),
                    format!(
                        "Captures: {:.1}/s   Adjacent unique frames: {:.1}/s   Rank gain: {:.1}/s",
                        (metrics.captures - previous.0) as f64 / dt,
                        (metrics.unique_frames - previous.1) as f64 / dt,
                        (progress.rank_symbols - previous.2) as f64 / dt
                    ),
                    format!(
                        "Elapsed: {:.1}s  Invalid frames: {}  Location locks: {}",
                        start.elapsed().as_secs_f64(),
                        metrics.invalid_frames,
                        metrics.location_locks
                    ),
                ];
                if let Some(path) = options.output.path() {
                    v.details.push(format!(
                        "Output: {}",
                        vft_tui::safe_text(&path.display().to_string())
                    ));
                }
                previous = (
                    metrics.captures,
                    metrics.unique_frames,
                    progress.rank_symbols,
                );
                snapshot_time = Instant::now();
            }
        }
        if start.elapsed().as_secs() >= options.timeout {
            save_stats(&options, &receiver, &metrics, start.elapsed())?;
            return Err(format!(
                "capture timed out; progress={:?}, invalid_frames={}",
                receiver.progress(),
                metrics.invalid_frames
            )
            .into());
        }
        let (image, decoded) = if options.auto && capture.is_none() {
            let search_region = match options.search_rect {
                Some(region) => region,
                None => desktop_region()?,
            };
            if search_capture
                .as_ref()
                .is_none_or(|c| c.region != search_region)
            {
                search_capture = Some(Capture::new(search_region)?);
            }
            let desktop = search_capture
                .as_ref()
                .ok_or("missing search capture")?
                .grab()?;
            metrics.location_scans += 1;
            let identity = receiver.manifest().map(|m| (m.session, m.object));
            let found = locate(&desktop, selected, |packet| {
                identity.is_none_or(|id| (packet.session, packet.object) == id)
            })?;
            let Some(found) = found else {
                if last_report.elapsed() >= Duration::from_secs(1) {
                    report(
                        ui,
                        format!(
                            "Searching for visible VFT frame; scans={} progress={:?}",
                            metrics.location_scans,
                            receiver.progress()
                        ),
                    );
                    last_report = Instant::now();
                }
                // Full desktop discovery is throttled independently of locked capture FPS.
                std::thread::sleep(Duration::from_millis(500).saturating_sub(iteration.elapsed()));
                continue;
            };
            let region = Region {
                x: search_region.x + found.rect.x as i32,
                y: search_region.y + found.rect.y as i32,
                width: found.rect.width as i32,
                height: found.rect.height as i32,
            }
            .validate()?;
            capture = Some(Capture::new(region)?);
            // Discovery needs a large DIB only while searching; release it once locked.
            search_capture = None;
            failures_since = None;
            metrics.location_locks += 1;
            report(
                ui,
                format!(
                    "LOCATED rect={} {} {} {} profile={:?}",
                    region.x, region.y, region.width, region.height, found.packet.profile
                ),
            );
            (desktop.crop(found.rect)?, Ok(found.packet))
        } else {
            if !options.auto {
                let region = if options.sender_window {
                    local_region()?
                } else {
                    options.rect.ok_or("missing region")?
                };
                if capture.as_ref().is_none_or(|c| c.region != region) {
                    capture = Some(Capture::new(region)?);
                }
            }
            let image = capture.as_ref().ok_or("missing capture")?.grab()?;
            let decoded = decode(&image, selected).and_then(|packet| {
                if options.auto
                    && receiver
                        .manifest()
                        .is_some_and(|m| packet.session != m.session || packet.object != m.object)
                {
                    Err(VisualError::Metadata)
                } else {
                    Ok(packet)
                }
            });
            (image, decoded)
        };
        metrics.captures += 1;
        if !preview_saved {
            if let Some(path) = &options.preview {
                preview(&image, path)?;
            }
            preview_saved = true;
        }
        match decoded {
            Ok(packet) => {
                failures_since = None;
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
                match receiver.push_packet_observed(&wire, || {
                    if let Some(ui) = ui {
                        ui.shared.lock().unwrap().details =
                            vec![String::from("Verifying SHA-256...")];
                    }
                }) {
                    Ok(_) => {}
                    Err(ReceiverError::HashMismatch) => {
                        save_stats(&options, &receiver, &metrics, start.elapsed())?;
                        return Err("reconstructed object failed SHA-256 verification".into());
                    }
                    Err(error) => warn(ui, format!("Rejected packet: {error}")),
                }
                if let Some(manifest) = receiver.manifest() {
                    if options.output.path().is_none() {
                        let path = options.output.resolve(
                            &manifest.filename,
                            options.preview.as_deref(),
                            options.stats.as_deref(),
                        )?;
                        report(
                            ui,
                            format!(
                                "Output: {}",
                                vft_tui::safe_text(&path.display().to_string())
                            ),
                        );
                    }
                    if selected.is_none() {
                        report(
                            ui,
                            format!(
                                "filename={} size={} session={:02x?} profile={:?}",
                                manifest.filename,
                                manifest.file_size,
                                manifest.session,
                                manifest.profile
                            ),
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
                if options.auto
                    && failures_since.get_or_insert_with(Instant::now).elapsed()
                        >= Duration::from_millis(750)
                {
                    capture = None;
                    failures_since = None;
                    report(
                        ui,
                        String::from("Lost VFT frame; restarting desktop search"),
                    );
                }
            }
        }
        if receiver.is_complete() {
            let output = options
                .output
                .path()
                .ok_or("missing resolved output path")?;
            save_verified(&receiver, output)
                .map_err(|e| format!("Verified output creation/write failed: {e}"))?;
            save_stats(&options, &receiver, &metrics, start.elapsed())?;
            let elapsed = start.elapsed().as_secs_f64();
            if let Some(ui) = ui {
                let mut v = ui.shared.lock().unwrap();
                v.state = Lifecycle::Complete;
                vft_tui::update_groups(&mut v, &receiver, 0);
                v.result = format!(
                    "Complete - SHA-256 verified and saved to {}",
                    vft_tui::safe_text(&output.display().to_string())
                );
                v.details = vec![
                    v.result.clone(),
                    format!(
                        "Bytes: {}   Elapsed: {elapsed:.3}s   Verified goodput: {:.1} KiB/s",
                        receiver.progress().recovered_bytes,
                        receiver.progress().recovered_bytes as f64 / elapsed / 1024.0
                    ),
                ];
            }
            report(ui, format!("COMPLETE sha256=verified bytes={} elapsed={elapsed:.3}s goodput={:.1} KiB/s progress={:?} invalid_frames={} output={}",
                receiver.progress().recovered_bytes,receiver.progress().recovered_bytes as f64/elapsed/1024.0,receiver.progress(),metrics.invalid_frames,vft_tui::safe_text(&output.display().to_string())));
            return Ok(());
        }
        if last_report.elapsed() >= Duration::from_secs(1) {
            report(
                ui,
                format!(
                    "progress={:?} captures={} valid_frames={} invalid_frames={}",
                    receiver.progress(),
                    metrics.captures,
                    metrics.valid_frames,
                    metrics.invalid_frames
                ),
            );
            last_report = Instant::now();
        }
        std::thread::sleep(interval.saturating_sub(iteration.elapsed()));
    }
}
fn save_verified(receiver: &ReceiverSession, path: &Path) -> AppResult<()> {
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
    writeln!(file,"profile,file_bytes,elapsed_seconds,captures,valid_frames,unique_frames,invalid_frames,crc_failures,valid_symbols,duplicate_symbols,dependent_symbols,recovered_bytes,hash_verified,verified_goodput_bytes_per_second,unique_frame_rate,packet_error_rate,fec_overhead,source_frames,repair_frames,location_scans,location_locks")?;
    writeln!(file,"{profile},{size},{seconds:.6},{},{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:.6},{overhead:.6},{},{},{},{}",
        metrics.captures,metrics.valid_frames,metrics.unique_frames,metrics.invalid_frames,metrics.crc_failures,
        p.valid_symbols,p.duplicate_symbols,p.dependent_symbols,p.recovered_bytes,receiver.is_complete(),
        verified_bytes as f64/seconds,metrics.unique_frames as f64/seconds,metrics.invalid_frames as f64/metrics.captures.max(1) as f64,
        metrics.source_frames,metrics.repair_frames,metrics.location_scans,metrics.location_locks)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_options_default_to_metadata_and_explicit_paths_override_directories() {
        let parse =
            |args: &[&str]| parse_options(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let mut options = parse(&["--auto"]).unwrap().unwrap();
        assert!(options.output.file.is_none());
        assert_eq!(
            options.output.resolve("metadata.bin", None, None).unwrap(),
            std::env::current_dir()
                .unwrap()
                .canonicalize()
                .unwrap()
                .join("metadata.bin")
        );
        let dir = crate::output::test_directory();
        let dir_text = dir.to_str().unwrap();
        let mut options = parse(&["--auto", "--output-dir", dir_text])
            .unwrap()
            .unwrap();
        assert_eq!(
            options.output.resolve("original.bin", None, None).unwrap(),
            dir.join("original.bin")
        );
        let file = dir.join("renamed.bin");
        let missing = dir.join("missing");
        let options = parse(&[
            "--auto",
            "--output-dir",
            missing.to_str().unwrap(),
            "--output",
            file.to_str().unwrap(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(options.output.file.as_deref(), Some(file.as_path()));
        for args in [
            vec!["--auto", "--output-dir"],
            vec!["--auto", "--output-dir", ""],
            vec!["--auto", "--output", ""],
            vec!["--auto", "--output-dir", missing.to_str().unwrap()],
        ] {
            assert!(parse(&args).is_err());
        }
    }

    #[test]
    fn metadata_output_requires_verified_bytes_and_never_overwrites_a_late_file() {
        use vft_protocol::{Digest, Manifest, Sha256, SymbolId};
        for race in [false, true] {
            let dir = crate::output::test_directory();
            let args = vec![
                "--auto".into(),
                "--output-dir".into(),
                dir.to_str().unwrap().into(),
            ];
            let mut options = parse_options(&args).unwrap().unwrap();
            let manifest = Manifest {
                session: [1; 16],
                object: [2; 16],
                filename: "original.bin".into(),
                file_size: 1,
                sha256: Sha256::digest([9]).into(),
                symbol_size: 1,
                source_symbols: 1,
                profile: ProfileId::Balanced,
            };
            let mut packet = Packet {
                session: manifest.session,
                object: manifest.object,
                profile: manifest.profile,
                kind: PacketType::Manifest,
                symbol: SymbolId {
                    generation: 0,
                    index: 0,
                },
                payload: manifest.encode().unwrap(),
            };
            let mut receiver = ReceiverSession::default();
            receiver.push_packet(&packet.encode().unwrap()).unwrap();
            let path = options
                .output
                .resolve(&receiver.manifest().unwrap().filename, None, None)
                .unwrap();
            assert!(save_verified(&receiver, path).is_err());
            assert!(!path.exists());
            if race {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .unwrap()
                    .write_all(b"keep existing")
                    .unwrap();
            }
            packet.kind = PacketType::SourceSymbol;
            packet.payload = vec![9];
            receiver.push_packet(&packet.encode().unwrap()).unwrap();
            assert!(receiver.is_complete());
            if race {
                assert!(save_verified(&receiver, path).is_err());
                assert_eq!(std::fs::read(path).unwrap(), b"keep existing");
            } else {
                save_verified(&receiver, path).unwrap();
                assert_eq!(std::fs::read(path).unwrap(), [9]);
                assert!(save_verified(&receiver, path).is_err());
            }
        }
    }

    #[test]
    fn auto_capture_options_are_exclusive_and_search_is_optional() {
        let output = std::env::temp_dir().join(format!(
            "vft-options-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let output = output.to_string_lossy();
        let parse =
            |args: &[&str]| parse_options(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let options = parse(&["--auto", "--output", &output]).unwrap().unwrap();
        assert!(options.auto && options.search_rect.is_none());
        let options = parse(&[
            "--auto",
            "--search-rect",
            "-200",
            "0",
            "800",
            "600",
            "--output",
            &output,
        ])
        .unwrap()
        .unwrap();
        assert_eq!(options.search_rect.unwrap().x, -200);
        for args in [
            vec!["--auto", "--sender-window", "--output", &output],
            vec![
                "--auto", "--rect", "0", "0", "800", "600", "--output", &output,
            ],
            vec![
                "--sender-window",
                "--search-rect",
                "0",
                "0",
                "800",
                "600",
                "--output",
                &output,
            ],
        ] {
            assert!(parse(&args).is_err());
        }
    }
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
            tui: Some(false),
            rect: None,
            sender_window: true,
            auto: false,
            search_rect: None,
            output: OutputDestination::explicit(path.clone()),
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
