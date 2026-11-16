mod common;
use common::*;
use std::{fs, path::Path, sync::atomic::Ordering};
use vft_git::{prepare, Control, Prepared, ReceiveSession, DEFAULT_MAX_PACKAGE};
fn apply(session: &ReceiveSession, path: &Path) -> vft_git::Result<vft_git::ApplyOutcome> {
    session.apply(path, DEFAULT_MAX_PACKAGE, &Control::default(), |_| {})
}
#[test]
fn normal_delta_merge_history_and_safe_repeat() {
    for format in ["sha1", "sha256"] {
        let (root, source, receiver, base) = fixture_format(format);
        advance(&source);
        git(&source, &["switch", "-c", "side", &base]);
        fs::write(source.join("side.txt"), b"side\n").unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-qm", "side"]);
        git(&source, &["switch", "main"]);
        git(&source, &["merge", "--no-ff", "--no-edit", "side"]);
        let target = git(&source, &["rev-parse", "HEAD"]);
        let path = package(&source, &base, &root.join("update.vftgit"));
        let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
        assert_eq!(session.head, base);
        let result = apply(&session, &path).unwrap();
        assert_eq!(result.target, target);
        assert_eq!(
            git(
                &receiver,
                &["rev-parse", result.backup_ref.as_ref().unwrap()]
            ),
            base
        );
        assert_eq!(
            git(&receiver, &["rev-parse", "HEAD^{tree}"]),
            git(&source, &["rev-parse", "HEAD^{tree}"])
        );
        for name in ["text.txt", "data.bin", "renamed.txt", "side.txt"] {
            assert_eq!(
                fs::read(receiver.join(name)).unwrap(),
                fs::read(source.join(name)).unwrap()
            );
        }
        assert!(!receiver.join("remove.txt").exists());
        assert!(path.exists());
        drop(session);
        let retry = ReceiveSession::open(&receiver, Some("main"), &Control::default()).unwrap();
        assert!(apply(&retry, &path).unwrap().already_current);
        assert!(git(
            &receiver,
            &["for-each-ref", "--format=%(refname)", "refs/vft/transfer/"]
        )
        .is_empty());
    }
}
#[test]
fn empty_delta_invalid_baselines_and_no_overwrite() {
    let (root, source, _, base) = fixture();
    assert!(matches!(
        prepare(&source, &base, "HEAD", None, &Control::default()).unwrap(),
        Prepared::NoChange { .. }
    ));
    advance(&source);
    for b in ["a".repeat(40), "--all".into(), base[..8].into()] {
        assert!(prepare(&source, &b, "HEAD", None, &Control::default()).is_err());
    }
    assert!(prepare(&source, &base, "HEAD~1", None, &Control::default()).is_err());
    let path = root.join("exists");
    fs::write(&path, b"keep").unwrap();
    assert!(prepare(&source, &base, "HEAD", Some(&path), &Control::default()).is_err());
    assert_eq!(fs::read(path).unwrap(), b"keep");
}
#[test]
fn dirty_and_collisions_preserve_package_then_offline_retry() {
    for kind in ["index", "worktree", "untracked", "ignored"] {
        let (root, source, receiver, base) = fixture();
        advance(&source);
        let package = package(&source, &base, &root.join("update.vftgit"));
        let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
        let path = receiver.join(if kind == "index" || kind == "worktree" {
            "text.txt"
        } else {
            "data.bin"
        });
        fs::write(&path, b"keep my local bytes").unwrap();
        if kind == "index" {
            git(&receiver, &["add", "text.txt"]);
        }
        if kind == "ignored" {
            fs::write(receiver.join(".git/info/exclude"), b"data.bin\n").unwrap();
        }
        let error = apply(&session, &package).unwrap_err().to_string();
        assert!(error.contains("--git-apply"), "{error}");
        assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), base);
        assert_eq!(fs::read(&path).unwrap(), b"keep my local bytes");
        assert!(package.exists());
        drop(session);
        if kind == "index" {
            git(&receiver, &["reset", "--", "text.txt"]);
        }
        if kind == "index" || kind == "worktree" {
            git(&receiver, &["restore", "text.txt"]);
        } else {
            fs::remove_file(&path).unwrap();
        }
        let retry = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
        apply(&retry, &package).unwrap();
    }
}
#[test]
fn head_branch_lock_and_dirty_startup_guards() {
    let (root, source, receiver, base) = fixture();
    advance(&source);
    let package = package(&source, &base, &root.join("update.vftgit"));
    let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
    assert!(ReceiveSession::open(&receiver, None, &Control::default()).is_err());
    git(&receiver, &["switch", "-c", "other"]);
    assert!(apply(&session, &package)
        .unwrap_err()
        .to_string()
        .contains("changed since startup"));
    git(&receiver, &["switch", "main"]);
    fs::write(receiver.join("other.txt"), b"other").unwrap();
    git(&receiver, &["add", "."]);
    git(&receiver, &["commit", "-qm", "divergent"]);
    assert!(apply(&session, &package).is_err());
    drop(session);
    assert!(ReceiveSession::open(&receiver, Some("other"), &Control::default()).is_err());
    let fresh = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
    assert!(apply(&fresh, &package)
        .unwrap_err()
        .to_string()
        .contains("baseline"));
    drop(fresh);
    git(&receiver, &["switch", "--detach"]);
    assert!(ReceiveSession::open(&receiver, None, &Control::default()).is_err());
    git(&receiver, &["switch", "--orphan", "unborn"]);
    assert!(ReceiveSession::open(&receiver, None, &Control::default()).is_err());
}
#[test]
fn repository_discovery_subdirectory_nonrepo_and_cancellation() {
    let (root, source, receiver, base) = fixture();
    advance(&source);
    fs::create_dir(source.join("sub")).unwrap();
    let package = package(&source.join("sub"), &base, &root.join("update.vftgit"));
    fs::create_dir(receiver.join("sub")).unwrap();
    let session = ReceiveSession::open(&receiver.join("sub"), None, &Control::default()).unwrap();
    assert_eq!(session.root(), receiver.canonicalize().unwrap());
    let control = Control::default();
    control.cancel.store(true, Ordering::Release);
    assert!(session
        .apply(&package, DEFAULT_MAX_PACKAGE, &control, |_| {})
        .is_err());
    assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), base);
    // Fixtures live inside the VFT checkout, so an ordinary directory here discovers
    // that parent repository. A gitfile pointing to an absent directory stops discovery
    // locally, without depending on the parent worktree's status or permissions.
    let non_repository = root.join("non-repository");
    fs::create_dir(&non_repository).unwrap();
    fs::write(non_repository.join(".git"), b"gitdir: missing.git\n").unwrap();
    let error =
        ReceiveSession::open(&non_repository, None, &Control::default()).unwrap_err_string();
    assert!(
        error.contains("not a git repository")
            || error.contains("gitfile does not point to a valid repository"),
        "{error}"
    );
    let no_dir = root.join("not-directory");
    fs::write(&no_dir, b"file").unwrap();
    assert!(vft_git::TempDir::in_directory(&no_dir, "vft-git").is_err());
}
#[test]
fn shallow_missing_objects_submodule_lfs_and_external_filters_rejected() {
    let (root, source, receiver, base) = fixture();
    advance(&source);
    fs::write(receiver.join(".git/shallow"), format!("{base}\n")).unwrap();
    assert!(ReceiveSession::open(&receiver, None, &Control::default())
        .unwrap_err_string()
        .contains("shallow"));
    fs::remove_file(receiver.join(".git/shallow")).unwrap();
    let object = receiver.join(format!(".git/objects/{}/{}", &base[..2], &base[2..]));
    fs::remove_file(object).unwrap();
    assert!(ReceiveSession::open(&receiver, None, &Control::default()).is_err());
    for (name, data) in [
        (".gitmodules", "[submodule \"x\"]\n"),
        (
            "pointer",
            "version https://git-lfs.github.com/spec/v1\noid sha256:abcd\nsize 4\n",
        ),
    ] {
        fs::write(source.join(name), data).unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-qm", "unsupported"]);
        assert!(prepare(
            &source,
            &base,
            "HEAD",
            Some(&root.join("rejected")),
            &Control::default()
        )
        .is_err());
        git(&source, &["reset", "--hard", "HEAD~1"]);
    }
    git(&source, &["config", "filter.bad.smudge", "not-a-command"]);
    assert!(prepare(
        &source,
        &base,
        "HEAD",
        Some(&root.join("unused-filter")),
        &Control::default()
    )
    .is_ok());
    fs::write(source.join(".gitattributes"), b"*.txt filter=bad\n").unwrap();
    git(&source, &["add", ".gitattributes"]);
    git(&source, &["commit", "-qm", "active filter"]);
    assert!(prepare(&source, &base, "HEAD", None, &Control::default()).is_err());
}
trait ErrorString {
    fn unwrap_err_string(self) -> String;
}
impl<T> ErrorString for vft_git::Result<T> {
    fn unwrap_err_string(self) -> String {
        match self {
            Ok(_) => panic!("expected error"),
            Err(e) => e.to_string(),
        }
    }
}
#[test]
fn corrupted_truncated_size_and_object_format_packages_never_update() {
    let (root, source, receiver, base) = fixture();
    advance(&source);
    let path = package(&source, &base, &root.join("good"));
    let bytes = fs::read(path).unwrap();
    let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
    for i in [0, 8, 10, 12, 13, 16, 24, 56, 88, 120, 128, bytes.len() - 1] {
        let mut bad = bytes.clone();
        bad[i] ^= 0x80;
        let path = root.join(format!("corrupt-{i}"));
        fs::write(&path, bad).unwrap();
        assert!(apply(&session, &path).is_err(), "byte {i}");
        assert!(path.exists());
        assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), base);
    }
    for len in [0, 127, 128, bytes.len() - 1] {
        let path = root.join(format!("short-{len}"));
        fs::write(&path, &bytes[..len]).unwrap();
        assert!(apply(&session, &path).is_err());
    }
    let path = root.join("too-large");
    fs::write(&path, &bytes).unwrap();
    assert!(session
        .apply(&path, bytes.len() as u64 - 1, &Control::default(), |_| {})
        .is_err());
}

