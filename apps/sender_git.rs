//! Shared native sender startup, before any display/capture initialization.
use std::path::PathBuf;
use vft_git::{cli::SenderOptions, Control, Prepared, Result};
pub fn prepare(options: &SenderOptions, tui: bool) -> Result<Option<PathBuf>> {
    let prepared = if tui {
        use std::sync::{Arc, Mutex};
        use vft_tui::{Lifecycle, View};
        let control = Control::default();
        let shared = Arc::new(Mutex::new(View {
            title: "VFT Git package preparation".into(),
            state: Lifecycle::Packing,
            details: vec![
                "Checking repository, pinning target and creating an offline bundle".into(),
            ],
            result: "Complete packages are retained for sending or offline retry.".into(),
            ..Default::default()
        }));
        let _panic_log = vft_tui::PanicLog::install(shared.clone());
        std::thread::scope(|scope| -> Result<Prepared> {
            let worker = scope.spawn(|| {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    options.prepare(&control)
                }))
                .unwrap_or_else(|_| Err("Git package worker panicked".into()));
                shared.lock().unwrap().worker_done = true;
                result
            });
            let ui = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                vft_tui::terminal::startup_ui(shared.clone(), control.cancel.clone())
            }))
            .unwrap_or_else(|_| Err(std::io::Error::other("Git preparation TUI panicked")));
            if ui.is_err() {
                vft_tui::stop(&control.cancel);
            }
            let result = worker
                .join()
                .map_err(|_| "Git package worker could not be joined")?;
            ui?;
            result
        })?
    } else {
        let control = Control::default();
        let _interrupt = vft_git::InterruptGuard::install(control.cancel.clone())?;
        println!("Packing committed Git history...");
        options.prepare(&control)?
    };
    match prepared {
        Prepared::NoChange { head } => {
            println!("No Git update needed: receiver and target are {head}");
            Ok(None)
        }
        Prepared::Package { path, header } => {
            println!("Git package ready: {}\nBase: {}\nTarget: {}\nKeep this package unchanged during transmission.", vft_tui::safe_text(&path.display().to_string()), header.base, header.target);
            Ok((!options.prepare_only).then_some(path))
        }
    }
}
