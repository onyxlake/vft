use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use vft_git::{Control, Prepared};
pub fn git(repo: &Path, args: &[&str]) -> String {
    git_with_global_config(repo, args, None)
}
fn git_with_global_config(repo: &Path, args: &[&str], global_config: Option<&Path>) -> String {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(config) = global_config {
        command.env("GIT_CONFIG_GLOBAL", config);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
pub fn fixture() -> (PathBuf, PathBuf, PathBuf, String) {
    fixture_format("sha1")
}
pub fn fixture_format(format: &str) -> (PathBuf, PathBuf, PathBuf, String) {
    fixture_format_with_global_config(format, None)
}
pub fn fixture_format_with_global_config(
    format: &str,
    global_config: Option<&str>,
) -> (PathBuf, PathBuf, PathBuf, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../output/git-tests");
    fs::create_dir_all(&root).unwrap();
    let mut tmp = vft_git::TempDir::in_directory(&root, "vft-git-work").unwrap();
    tmp.preserve();
    // Scope external configuration to these Git children, not parallel test threads.
    let global_config = global_config.map(|contents| {
        // Git config paths must not contain Windows canonical verbatim prefixes.
        let path = root
            .join(tmp.path().file_name().unwrap())
            .join("global.gitconfig");
        fs::write(&path, contents).unwrap();
        path
    });
    let root = tmp.path().to_owned();
    let git =
        |repo: &Path, args: &[&str]| git_with_global_config(repo, args, global_config.as_deref());
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    git(
        &source,
        &[
            "init",
            "--template=",
            "--initial-branch=main",
            &format!("--object-format={format}"),
        ],
    );
    git(&source, &["config", "user.name", "VFT Test"]);
    git(&source, &["config", "user.email", "vft@example.invalid"]);
    git(&source, &["config", "commit.gpgsign", "false"]);
    git(&source, &["config", "core.autocrlf", "false"]);
    fs::write(source.join("text.txt"), b"baseline\n").unwrap();
    fs::write(source.join("remove.txt"), b"delete me\n").unwrap();
    fs::write(source.join("rename.txt"), b"rename me\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-qm", "base"]);
    let base = git(&source, &["rev-parse", "HEAD"]);
    let receiver = root.join("receiver");
    // Clone performs the first checkout, so disable conversion before it starts.
    git(
        &root,
        &[
            "clone",
            "--no-hardlinks",
            "--config",
            "core.autocrlf=false",
            "source",
            "receiver",
        ],
    );
    for (key, value) in [
        ("user.name", "VFT Test"),
        ("user.email", "vft@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&receiver, &["config", key, value]);
    }
    let status = git(
        &receiver,
        &["status", "--porcelain=v1", "--untracked-files=no"],
    );
    assert!(
        status.is_empty(),
        "receiver fixture must start with a clean index/worktree: {status}"
    );
    (root, source, receiver, base)
}
pub fn advance(source: &Path) -> String {
    fs::write(source.join("text.txt"), b"updated text\n").unwrap();
    fs::write(
        source.join("data.bin"),
        (0..4096).map(|i| (i % 256) as u8).collect::<Vec<_>>(),
    )
    .unwrap();
    fs::remove_file(source.join("remove.txt")).unwrap();
    fs::rename(source.join("rename.txt"), source.join("renamed.txt")).unwrap();
    git(source, &["add", "."]);
    git(source, &["commit", "-qm", "target"]);
    git(source, &["rev-parse", "HEAD"])
}
pub fn package(source: &Path, base: &str, path: &Path) -> PathBuf {
    match vft_git::prepare(source, base, "HEAD", Some(path), &Control::default()).unwrap() {
        Prepared::Package { path, .. } => path,
        _ => panic!("expected update"),
    }
}
