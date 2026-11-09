mod capture;
use crate::output::OutputDestination;
use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
    sync::{atomic::AtomicBool, mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use vft_linux::{save_preview, save_verified, Decoder, Result};
use vft_protocol::ProfileId;
use vft_tui::{
    terminal::{random_id, receiver_ui, ReceiverCommand},
    Lifecycle, SharedView, Stop, View,
};
use vft_visual::{ImageRect, LumaImage};

struct Options {
    rect: Option<ImageRect>,
    search: Option<ImageRect>,
    output: OutputDestination,
    preview: Option<PathBuf>,
    stats: Option<PathBuf>,
    profile: Option<ProfileId>,
    fps: u32,
    timeout: u64,
    max_file_size: u64,
    tui: Option<bool>,
}
fn options(args: &[String]) -> Result<Option<Options>> {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("vft-recv-linux (--auto | --rect X Y WIDTH HEIGHT)\n  [--output FILE] [--output-dir DIR]\n  [--search-rect X Y WIDTH HEIGHT] [--profile auto|robust|balanced]\n  [--fps 30] [--timeout 3600] [--max-file-size 268435456]\n  [--preview FILE.bmp] [--stats FILE.csv] [--tui | --no-tui]\nWithout --output, save the metadata filename in --output-dir (default: current directory).\nThe directory must exist. --output uses its exact path and overrides --output-dir.\nChoose a monitor/window in the ScreenCast portal. Coordinates are selected-stream pixels,\nnot global desktop coordinates. Output, preview and statistics must be new, distinct files.");
        return Ok(None);
    }
    let mut o = Options {
        rect: None,
        search: None,
        output: OutputDestination::default(),
        preview: None,
        stats: None,
        profile: None,
        fps: 30,
        timeout: 3600,
        max_file_size: 256 * 1024 * 1024,
        tui: None,
    };
    let mut auto = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || iter.next().ok_or("missing option value");
        match arg.as_str() {
            "--auto" => auto = true,
            "--rect" | "--search-rect" => {
                let r = ImageRect {
                    x: value()?.parse()?,
                    y: value()?.parse()?,
                    width: value()?.parse()?,
                    height: value()?.parse()?,
                };
                if r.width < 192
                    || r.height < 108
                    || r.x.checked_add(r.width).is_none_or(|n| n > 8192)
                    || r.y.checked_add(r.height).is_none_or(|n| n > 8192)
                {
                    return Err("invalid stream rectangle (maximum 8192x8192)".into());
                }
                if arg == "--rect" {
                    o.rect = Some(r);
                } else {
                    o.search = Some(r);
                }
            }
            "--output" => o.output.file = Some(value()?.into()),
            "--output-dir" => o.output.directory = value()?.into(),
            "--preview" => o.preview = Some(value()?.into()),
            "--stats" => o.stats = Some(value()?.into()),
            "--profile" => {
                o.profile = match value()?.as_str() {
                    "auto" => None,
                    "balanced" => Some(ProfileId::Balanced),
                    "robust" => Some(ProfileId::Robust),
                    _ => return Err("unknown profile".into()),
                }
            }
            "--fps" => o.fps = value()?.parse()?,
            "--timeout" => o.timeout = value()?.parse()?,
            "--max-file-size" => o.max_file_size = value()?.parse()?,
            "--tui" => vft_tui::mode_flag(&mut o.tui, true)?,
            "--no-tui" => vft_tui::mode_flag(&mut o.tui, false)?,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if auto == o.rect.is_some()
        || (o.search.is_some() && !auto)
        || !(1..=120).contains(&o.fps)
        || o.timeout == 0
    {
        return Err("supply exactly one of --auto/--rect, FPS 1..120 and positive timeout; --search-rect requires --auto".into());
    }
    o.output
        .validate(o.preview.as_deref(), o.stats.as_deref())?;
    Ok(Some(o))
}
struct State {
    options: Options,
    decoder: Decoder,
    start: Instant,
    last_report: Instant,
    captures: u64,
    valid: u64,
    preview_saved: bool,
    shared: SharedView,
    stop: Stop,
    commands: mpsc::Receiver<ReceiverCommand>,
    page: usize,
}
impl State {
    fn report(&self, message: &str) {
        if self.options.tui == Some(true) {
            self.shared.lock().unwrap().log(message);
        } else {
            println!("{}", vft_tui::safe_text(message));
        }
    }
    fn details(&self) -> Vec<String> {
        let p = self.decoder.receiver.progress();
        let mut details = vec![
            format!(
                "Recovered bytes: {}   Rank: {}   Hash: {:?}",
                p.recovered_bytes, p.rank_symbols, p.hash
            ),
            format!(
                "Processed captures: {}   Valid frames: {}   Invalid: {}   Locks: {}",
                self.captures, self.valid, self.decoder.invalid, self.decoder.locks
            ),
            format!(
                "Elapsed: {:.1}s   Requested processing FPS: {}",
                self.start.elapsed().as_secs_f64(),
                self.options.fps
            ),
        ];
        if let Some(path) = self.options.output.path() {
            details.push(format!(
                "Output: {}",
                vft_tui::safe_text(&path.display().to_string())
            ));
        }
        details
    }
    fn refresh_view(&self) {
        let mut v = self.shared.lock().unwrap();
        v.state = if self.decoder.receiver.is_complete() {
            Lifecycle::Complete
        } else {
            Lifecycle::Running
        };
        v.title = String::from("VFT Receive — Wayland portal / PipeWire");
        v.details = self.details();
        v.manifest = self.decoder.receiver.manifest().cloned();
        vft_tui::update_groups(&mut v, &self.decoder.receiver, self.page);
    }
    fn tick(&mut self, image: Option<LumaImage>) -> Result<bool> {
        if vft_tui::stopped(&self.stop) {
            return Err("capture stopped before verified completion".into());
        }
        if self.start.elapsed() >= Duration::from_secs(self.options.timeout) {
            return Err("capture timed out before verified completion".into());
        }
        while let Ok(command) = self.commands.try_recv() {
            match command {
                ReceiverCommand::Page(p) => self.page = p,
                ReceiverCommand::Generate(selected) => {
                    let snapshot = vft_tui::generate_snapshot(
                        &self.decoder.receiver,
                        &selected,
                        random_id,
                        format!("{:.1}s", self.start.elapsed().as_secs_f64()),
                    );
                    match snapshot {
                        Ok(s) => self.shared.lock().unwrap().request = Some(s),
                        Err(e) => self.report(&e),
                    }
                }
            }
        }
        if let Some(image) = image {
            self.captures += 1;
            let locks = self.decoder.locks;
            if let Some(rect) = self.decoder.push(&image, Instant::now())? {
                self.valid += 1;
                if self.options.output.path().is_none() {
                    if let Some(manifest) = self.decoder.receiver.manifest() {
                        let path = self.options.output.resolve(
                            &manifest.filename,
                            self.options.preview.as_deref(),
                            self.options.stats.as_deref(),
                        )?;
                        let message = format!("Output: {}", path.display());
                        self.report(&message);
                    }
                }
                if self.decoder.locks != locks {
                    self.report(&format!(
                        "LOCATED stream_rect={} {} {} {}",
                        rect.x, rect.y, rect.width, rect.height
                    ));
                }
                if !self.preview_saved {
                    if let Some(path) = &self.options.preview {
                        save_preview(&image.crop(rect)?, path)?;
                    }
                    self.preview_saved = true;
                }
            }
        }
        if self.decoder.receiver.is_complete() {
            let output = self
                .options
                .output
                .path()
                .ok_or("missing resolved output path")?;
            save_verified(&self.decoder.receiver, output)?;
            // Completion must replace the last throttled progress snapshot, including
            // recovered bytes, hash and incomplete groups. Keep the path in scrollable text.
            self.refresh_view();
            self.shared.lock().unwrap().result = format!(
                "SHA-256 verified; output: {}. Q exits.",
                vft_tui::safe_text(&output.display().to_string())
            );
            self.report(&format!(
                "COMPLETE SHA-256 verified; output={}",
                vft_tui::safe_text(&output.display().to_string())
            ));
            return Ok(true);
        }
        if self.last_report.elapsed() >= Duration::from_millis(200) {
            if self.options.tui == Some(true) {
                self.refresh_view();
            } else if self.last_report.elapsed() >= Duration::from_secs(1) {
                self.report(&self.details().join(" | "));
            }
            if self.options.tui == Some(true)
                || self.last_report.elapsed() >= Duration::from_secs(1)
            {
                self.last_report = Instant::now();
            }
        }
        Ok(false)
    }
    fn stats(&self, success: bool) -> Result<()> {
        let Some(path) = &self.options.stats else {
            return Ok(());
        };
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let elapsed = self.start.elapsed().as_secs_f64();
        let bytes = if success {
            self.decoder.receiver.manifest().map_or(0, |m| m.file_size)
        } else {
            0
        };
        writeln!(f, "success,elapsed_seconds,verified_bytes,verified_bytes_per_second,processed_captures,valid_frames,invalid_frames,location_scans,location_locks,requested_fps")?;
        writeln!(
            f,
            "{success},{elapsed:.6},{bytes},{:.3},{},{},{},{},{},{}",
            bytes as f64 / elapsed.max(0.000001),
            self.captures,
            self.valid,
            self.decoder.invalid,
            self.decoder.scans,
            self.decoder.locks,
            self.options.fps
        )?;
        f.sync_all()?;
        Ok(())
    }
}
fn receive(
    options: Options,
    shared: SharedView,
    stop: Stop,
    commands: mpsc::Receiver<ReceiverCommand>,
) -> Result<()> {
    let now = Instant::now();
    let mut state = State {
        decoder: Decoder::new(
            options.profile,
            options.rect,
            options.search,
            options.max_file_size,
        ),
        options,
        start: now,
        last_report: now,
        captures: 0,
        valid: 0,
        preview_saved: false,
        shared,
        stop,
        commands,
        page: 0,
    };
    state.report(
        "Select a monitor or window in the ScreenCast portal; keep the complete VFT frame visible.",
    );
    let result = capture::run(&mut state);
    // Preserve failed-run diagnostics; a stats error must not conceal the original capture failure.
    if let Err(e) = state.stats(result.is_ok()) {
        if result.is_ok() {
            return Err(e);
        }
        state.report(&format!("Statistics failed: {e}"));
    }
    result
}
struct CaptureWorker {
    stop: Stop,
    thread: Option<std::thread::JoinHandle<Result<()>>>,
}
impl Drop for CaptureWorker {
    fn drop(&mut self) {
        vft_tui::stop(&self.stop);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
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
    let shared = Arc::new(Mutex::new(View {
        title: "VFT Receive — choose capture source".into(),
        ..View::default()
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(32);
    if !tui {
        return receive(options, shared, stop, rx);
    }
    let _panic_log = vft_tui::PanicLog::install(shared.clone());
    let view = shared.clone();
    let cancel = stop.clone();
    let worker = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            receive(options, view.clone(), cancel, rx)
        }))
        .unwrap_or_else(|_| Err("capture worker panicked".into()));
        let mut v = view.lock().unwrap();
        v.state = if result.is_ok() {
            Lifecycle::Complete
        } else {
            Lifecycle::Failed
        };
        if let Err(error) = &result {
            v.result = error.to_string();
        }
        v.worker_done = true;
        result
    });
    let mut worker = CaptureWorker {
        stop: stop.clone(),
        thread: Some(worker),
    };
    let ui_result = receiver_ui(shared, stop.clone(), tx);
    vft_tui::stop(&stop);
    let result = worker
        .thread
        .take()
        .unwrap()
        .join()
        .map_err(|_| "capture worker panicked")?;
    ui_result?;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_options_default_to_metadata_and_explicit_paths_override_directories() {
        let parse =
            |args: &[&str]| options(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
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
        let mut options = parse(&["--auto", "--output-dir", dir.to_str().unwrap()])
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
    fn strict_options_before_portal() {
        for args in [
            vec!["--auto", "--output-dir", ""],
            vec!["--auto", "--rect", "0", "0", "384", "216", "--output", "f"],
            vec!["--rect", "-1", "0", "384", "216", "--output", "f"],
            vec!["--auto", "--output", "f", "--fps", "0"],
            vec!["--auto", "--output", "f", "--preview", "./f"],
        ] {
            assert!(options(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    #[test]
    fn completion_publishes_verified_bytes_hash_groups_and_output_without_waiting_for_timer() {
        completion(false, false);
    }
    #[test]
    fn metadata_completion_uses_the_original_name_and_preserves_a_late_existing_file() {
        completion(true, false);
        completion(true, true);
    }
    fn completion(metadata: bool, race: bool) {
        use vft_protocol::{Digest, Sha256};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../output/linux-tui-tests");
        std::fs::create_dir_all(&root).unwrap();
        let directory = root.join(format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        let input = directory.join("input.bin");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&input)
            .unwrap()
            .write_all(&[9; 100])
            .unwrap();
        let received = directory.join("received");
        std::fs::create_dir(&received).unwrap();
        let output = received.join(if metadata {
            "input.bin"
        } else {
            "recovered.bin"
        });
        let mut args = vec![
            "--auto".into(),
            "--output-dir".into(),
            received.to_str().unwrap().into(),
        ];
        if !metadata {
            args.extend(["--output".into(), output.to_str().unwrap().into()]);
        }
        let mut options = options(&args).unwrap().unwrap();
        options.tui = Some(true);
        let mut sender = vft_linux::open_sender(&input, ProfileId::Balanced).unwrap();
        let shared = Arc::new(Mutex::new(View::default()));
        let (_, commands) = mpsc::sync_channel(32);
        let now = Instant::now();
        let mut state = State {
            options,
            decoder: Decoder::new(None, None, None, 1000),
            start: now,
            last_report: now,
            captures: 0,
            valid: 0,
            preview_saved: false,
            shared: shared.clone(),
            stop: Arc::new(AtomicBool::new(false)),
            commands,
            page: 0,
        };
        assert!(!state
            .tick(Some(sender.next_frame().unwrap().render(2).unwrap()))
            .unwrap());
        state.refresh_view();
        assert!(!output.exists());
        assert!(shared
            .lock()
            .unwrap()
            .details
            .join(" ")
            .contains(&output.display().to_string()));
        assert_eq!(shared.lock().unwrap().missing, 1);
        assert!(shared
            .lock()
            .unwrap()
            .details
            .join(" ")
            .contains("Hash: Pending"));
        if race {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .unwrap()
                .write_all(b"keep existing")
                .unwrap();
            assert!(state
                .tick(Some(sender.next_frame().unwrap().render(2).unwrap()))
                .is_err());
            assert_eq!(std::fs::read(output).unwrap(), b"keep existing");
            assert_ne!(shared.lock().unwrap().state, Lifecycle::Complete);
            return;
        }
        assert!(state
            .tick(Some(sender.next_frame().unwrap().render(2).unwrap()))
            .unwrap());
        let view = shared.lock().unwrap();
        assert_eq!(view.state, Lifecycle::Complete);
        assert!(view.details.join(" ").contains("Recovered bytes: 100"));
        assert!(view.details.join(" ").contains("Hash: Verified"));
        assert_eq!(view.missing, 0);
        assert!(view.groups.is_empty());
        assert!(view.result.contains(&output.display().to_string()));
        assert_eq!(
            Sha256::digest(std::fs::read(output).unwrap()),
            Sha256::digest([9; 100])
        );
    }
}
