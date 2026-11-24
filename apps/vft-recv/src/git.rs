use std::path::Path;
use vft_git::{Control, ReceiveSession, Result};
use vft_tui::{Lifecycle, SharedView, Stop};
/// Shared post-save phase. The caller retains the package before entering here.
/// Never publish Complete merely because the VFT core has recovered its bytes.
pub fn apply(
    session: &ReceiveSession,
    path: &Path,
    limit: u64,
    shared: Option<&SharedView>,
    stop: Option<&Stop>,
) -> Result<String> {
    let mut control = Control::default();
    if let Some(stop) = stop {
        control.cancel = stop.clone();
    }
    let outcome = session.apply(path, limit, &control, |message| {
        if let Some(shared) = shared {
            let mut view = shared.lock().unwrap();
            view.state = if message.starts_with("Verifying") {
                Lifecycle::Verifying
            } else {
                Lifecycle::Applying
            };
            view.result = message.into();
            view.log(message);
            view.details = vec![
                message.into(),
                format!(
                    "Package retained: {}",
                    vft_tui::safe_text(&path.display().to_string())
                ),
            ];
        } else {
            println!("{}", vft_tui::safe_text(message));
        }
    });
    match outcome {
        Ok(outcome) => Ok(format!(
            "Git {}: {} at {}; package retained: {}{}",
            if outcome.already_current {
                "already current"
            } else {
                "applied"
            },
            session.branch,
            outcome.target,
            vft_tui::safe_text(&path.display().to_string()),
            outcome
                .backup_ref
                .map(|r| format!("; previous commit: {r}"))
                .unwrap_or_default()
        )),
        Err(error) => {
            if let Some(shared) = shared {
                let mut view = shared.lock().unwrap();
                view.state = Lifecycle::Failed;
                view.result = error.to_string();
                view.log(&error.to_string());
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_fixture::*;
    use std::{
        fs,
        sync::{Arc, Mutex},
    };
    #[test]
    fn saved_package_failure_never_publishes_complete_and_retry_works() {
        let (root, source, receiver, base) = fixture();
        let target = advance(&source);
        let path = package(&source, &base, &root.join("received.vftgit"));
        let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
        fs::write(receiver.join("text.txt"), b"local work").unwrap();
        let view = Arc::new(Mutex::new(vft_tui::View::default()));
        let error = apply(
            &session,
            &path,
            vft_git::DEFAULT_MAX_PACKAGE,
            Some(&view),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("--git-apply"));
        assert_eq!(view.lock().unwrap().state, Lifecycle::Failed);
        assert!(path.is_file());
        assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), base);
        git(&receiver, &["restore", "text.txt"]);
        let result = apply(
            &session,
            &path,
            vft_git::DEFAULT_MAX_PACKAGE,
            Some(&view),
            None,
        )
        .unwrap();
        assert!(result.contains(&target));
        // The adapter owns the overall Complete transition after all output steps succeed.
        assert_ne!(view.lock().unwrap().state, Lifecycle::Complete);
    }
}
