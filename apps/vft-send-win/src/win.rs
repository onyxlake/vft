use std::{
    cell::RefCell,
    ffi::c_void,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use vft_protocol::{Digest, Manifest, ProfileId, Sha256};
use vft_sender::{ObjectSource, SenderConfig, SenderSession};
use vft_visual::{GridConfig, LumaImage};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;
struct FileSource(File);
impl ObjectSource for FileSource {
    fn read_exact_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
        self.0
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.0.read_exact(buffer))
            .map_err(|e| e.to_string())
    }
}
struct Options {
    file: Option<PathBuf>,
    profile: ProfileId,
    fps: u32,
    cell_size: usize,
    fullscreen: bool,
    frames: Option<u64>,
    rect_file: Option<PathBuf>,
    position: (i32, i32),
}
fn options() -> AppResult<Option<Options>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("vft-send-win FILE [--profile robust|balanced] [--fps 15] [--cell-size 8] [--fullscreen]\n  [--frames N] [--rect-file PATH] [--position X Y]\nvft-send-win --test-pattern\nESC exits; SPACE pauses; F11 toggles fullscreen.");
        return Ok(None);
    }
    let mut out = Options {
        file: None,
        profile: ProfileId::Balanced,
        fps: 15,
        cell_size: 0,
        fullscreen: false,
        frames: None,
        rect_file: None,
        position: (40, 40),
    };
    let mut pattern = false;
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
            "--profile" => {
                out.profile = match value()? {
                    "robust" => ProfileId::Robust,
                    "balanced" => ProfileId::Balanced,
                    _ => return Err("unknown profile".into()),
                };
            }
            "--fps" => out.fps = value()?.parse()?,
            "--cell-size" => out.cell_size = value()?.parse()?,
            "--fullscreen" => out.fullscreen = true,
            "--frames" => out.frames = Some(value()?.parse()?),
            "--rect-file" => out.rect_file = Some(PathBuf::from(value()?)),
            "--position" => out.position = (value()?.parse()?, value()?.parse()?),
            "--test-pattern" => pattern = true,
            _ if !arg.starts_with('-') && out.file.is_none() => out.file = Some(PathBuf::from(arg)),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if out.cell_size == 0 {
        out.cell_size = if out.profile == ProfileId::Robust {
            16
        } else {
            8
        };
    }
    if !(1..=60).contains(&out.fps) || !(2..=32).contains(&out.cell_size) || out.frames == Some(0) {
        return Err("invalid FPS, cell size or frame count".into());
    }
    if out.file.is_none() && !pattern {
        return Err("supply a file or --test-pattern".into());
    }
    if out.file.is_some() && pattern {
        return Err("test pattern and file are mutually exclusive".into());
    }
    Ok(Some(out))
}

