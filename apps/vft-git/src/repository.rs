use crate::{
    format::copy_hash, process::Git, temporary::unique_id, Control, Header, ObjectFormat, Result,
    TempDir, HEADER_LEN,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

struct Repository {
    git: Git,
    root: PathBuf,
    git_dir: PathBuf,
    format: ObjectFormat,
    _lock: File,
}
impl Repository {
    fn open(path: &Path, control: &Control) -> Result<Self> {
        let git = Git::new(path, control)?;
        let version = git.text(&["--version"], control)?;
        let parts: Vec<_> = version
            .strip_prefix("git version ")
            .unwrap_or("")
            .split('.')
            .take(2)
            .map(str::parse::<u32>)
            .collect();
        if !matches!(parts.as_slice(), [Ok(major), Ok(minor)] if (*major, *minor) >= (2,48)) {
            return Err(format!("Git 2.48+ required; found {version}").into());
        }
        if git.text(&["rev-parse", "--is-bare-repository"], control)? != "false" {
            return Err("Git mode requires a non-bare working repository".into());
        }
        let root =
            PathBuf::from(git.text(&["rev-parse", "--show-toplevel"], control)?).canonicalize()?;
        let git = Git::new(&root, control)?;
        let git_dir = PathBuf::from(git.text(&["rev-parse", "--absolute-git-dir"], control)?)
            .canonicalize()?;
        let common = PathBuf::from(git.text(
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            control,
        )?)
        .canonicalize()?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(common.join("vft-git.lock"))?;
        lock.try_lock()
            .map_err(|e| format!("another VFT Git operation holds the repository lock: {e}"))?;
        let format = match git
            .text(&["rev-parse", "--show-object-format"], control)?
            .as_str()
        {
            "sha1" => ObjectFormat::Sha1,
            "sha256" => ObjectFormat::Sha256,
            _ => return Err("unsupported repository object format".into()),
        };
        let repo = Self {
            git,
            root,
            git_dir,
            format,
            _lock: lock,
        };
        repo.validate(control)?;
        Ok(repo)
    }
    fn validate(&self, control: &Control) -> Result<()> {
        self.git.validate_filters(control)?;
        if self
            .git
            .text(&["rev-parse", "--is-shallow-repository"], control)?
            != "false"
        {
            return Err("shallow repositories are unsupported; a matching HEAD does not imply complete prerequisites".into());
        }
        let config = self.git.raw(
            [
                "config",
                "--get-regexp",
                "^(extensions\\.partialclone|remote\\..*\\.promisor)$",
            ],
            control,
        )?;
        if config.status.success() && !config.stdout.is_empty() {
            return Err("partial repositories are unsupported".into());
        }
        if !config.status.success() && config.status.code() != Some(1) {
            config.require()?;
        }
        let sparse = self.git.raw(
            ["config", "--bool", "--get", "core.sparseCheckout"],
            control,
        )?;
        if sparse.status.success() && sparse.stdout.starts_with(b"true") {
            return Err("sparse checkouts are unsupported".into());
        }
        if !sparse.status.success() && sparse.status.code() != Some(1) {
            sparse.require()?;
        }
        let packs = PathBuf::from(self.git.text(
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "objects/pack",
            ],
            control,
        )?);
        if packs.is_dir() {
            for file in fs::read_dir(packs)? {
                if file?
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "promisor")
                {
                    return Err("promisor/partial object stores are unsupported".into());
                }
            }
        }
        for entry in [
            "info/grafts",
            "objects/info/alternates",
            "objects/info/http-alternates",
        ] {
            let path = PathBuf::from(self.git.text(
                &["rev-parse", "--path-format=absolute", "--git-path", entry],
                control,
            )?);
            if path.exists() {
                return Err(format!("unsupported repository metadata: {entry}").into());
            }
        }
        if !self
            .git
            .run(
                &["for-each-ref", "--format=%(refname)", "refs/replace/"],
                control,
            )?
            .is_empty()
        {
            return Err("replace refs are unsupported".into());
        }
        self.git
            .run(
                &[
                    "fsck",
                    "--full",
                    "--strict",
                    "--no-reflogs",
                    "--no-dangling",
                ],
                control,
            )
            .map_err(|e| format!("repository has missing or invalid objects: {e}"))?;
        Ok(())
    }
    fn oid(&self, name: &str, control: &Control) -> Result<String> {
        let value = self.git.text(
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{name}^{{commit}}"),
            ],
            control,
        )?;
        self.format.validate_oid(&value)?;
        Ok(value)
    }
    fn branch(&self, control: &Control) -> Result<String> {
        let branch = self
            .git
            .text(&["symbolic-ref", "-q", "HEAD"], control)
            .map_err(|_| "detached HEAD is unsupported; check out the target branch first")?;
        if !branch.starts_with("refs/heads/") {
            return Err("HEAD must name a local branch".into());
        }
        validate_ref(&branch)?;
        self.oid("HEAD", control).map_err(|_| {
            "unborn HEAD is unsupported; receiving repository needs a committed baseline"
        })?;
        Ok(branch)
    }
    fn ancestor(&self, base: &str, target: &str, control: &Control) -> Result<()> {
        self.git
            .run(&["merge-base", "--is-ancestor", base, target], control)
            .map_err(|e| format!("base is not an ancestor of target (divergent history): {e}"))?;
        Ok(())
    }
    fn clean(&self, control: &Control) -> Result<()> {
        for entry in [
            "MERGE_HEAD",
            "AUTO_MERGE",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "REBASE_HEAD",
            "rebase-apply",
            "rebase-merge",
            "sequencer",
            "BISECT_START",
            "index.lock",
            "HEAD.lock",
        ] {
            if self.git_dir.join(entry).exists() {
                return Err(format!("repository operation in progress: {entry}").into());
            }
        }
        let files = self.git.run(&["ls-files", "-v", "-z"], control)?;
        if files.split(|b| *b == 0).any(|s| {
            s.first()
                .is_some_and(|b| b.is_ascii_lowercase() || *b == b'S')
        }) {
            return Err("assume-unchanged/skip-worktree index entries are unsupported".into());
        }
        if !self
            .git
            .run(
                &[
                    "status",
                    "--porcelain=v1",
                    "--untracked-files=no",
                    "--ignore-submodules=none",
                ],
                control,
            )?
            .is_empty()
        {
            return Err("Git index/worktree is dirty; commit or preserve local changes before offline retry".into());
        }
        Ok(())
    }
    fn supported_tree(&self, oid: &str, worktree: bool, control: &Control) -> Result<()> {
        let entries = self
            .git
            .run(&["ls-tree", "-r", "--full-tree", "-z", oid], control)?;
        let mut paths = Vec::with_capacity(entries.len());
        for entry in entries.split(|b| *b == 0).filter(|s| !s.is_empty()) {
            let tab = entry
                .iter()
                .position(|b| *b == b'\t')
                .ok_or("invalid ls-tree result")?;
            if entry.starts_with(b"160000 ")
                || entry[tab + 1..].rsplit(|b| *b == b'/').next() == Some(b".gitmodules")
            {
                return Err("submodules are unsupported".into());
            }
            paths.extend_from_slice(&entry[tab + 1..]);
            paths.push(0);
        }
        // Check committed attributes, plus receiving worktree attributes when applying.
        // An untracked .gitattributes can affect a newly arriving path too.
        for from_tree in [true, false].into_iter().take(if worktree { 2 } else { 1 }) {
            let source = format!("--source={oid}");
            let mut args = vec!["check-attr", "--stdin", "-z", "filter"];
            if from_tree {
                args.push(&source);
            }
            let attrs = self.git.with_input(&args, &paths, control)?;
            let fields: Vec<_> = attrs.split(|b| *b == 0).collect();
            if fields.last() != Some(&b"".as_slice()) || (fields.len() - 1) % 3 != 0 {
                return Err("invalid Git attribute response".into());
            }
            for triple in fields[..fields.len() - 1].chunks(3) {
                if triple[1] != b"filter" || !matches!(triple[2], b"unspecified" | b"unset") {
                    return Err("external filter attributes (including LFS) are unsupported".into());
                }
            }
        }
        let lfs = self.git.raw(
            [
                "grep",
                "-I",
                "-l",
                "-z",
                "--no-textconv",
                "-e",
                "^version https://git-lfs.github.com/spec/v1$",
                oid,
                "--",
            ],
            control,
        )?;
        if lfs.status.success() {
            return Err("Git LFS pointer content is unsupported".into());
        }
        if lfs.status.code() != Some(1) {
            lfs.require()?;
        }
        Ok(())
    }
    fn supported_delta(&self, base: &str, target: &str, control: &Control) -> Result<()> {
        self.supported_tree(base, false, control)?;
        let commits = self
            .git
            .text(&["rev-list", &format!("{base}..{target}")], control)?;
        for commit in commits.lines() {
            self.format.validate_oid(commit)?;
            self.supported_tree(commit, false, control)?;
        }
        Ok(())
    }
}
fn validate_ref(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('=')
        || name.len() > 1024
        || name.starts_with('-')
        || name.contains(['\0', '\n', '\r', '~', '^', ':', '?', '*', '[', '\\', ' '])
        || name.contains("..")
        || name.contains("@{")
        || name.ends_with('/')
        || name.chars().any(char::is_control)
    {
        return Err("use HEAD, a full OID or a plain Git ref name (no revspec/options)".into());
    }
    Ok(())
}
struct OwnedRef<'a> {
    repo: &'a Repository,
    name: String,
    oid: String,
}
impl<'a> OwnedRef<'a> {
    fn new(repo: &'a Repository, oid: &str, control: &Control) -> Result<Self> {
        let name = format!("refs/vft/transfer/{}", unique_id()?);
        repo.git.run(
            &[
                "update-ref",
                "--no-deref",
                &name,
                oid,
                &"0".repeat(repo.format.oid_len()),
            ],
            control,
        )?;
        Ok(Self {
            repo,
            name,
            oid: oid.into(),
        })
    }
}
impl Drop for OwnedRef<'_> {
    fn drop(&mut self) {
        let _ = self.repo.git.run(
            &["update-ref", "--no-deref", "-d", &self.name, &self.oid],
            &Control::default(),
        );
    }
}
#[derive(Debug)]
pub enum Prepared {
    NoChange { head: String },
    Package { path: PathBuf, header: Header },
}
/// Pin target before walking history, and keep only complete packages. No source bytes are
/// loaded wholesale. An explicitly chosen package path is never overwritten.
pub fn prepare(
    repo: &Path,
    base: &str,
    target_ref: &str,
    package: Option<&Path>,
    control: &Control,
) -> Result<Prepared> {
    control.check()?;
    let repo = Repository::open(repo, control)?;
    repo.format.validate_oid(base)?;
    validate_ref(target_ref)?;
    if target_ref != "HEAD" && repo.format.validate_oid(target_ref).is_err() {
        let full_ref = repo.git.text(
            &[
                "rev-parse",
                "--symbolic-full-name",
                "--verify",
                "--end-of-options",
                target_ref,
            ],
            control,
        )?;
        if !full_ref.starts_with("refs/") {
            return Err("--git-ref must be HEAD, a named ref or a full object ID".into());
        }
    }
    let target = repo.oid(target_ref, control)?;
    let base_resolved = repo
        .oid(base, control)
        .map_err(|e| format!("unknown baseline: {e}"))?;
    if base_resolved != base {
        return Err("baseline must identify a commit directly".into());
    }
    repo.ancestor(base, &target, control)?;
    if base == target {
        return Ok(Prepared::NoChange { head: target });
    }
    let pinned = OwnedRef::new(&repo, &target, control)?;
    repo.supported_delta(base, &target, control)?;
    let scratch = TempDir::new("vft-git-work")?;
    let bundle = scratch.path().join("update.bundle");
    repo.git
        .raw(
            [
                std::ffi::OsStr::new("bundle"),
                "create".as_ref(),
                "--version=3".as_ref(),
                bundle.as_os_str(),
                pinned.name.as_ref(),
                format!("^{base}").as_ref(),
            ],
            control,
        )?
        .require()?;
    let mut input = File::open(&bundle)?;
    let len = input.metadata()?.len();
    let hash = copy_hash(&mut input, &mut std::io::sink(), len, control)?;
    let header = Header {
        format: repo.format,
        base: base.into(),
        target,
        bundle_len: len,
        bundle_sha256: hash,
    };
    let mut temp = if package.is_none() {
        Some(TempDir::new("vft-git")?)
    } else {
        None
    };
    let path = package
        .map(Path::to_path_buf)
        .unwrap_or_else(|| temp.as_ref().unwrap().path().join("update.vftgit"));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let write = (|| -> Result<()> {
        output.write_all(&header.encode()?)?;
        input.seek(SeekFrom::Start(0))?;
        let copied = copy_hash(&mut input, &mut output, len, control)?;
        if copied != hash {
            return Err("bundle changed during packaging".into());
        }
        output.sync_all()?;
        Ok(())
    })();
    drop(output);
    if let Err(e) = write {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    if let Some(temp) = &mut temp {
        temp.preserve();
    }
    Ok(Prepared::Package {
        path: path.canonicalize()?,
        header,
    })
}
#[derive(Debug)]
pub struct ApplyOutcome {
    pub target: String,
    pub already_current: bool,
    pub backup_ref: Option<String>,
}
pub struct ReceiveSession {
    repo: Repository,
    pub branch: String,
    pub head: String,
}
impl ReceiveSession {
    pub fn open(path: &Path, branch: Option<&str>, control: &Control) -> Result<Self> {
        let repo = Repository::open(path, control)?;
        let current = repo.branch(control)?;
        if let Some(branch) = branch {
            validate_ref(branch)?;
            let expected = if branch.starts_with("refs/heads/") {
                branch.into()
            } else {
                format!("refs/heads/{branch}")
            };
            if current != expected {
                return Err("--git-branch must be the currently checked-out branch; VFT never switches branches".into());
            }
        }
        let head = repo.oid("HEAD", control)?;
        repo.clean(control)?;
        repo.supported_tree(&head, true, control)?;
        Ok(Self {
            repo,
            branch: current,
            head,
        })
    }
    pub fn root(&self) -> &Path {
        &self.repo.root
    }
    fn unchanged(&self, control: &Control) -> Result<()> {
        if self.repo.branch(control)? != self.branch || self.repo.oid("HEAD", control)? != self.head
        {
            return Err(
                "receiving branch/HEAD changed since startup; package preserved for offline retry"
                    .into(),
            );
        }
        self.repo.clean(control)
    }
    pub fn apply(
        &self,
        package: &Path,
        limit: u64,
        control: &Control,
        mut progress: impl FnMut(&str),
    ) -> Result<ApplyOutcome> {
        let result = self.apply_inner(package, limit, control, &mut progress);
        result.map_err(|e| {
            format!(
                "file received but Git apply failed: {e}. Package retained: {}. Retry: {}",
                package.display(),
                retry_command(&self.repo.root, &self.branch, package)
            )
            .into()
        })
    }
    fn apply_inner(
        &self,
        package: &Path,
        limit: u64,
        control: &Control,
        progress: &mut impl FnMut(&str),
    ) -> Result<ApplyOutcome> {
        progress("Verifying Git package and bundle SHA-256");
        if !fs::metadata(package)?.is_file() {
            return Err("Git package must be a regular file".into());
        }
        let mut input = File::open(package)?;
        if !input.metadata()?.is_file() {
            return Err("Git package must be a regular file".into());
        }
        let mut bytes = [0; HEADER_LEN];
        input.read_exact(&mut bytes)?;
        let header = Header::decode(&bytes, input.metadata()?.len(), limit)?;
        if header.format != self.repo.format {
            return Err("package/repository object-format mismatch".into());
        }
        let scratch = TempDir::new("vft-git-work")?;
        let bundle = scratch.path().join("received.bundle");
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&bundle)?;
        if copy_hash(&mut input, &mut output, header.bundle_len, control)? != header.bundle_sha256 {
            return Err("bundle SHA-256 mismatch".into());
        }
        let mut tail = [0; 1];
        if input.read(&mut tail)? != 0 {
            return Err("trailing package bytes".into());
        }
        output.sync_all()?;
        drop(output);
        self.repo.validate(control)?;
        self.unchanged(control)?;
        if self.head != header.base && self.head != header.target {
            return Err("package baseline does not match receiving HEAD".into());
        }
        progress("Verifying Git bundle prerequisites");
        self.repo
            .git
            .raw(
                ["bundle".as_ref(), "verify".as_ref(), bundle.as_os_str()],
                control,
            )?
            .require()?;
        let heads = self
            .repo
            .git
            .raw(
                ["bundle".as_ref(), "list-heads".as_ref(), bundle.as_os_str()],
                control,
            )?
            .require()?;
        let heads = String::from_utf8(heads)?;
        let lines: Vec<_> = heads.lines().collect();
        if lines.len() != 1 || lines[0].split_once(' ').map(|(oid, _)| oid) != Some(&header.target)
        {
            return Err("bundle must advertise exactly the declared target commit".into());
        }
        progress("Importing Git objects (branch unchanged)");
        self.repo
            .git
            .raw(
                ["bundle".as_ref(), "unbundle".as_ref(), bundle.as_os_str()],
                control,
            )?
            .require()?;
        if self.repo.oid(&header.target, control)? != header.target {
            return Err("bundle target must be a commit".into());
        }
        let _imported = OwnedRef::new(&self.repo, &header.target, control)?;
        self.repo.validate(control)?;
        self.repo.ancestor(&header.base, &header.target, control)?;
        self.repo
            .supported_delta(&header.base, &header.target, control)?;
        self.repo.supported_tree(&header.target, true, control)?;
        self.unchanged(control)?;
        if self.head == header.target {
            return Ok(ApplyOutcome {
                target: header.target,
                already_current: true,
                backup_ref: None,
            });
        }
        let backup = format!("refs/vft/backups/{}", unique_id()?);
        self.repo.git.run(
            &[
                "update-ref",
                "--no-deref",
                &backup,
                &self.head,
                &"0".repeat(self.repo.format.oid_len()),
            ],
            control,
        )?;
        self.unchanged(control)?;
        control.check()?;
        progress(&format!(
            "Applying fast-forward; Stop waits for Git. Previous commit: {backup}"
        ));
        let updating = control.updating();
        // Explicit flags override local merge defaults. Hooks, filters, lazy fetching,
        // maintenance and submodule recursion cannot run during this operation.
        let merge = self.repo.git.run(
            &[
                "-c",
                &format!(
                    "branch.{}.mergeOptions=",
                    self.branch.strip_prefix("refs/heads/").unwrap()
                ),
                "merge",
                "--ff-only",
                "--no-overwrite-ignore",
                "--no-autostash",
                "--no-edit",
                "--no-stat",
                "--no-verify",
                "--no-gpg-sign",
                "--no-verify-signatures",
                "--",
                &header.target,
            ],
            &updating,
        );
        if let Err(error) = merge {
            let actual = self
                .repo
                .oid("HEAD", &updating)
                .unwrap_or_else(|e| format!("unknown ({e})"));
            return Err(format!("fast-forward failed: {error}; actual HEAD={actual}; previous commit retained as {backup}; inspect index/worktree, no automatic rollback was attempted").into());
        }
        if self.repo.branch(&updating)? != self.branch
            || self.repo.oid("HEAD", &updating)? != header.target
        {
            return Err(
                format!("final HEAD/branch mismatch; inspect repository; backup={backup}").into(),
            );
        }
        let expected = self.repo.git.text(
            &["rev-parse", &format!("{}^{{tree}}", header.target)],
            &updating,
        )?;
        let actual = self.repo.git.text(&["write-tree"], &updating)?;
        if actual != expected {
            return Err(format!("final index tree differs from target; backup={backup}").into());
        }
        self.repo.clean(&updating)?;
        progress("Git branch, index tree and worktree verified");
        Ok(ApplyOutcome {
            target: header.target,
            already_current: false,
            backup_ref: Some(backup),
        })
    }
}

fn retry_command(root: &Path, branch: &str, package: &Path) -> String {
    let quote = |value: &str| {
        #[cfg(windows)]
        let escaped = value.replace('\'', "''"); // PowerShell single-quoted literal.
        #[cfg(not(windows))]
        let escaped = value.replace('\'', "'\\''"); // POSIX shell single-quoted literal.
        format!("'{escaped}'")
    };
    format!(
        "vft-recv --git-repo {} --git-branch {} --git-apply {}",
        quote(&root.to_string_lossy()),
        quote(branch),
        quote(&package.to_string_lossy())
    )
}
