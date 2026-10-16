use softbuffer::{Context, Surface};
use std::{
    io::IsTerminal,
    num::NonZeroU32,
    path::PathBuf,
    sync::{atomic::AtomicBool, mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use vft_linux::{open_sender, FileSource, Result};
use vft_protocol::ProfileId;
use vft_sender::{ApplyResult, SenderSession};
use vft_tui::{
    terminal::{sender_ui, SenderCommand},
    Lifecycle, SharedView, Stop, View,
};
use vft_visual::{GridConfig, VisualFrame};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    platform::wayland::EventLoopBuilderExtWayland,
    window::{Fullscreen, Window, WindowId},
};

struct Options {
    file: PathBuf,
    profile: ProfileId,
    fps: u32,
    cell: usize,
    fullscreen: bool,
    frames: Option<u64>,
    tui: Option<bool>,
}
fn options(args: &[String]) -> Result<Option<Options>> {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("vft-send-linux FILE [--profile balanced|robust] [--fps 15] [--cell-size 8]\n  [--fullscreen] [--frames N] [--tui | --no-tui]\nNative Wayland only. ESC exits; SPACE pauses; F11 toggles fullscreen.\nCell size is in physical buffer pixels; resized windows center an integer-cell frame.");
        return Ok(None);
    }
    let mut o = Options {
        file: PathBuf::new(),
        profile: ProfileId::Balanced,
        fps: 15,
        cell: 0,
        fullscreen: false,
        frames: None,
        tui: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || iter.next().ok_or("missing option value");
        match arg.as_str() {
            "--profile" => {
                o.profile = match value()?.as_str() {
                    "balanced" => ProfileId::Balanced,
                    "robust" => ProfileId::Robust,
                    _ => return Err("unknown profile".into()),
                }
            }
            "--fps" => o.fps = value()?.parse()?,
            "--cell-size" => {
                o.cell = value()?.parse()?;
                if !(2..=32).contains(&o.cell) {
                    return Err("cell size must be 2..32".into());
                }
            }
            "--frames" => o.frames = Some(value()?.parse()?),
            "--fullscreen" => o.fullscreen = true,
            "--tui" => vft_tui::mode_flag(&mut o.tui, true)?,
            "--no-tui" => vft_tui::mode_flag(&mut o.tui, false)?,
            _ if !arg.starts_with('-') && o.file.as_os_str().is_empty() => o.file = arg.into(),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if o.file.as_os_str().is_empty() || !(1..=60).contains(&o.fps) || o.frames == Some(0) {
        return Err("supply FILE, FPS 1..60 and a positive frame count".into());
    }
    if o.cell == 0 {
        o.cell = if o.profile == ProfileId::Robust {
            16
        } else {
            8
        };
    }
    Ok(Some(o))
}
struct App {
    options: Options,
    sender: SenderSession<FileSource>,
    window: Option<Arc<Window>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    frame: Option<VisualFrame>,
    paused: bool,
    occluded: bool,
    next: Instant,
    presented: u64,
    error: Option<String>,
    shared: SharedView,
    stop: Stop,
    commands: mpsc::Receiver<SenderCommand>,
    tui: bool,
}
impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl ToString) {
        self.error = Some(error.to_string());
        event_loop.exit();
    }
    fn draw(&mut self) -> Result<()> {
        let window = self.window.as_ref().ok_or("window missing")?;
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        if size.width > 8192 || size.height > 8192 {
            return Err("window exceeds 8192-pixel buffer limit".into());
        }
        let grid = GridConfig::for_profile(self.options.profile);
        let cell = self
            .options
            .cell
            .min(size.width as usize / grid.columns)
            .min(size.height as usize / grid.rows);
        let ready = cell >= 2 && !self.occluded;
        let advance = ready && !self.paused && Instant::now() >= self.next;
        if ready && (self.frame.is_none() || advance) {
            let manual_before = self.sender.manual_remaining();
            self.frame = Some(self.sender.next_frame()?);
            if manual_before > 0 && self.sender.manual_remaining() == 0 {
                self.shared
                    .lock()
                    .unwrap()
                    .log("Manual repair batch sent; receiver completion is unconfirmed");
            }
        }
        let surface = self.surface.as_mut().ok_or("surface missing")?;
        surface
            .resize(
                NonZeroU32::new(size.width).unwrap(),
                NonZeroU32::new(size.height).unwrap(),
            )
            .map_err(|e| e.to_string())?;
        let mut buffer = surface.buffer_mut().map_err(|e| e.to_string())?;
        buffer.fill(0);
        if ready {
            if let Some(frame) = &self.frame {
                let x0 = (size.width as usize - grid.columns * cell) / 2;
                let y0 = (size.height as usize - grid.rows * cell) / 2;
                for y in 0..grid.rows * cell {
                    let start = (y0 + y) * size.width as usize + x0;
                    for (x, p) in buffer[start..start + grid.columns * cell]
                        .iter_mut()
                        .enumerate()
                    {
                        let v = u32::from(frame.cells[(y / cell) * grid.columns + x / cell]);
                        *p = v * 0x010101;
                    }
                }
            }
        }
        buffer.present().map_err(|e| e.to_string())?;
        if advance {
            self.presented += 1;
        }
        self.next = Instant::now() + Duration::from_secs_f64(1.0 / f64::from(self.options.fps));
        let progress = self.sender.progress();
        let title = format!(
            "VFT Send — {} — {} frames{}",
            vft_tui::safe_text(
                &self
                    .options
                    .file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ),
            self.presented,
            if !ready {
                " — enlarge window (minimum 2px/cell)"
            } else if self.paused {
                " — PAUSED"
            } else {
                ""
            }
        );
        window.set_title(&title);
        let mut v = self.shared.lock().unwrap();
        v.title = title;
        v.state = if self.paused || !ready {
            Lifecycle::Paused
        } else {
            Lifecycle::Running
        };
        v.details = vec![
            format!(
                "Requested FPS: {}   Physical cell: {}   Presented: {}",
                self.options.fps, cell, self.presented
            ),
            format!(
                "Source: {}   Repair: {}   Manual remaining: {}",
                progress.source_symbols,
                progress.repair_symbols,
                self.sender.manual_remaining()
            ),
        ];
        Ok(())
    }
}
impl ApplicationHandler<()> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let grid = GridConfig::for_profile(self.options.profile);
        let attributes = Window::default_attributes()
            .with_title("VFT Send")
            .with_inner_size(PhysicalSize::new(
                (grid.columns * self.options.cell) as u32,
                (grid.rows * self.options.cell) as u32,
            ))
            .with_min_inner_size(PhysicalSize::new(
                (grid.columns * 2) as u32,
                (grid.rows * 2) as u32,
            ))
            .with_fullscreen(
                self.options
                    .fullscreen
                    .then_some(Fullscreen::Borderless(None)),
            );
        let window = match el.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.fail(el, e);
                return;
            }
        };
        let surface =
            Context::new(window.clone()).and_then(|context| Surface::new(&context, window.clone()));
        match surface {
            Ok(s) => {
                self.surface = Some(s);
                self.window = Some(window);
            }
            Err(e) => self.fail(el, e),
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Occluded(value) => self.occluded = value,
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Named(NamedKey::Space) => self.paused = !self.paused,
                    Key::Named(NamedKey::F11) => {
                        if let Some(window) = &self.window {
                            window.set_fullscreen(if window.fullscreen().is_some() {
                                None
                            } else {
                                Some(Fullscreen::Borderless(None))
                            });
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.draw() {
                    self.fail(el, error);
                }
                if self.options.frames.is_some_and(|n| self.presented >= n) {
                    el.exit();
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if vft_tui::stopped(&self.stop) {
            el.exit();
            return;
        }
        while let Ok(command) = self.commands.try_recv() {
            match command {
                SenderCommand::TogglePause => self.paused = !self.paused,
                SenderCommand::Cancel => self.sender.cancel_manual(),
                SenderCommand::Apply { serial, request } => {
                    let result = self.sender.apply_request(request);
                    let mut v = self.shared.lock().unwrap();
                    v.applied = serial;
                    v.result = match result {
                        Ok(ApplyResult::Applied) => {
                            String::from("Request applied; manual repair batch queued")
                        }
                        Ok(ApplyResult::Duplicate) => {
                            String::from("Duplicate request; no new repairs queued")
                        }
                        Err(e) => e.to_string(),
                    };
                }
            }
        }
        if Instant::now() >= self.next {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        // Wake for terminal stop even while no surface is visible.
        el.set_control_flow(ControlFlow::WaitUntil(
            self.next
                .max(Instant::now() + Duration::from_millis(2))
                .min(Instant::now() + Duration::from_millis(50)),
        ));
    }
}
struct TerminalWorker {
    stop: Stop,
    shared: SharedView,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for TerminalWorker {
    fn drop(&mut self) {
        vft_tui::stop(&self.stop);
        if let Ok(mut v) = self.shared.lock() {
            v.worker_done = true;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub fn run() -> Result<()> {
    let Some(mut options) = options(&std::env::args().skip(1).collect::<Vec<_>>())? else {
        return Ok(());
    };
    let tui = vft_tui::choose_mode(
        options.tui,
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )?;
    options.tui = Some(tui);
    // File validation and hashing precede any Wayland/terminal setup.
    let sender = open_sender(&options.file, options.profile)?;
    if !tui {
        println!(
            "filename={} size={} sha256={:02x?} requested_fps={}",
            vft_tui::safe_text(&sender.manifest().filename),
            sender.manifest().file_size,
            sender.manifest().sha256,
            options.fps
        );
    }
    let event_loop = EventLoop::builder().with_wayland().build()?;
    let shared = Arc::new(Mutex::new(View {
        manifest: Some(sender.manifest().clone()),
        ..View::default()
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, commands) = mpsc::sync_channel(32);
    let _panic_log = tui.then(|| vft_tui::PanicLog::install(shared.clone()));
    let mut worker = TerminalWorker {
        shared: shared.clone(),
        stop: stop.clone(),
        thread: None,
    };
    if tui {
        let view = shared.clone();
        let cancel = stop.clone();
        let proxy = event_loop.create_proxy();
        worker.thread = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sender_ui(view.clone(), cancel.clone(), tx, || {
                    let _ = proxy.send_event(());
                })
            }))
            .unwrap_or_else(|_| Err(std::io::Error::other("terminal worker panicked")));
            if let Err(e) = result {
                let mut v = view.lock().unwrap();
                v.state = Lifecycle::Failed;
                v.result = e.to_string();
                v.log(&e.to_string());
                vft_tui::stop(&cancel);
            }
        }));
    }
    let mut app = App {
        options,
        sender,
        window: None,
        surface: None,
        frame: None,
        paused: false,
        occluded: false,
        next: Instant::now(),
        presented: 0,
        error: None,
        shared,
        stop,
        commands,
        tui,
    };
    event_loop.run_app(&mut app)?;
    if !app.tui {
        println!(
            "Presented frames: {} (not measured display refresh rate)",
            app.presented
        );
    }
    if let Some(error) = app.error {
        return Err(error.into());
    }
    let view = app.shared.lock().unwrap();
    if view.state == Lifecycle::Failed {
        return Err(view.result.clone().into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_options() {
        for args in [
            vec!["f", "--fps", "0"],
            vec!["f", "--cell-size", "0"],
            vec!["f", "--frames", "0"],
            vec!["f", "--profile", "bad"],
            vec!["f", "--tui", "--no-tui"],
        ] {
            assert!(options(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }
}