struct App {
    sender: Option<SenderSession<FileSource>>,
    pixels: Vec<u8>,
    width: i32,
    height: i32,
    options: Options,
    paused: bool,
    fullscreen: bool,
    placement: RECT,
    frame_count: u64,
    error: Option<String>,
    filename: String,
}
impl App {
    fn new(options: Options) -> AppResult<Self> {
        let grid = GridConfig::for_profile(options.profile);
        let mut filename = String::from("test pattern");
        let sender = if let Some(path) = &options.file {
            let mut file = File::open(path)?;
            let size = file.metadata()?.len();
            filename = path
                .file_name()
                .ok_or("missing filename")?
                .to_string_lossy()
                .into_owned();
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            let sha256: [u8; 32] = hasher.finalize().into();
            let mut hash = Sha256::new();
            hash.update(sha256);
            hash.update(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_nanos()
                    .to_le_bytes(),
            );
            hash.update(std::process::id().to_le_bytes());
            let session = hash.finalize()[..16].try_into().unwrap();
            let symbol_size = grid.payload_capacity(options.profile)? as u32;
            let count = u32::try_from(size.div_ceil(u64::from(symbol_size)))?;
            let manifest = Manifest {
                session,
                object: sha256[..16].try_into().unwrap(),
                filename: filename.clone(),
                file_size: size,
                sha256,
                symbol_size,
                source_symbols: count,
                profile: options.profile,
            };
            println!(
                "filename={} size={} session={:02x?} profile={:?} fps={}",
                manifest.filename, size, session, options.profile, options.fps
            );
            Some(SenderSession::new(
                manifest,
                FileSource(file),
                SenderConfig {
                    grid,
                    manifest_interval: 30,
                },
            )?)
        } else {
            None
        };
        let width = (grid.columns * options.cell_size) as i32;
        let height = (grid.rows * options.cell_size) as i32;
        let mut app = Self {
            sender,
            pixels: Vec::new(),
            width,
            height,
            options,
            paused: false,
            fullscreen: false,
            placement: RECT::default(),
            frame_count: 0,
            error: None,
            filename,
        };
        app.advance()?;
        Ok(app)
    }
    fn advance(&mut self) -> AppResult<()> {
        let image = if let Some(sender) = &mut self.sender {
            sender.next_frame()?.render(self.options.cell_size)?
        } else {
            LumaImage {
                width: self.width as usize,
                height: self.height as usize,
                pixels: (0..self.width as usize * self.height as usize)
                    .map(|i| ((i % self.width as usize) * 4 / self.width as usize * 85) as u8)
                    .collect(),
            }
        };
        self.pixels = image.pixels.iter().flat_map(|&v| [v, v, v, 0]).collect();
        self.frame_count += 1;
        Ok(())
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
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

pub fn run() -> AppResult<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    let app = Box::new(RefCell::new(App::new(options)?));
    let (width, height, position, fullscreen, fps) = {
        let app = app.borrow();
        (
            app.width,
            app.height,
            app.options.position,
            app.options.fullscreen,
            app.options.fps,
        )
    };
    // All handles and the borrowed App pointer stay on this thread; Box outlives the message loop.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            hInstance: instance,
            lpszClassName: w!("VFTSenderWindow"),
            lpfnWndProc: Some(window_proc),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread().into());
        }
        let style = WS_OVERLAPPEDWINDOW;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        AdjustWindowRectEx(&mut rect, style, false, WINDOW_EX_STYLE(0))?;
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("VFTSenderWindow"),
            w!("VFT Sender"),
            style,
            position.0,
            position.1,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(instance),
            Some((&*app as *const RefCell<App>).cast::<c_void>()),
        )?;
        if fullscreen {
            if let Err(error) = toggle_fullscreen(hwnd, &mut app.borrow_mut()) {
                let _ = DestroyWindow(hwnd);
                return Err(error);
            }
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        if SetTimer(Some(hwnd), 1, (1000 / fps).max(1), None) == 0 {
            let _ = DestroyWindow(hwnd);
            return Err("SetTimer failed".into());
        }
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == 0 {
                if message.wParam.0 != 0 {
                    let _ = DestroyWindow(hwnd);
                    return Err("Win32 callback failed".into());
                }
                break;
            }
            if result == -1 {
                let _ = DestroyWindow(hwnd);
                return Err(windows::core::Error::from_thread().into());
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    if let Some(error) = app.borrow_mut().error.take() {
        return Err(error.into());
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Catch Rust failures before they could unwind through the Win32 callback ABI.
    let result = std::panic::catch_unwind(|| unsafe { handle_message(hwnd, msg, wparam, lparam) });
    match result {
        Ok(value) => value,
        Err(_) => {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(hwnd);
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}
unsafe fn handle_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        if msg == WM_DESTROY {
            let _ = KillTimer(Some(hwnd), 1);
            PostQuitMessage(0);
            return LRESULT(0);
        }
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        // DefWindowProc/SetWindowText/SetWindowPos can synchronously re-enter this procedure.
        // Borrow App only for the four messages that actually access it.
        if !matches!(msg, WM_PAINT | WM_ERASEBKGND | WM_TIMER | WM_KEYDOWN) {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<App>;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let Ok(mut borrowed) = (*ptr).try_borrow_mut() else {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        };
        let app = &mut *borrowed;
        match msg {
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                StretchDIBits(
                    dc,
                    0,
                    0,
                    rect.right,
                    rect.bottom,
                    0,
                    0,
                    app.width,
                    app.height,
                    Some(app.pixels.as_ptr().cast()),
                    &bitmap_info(app.width, app.height),
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                let _ = EndPaint(hwnd, &paint);
                if let Some(path) = &app.options.rect_file {
                    let mut origin = POINT::default();
                    let _ = ClientToScreen(hwnd, &mut origin);
                    if let Err(error) = std::fs::write(
                        path,
                        format!("{} {} {} {}", origin.x, origin.y, rect.right, rect.bottom),
                    ) {
                        app.error = Some(error.to_string());
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER => {
                if !app.paused {
                    if app
                        .options
                        .frames
                        .is_some_and(|limit| app.frame_count >= limit)
                    {
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    if let Err(error) = app.advance() {
                        app.error = Some(error.to_string());
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    let progress = app.sender.as_ref().map(|s| s.progress());
                    let title = wide(&format!(
                        "VFT — {} | {:?} {} FPS | frame {} | {:?}",
                        app.filename,
                        app.options.profile,
                        app.options.fps,
                        app.frame_count,
                        progress
                    ));
                    let _ = SetWindowTextW(hwnd, PCWSTR(title.as_ptr()));
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                match wparam.0 {
                    0x1b => {
                        let _ = DestroyWindow(hwnd);
                    }
                    0x20 => app.paused = !app.paused,
                    0x7a => {
                        if let Err(error) = toggle_fullscreen(hwnd, app) {
                            app.error = Some(error.to_string());
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
unsafe fn toggle_fullscreen(hwnd: HWND, app: &mut App) -> AppResult<()> {
    unsafe {
        if !app.fullscreen {
            GetWindowRect(hwnd, &mut app.placement)?;
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return Err("GetMonitorInfoW failed".into());
            }
            SetWindowLongPtrW(hwnd, GWL_STYLE, WS_POPUP.0 as isize);
            SetWindowPos(
                hwnd,
                None,
                info.rcMonitor.left,
                info.rcMonitor.top,
                info.rcMonitor.right - info.rcMonitor.left,
                info.rcMonitor.bottom - info.rcMonitor.top,
                SWP_FRAMECHANGED | SWP_NOZORDER,
            )?;
        } else {
            SetWindowLongPtrW(hwnd, GWL_STYLE, WS_OVERLAPPEDWINDOW.0 as isize);
            let r = app.placement;
            SetWindowPos(
                hwnd,
                None,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_FRAMECHANGED | SWP_NOZORDER,
            )?;
        }
        app.fullscreen = !app.fullscreen;
        Ok(())
    }
}
