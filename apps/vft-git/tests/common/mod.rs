use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use vft_git::{Control, Prepared};
pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../output/git-tests");
    fs::create_dir_all(&root).unwrap();
    let mut tmp = vft_git::TempDir::in_directory(&root, "vft-git-work").unwrap();
    tmp.preserve();
    let root = tmp.path().to_owned();
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
    git(&root, &["clone", "--no-hardlinks", "source", "receiver"]);
    for (key, value) in [
        ("user.name", "VFT Test"),
        ("user.email", "vft@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&receiver, &["config", key, value]);
    }
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