#[test]
fn cancellation_before_and_after_update_boundary_and_subprocess_timeout() {
    for during_update in [false, true] {
        let (root, source, receiver, base) = fixture();
        let target = advance(&source);
        let path = package(&source, &base, &root.join("update"));
        let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
        let control = Control::default();
        let result = session.apply(&path, DEFAULT_MAX_PACKAGE, &control, |phase| {
            if phase.starts_with(if during_update {
                "Applying fast-forward"
            } else {
                "Importing"
            }) {
                control.cancel.store(true, Ordering::Release);
            }
        });
        if during_update {
            assert!(result.is_ok());
            assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), target);
        } else {
            assert!(result.is_err());
            assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), base);
        }
        assert!(path.is_file());
    }
    let (_, source, _, base) = fixture();
    let control = Control {
        timeout: std::time::Duration::ZERO,
        ..Default::default()
    };
    assert!(prepare(&source, &base, "HEAD", None, &control)
        .unwrap_err()
        .to_string()
        .contains("timed out"));
}

#[test]
fn sender_target_stays_consistent_when_branch_moves_and_hooks_do_not_run() {
    let (root, source, receiver, base) = fixture();
    let first = advance(&source);
    fs::write(source.join("later.txt"), b"later\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-qm", "later"]);
    let second = git(&source, &["rev-parse", "HEAD"]);
    // An executable hook would make import/update fail if the private hooksPath were ignored.
    let hooks = receiver.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    for name in ["post-merge", "reference-transaction"] {
        let hook = hooks.join(name);
        fs::write(&hook, b"#!/bin/sh\necho ran > hook-ran\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(hook, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let src = source.clone();
    let a = first.clone();
    let b = second.clone();
    let moving = std::thread::spawn(move || {
        for n in 0..40 {
            git(
                &src,
                &[
                    "update-ref",
                    "refs/heads/main",
                    if n % 2 == 0 { &a } else { &b },
                ],
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
    let path = package(&source, &base, &root.join("update"));
    moving.join().unwrap();
    let bytes = fs::read(&path).unwrap();
    let header = vft_git::Header::decode(
        bytes[..128].try_into().unwrap(),
        bytes.len() as u64,
        DEFAULT_MAX_PACKAGE,
    )
    .unwrap();
    assert!([first, second].contains(&header.target));
    let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
    apply(&session, &path).unwrap();
    assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), header.target);
    assert!(!receiver.join("hook-ran").exists());
}

#[test]
fn true_object_format_mismatch_and_promisor_rejection() {
    let (root, source, _, base) = fixture_format("sha256");
    advance(&source);
    let path = package(&source, &base, &root.join("sha256"));
    let (_, _, receiver, _) = fixture();
    let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
    assert!(apply(&session, &path)
        .unwrap_err()
        .to_string()
        .contains("object-format mismatch"));
    drop(session);
    fs::write(receiver.join(".git/objects/pack/pack-test.promisor"), b"").unwrap();
    assert!(ReceiveSession::open(&receiver, None, &Control::default())
        .unwrap_err_string()
        .contains("promisor"));
}

#[test]
fn unused_configured_filters_are_inert_but_active_worktree_attributes_are_rejected() {
    let (root, source, receiver, base) = fixture();
    advance(&source);
    let path = package(&source, &base, &root.join("update"));
    let command = if cfg!(windows) {
        "cmd /c echo ran > vft-filter-ran"
    } else {
        "sh -c 'echo ran > vft-filter-ran'"
    };
    for field in ["clean", "smudge", "process"] {
        git(
            &receiver,
            &["config", &format!("filter.bad.{field}"), command],
        );
    }
    git(&receiver, &["config", "filter.bad.required", "true"]);
    let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
    // This untracked attribute file affects only the new binary in the target tree.
    fs::write(receiver.join(".gitattributes"), b"data.bin filter=bad\n").unwrap();
    assert!(apply(&session, &path)
        .unwrap_err()
        .to_string()
        .contains("filter attributes"));
    assert!(!receiver.join("vft-filter-ran").exists());
    assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), base);
    fs::remove_file(receiver.join(".gitattributes")).unwrap();
    apply(&session, &path).unwrap();
    assert!(!receiver.join("vft-filter-ran").exists());
}
