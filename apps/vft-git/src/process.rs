use crate::{Result, TempDir};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
const OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;
const ERROR_LIMIT: u64 = 64 * 1024;

/// Keep canonical paths for Rust filesystem operations, but pass ordinary drive/UNC
/// paths to Git: its lockfile code rejects Windows verbatim prefixes after slash conversion.
pub(crate) fn path_arg(path: &Path) -> OsString {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            let mut argument = match prefix.kind() {
                Prefix::VerbatimDisk(drive) => OsString::from(format!("{}:", char::from(drive))),
                Prefix::VerbatimUNC(server, share) => {
                    let mut argument = OsString::from(r"\\");
                    argument.push(server);
                    argument.push(r"\");
                    argument.push(share);
                    argument
                }
                _ => return path.as_os_str().to_owned(),
            };
            argument.push(components.as_path());
            return argument;
        }
    }
    path.as_os_str().to_owned()
}

#[derive(Clone, Debug)]
pub struct Control {
    pub cancel: Arc<AtomicBool>,
    pub timeout: Duration,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            timeout: Duration::from_secs(120),
        }
    }
}
impl Control {
    pub fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            Err("Git operation cancelled before branch update".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn updating(&self) -> Self {
        // Once merge begins, a UI Stop waits for it to finish. A hard deadline still
        // stops a stuck child; callers must inspect HEAD/index, never reset blindly.
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            timeout: self.timeout,
        }
    }
}
pub(crate) struct Git {
    cwd: PathBuf,
    scratch: TempDir,
    filters: Vec<String>,
}
pub(crate) struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: String,
}
impl Output {
    pub fn require(self) -> Result<Vec<u8>> {
        if !self.status.success() {
            return Err(format!(
                "Git exited {}: {} {}",
                self.status,
                self.stderr,
                String::from_utf8_lossy(&self.stdout)
                    .chars()
                    .take(4096)
                    .collect::<String>()
            )
            .into());
        }
        Ok(self.stdout)
    }
}
impl Git {
    pub fn new(cwd: &Path, control: &Control) -> Result<Self> {
        let scratch = TempDir::new("vft-git-work")?;
        std::fs::create_dir(scratch.path().join("hooks"))?;
        let mut git = Self {
            cwd: cwd.canonicalize()?,
            scratch,
            filters: Vec::new(),
        };
        // Reading config cannot execute a filter. Disable every configured driver before
        // any command that can inspect working-tree contents, including status.
        git.filters = git.filter_names(control)?;
        Ok(git)
    }
    fn filter_names(&self, control: &Control) -> Result<Vec<String>> {
        let output = self.raw(
            [
                "config",
                "--name-only",
                "--null",
                "--get-regexp",
                "^filter\\..*\\.(clean|smudge|process|required)$",
            ],
            control,
        )?;
        if output.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        let keys = output.require()?;
        let mut names = std::collections::BTreeSet::new();
        for key in keys.split(|b| *b == 0).filter(|key| !key.is_empty()) {
            let key = std::str::from_utf8(key)?;
            names.insert(
                key.rsplit_once('.')
                    .ok_or("invalid filter config key")?
                    .0
                    .to_owned(),
            );
        }
        if names.len() > 128 || names.iter().map(String::len).sum::<usize>() > 4096 {
            return Err("configured filter names exceed Git adapter resource limits".into());
        }
        Ok(names.into_iter().collect())
    }
    pub fn validate_filters(&self, control: &Control) -> Result<()> {
        if self.filter_names(control)? != self.filters {
            return Err(
                "Git filter configuration changed during this operation; retry with a new session"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn with_input(&self, args: &[&str], input: &[u8], control: &Control) -> Result<Vec<u8>> {
        let path = self
            .scratch
            .path()
            .join(format!("{}.input", crate::temporary::unique_id()?));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(input)?;
        drop(file);
        let result = self.execute(args, control, Some(File::open(&path)?));
        let _ = std::fs::remove_file(path);
        result?.require()
    }
    pub fn run(&self, args: &[&str], control: &Control) -> Result<Vec<u8>> {
        self.raw(args.iter().map(OsStr::new), control)?.require()
    }
    pub fn text(&self, args: &[&str], control: &Control) -> Result<String> {
        Ok(String::from_utf8(self.run(args, control)?)?
            .trim_end_matches(['\r', '\n'])
            .to_owned())
    }
    pub fn raw(
        &self,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
        control: &Control,
    ) -> Result<Output> {
        self.execute(args, control, None)
    }
    fn execute(
        &self,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
        control: &Control,
        input: Option<File>,
    ) -> Result<Output> {
        control.check()?;
        let mut command = Command::new("git");
        command.current_dir(&self.cwd).arg("--no-pager");
        // Do not inherit repository overrides, injected -c settings, tracing destinations,
        // external object directories or credentials from a parent Git invocation.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_ALLOW_PROTOCOL", "")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C");
        for config in [
            "protocol.allow=never",
            "maintenance.auto=false",
            "gc.auto=0",
            "gc.autoDetach=false",
            "fetch.writeCommitGraph=false",
            "core.fsmonitor=false",
            "core.untrackedCache=false",
            "submodule.recurse=false",
            "fetch.recurseSubmodules=false",
            "merge.autoStash=false",
            "merge.verifySignatures=false",
            "core.pager=",
            "color.ui=false",
            "transfer.fsckObjects=true",
            "fetch.fsckObjects=true",
            "pack.threads=1",
            "pack.windowMemory=64m",
        ] {
            command.arg("-c").arg(config);
        }
        command.env("GIT_CONFIG_COUNT", (self.filters.len() * 4).to_string());
        for (index, (key, value)) in self
            .filters
            .iter()
            .flat_map(|prefix| {
                [
                    ("clean", ""),
                    ("smudge", ""),
                    ("process", ""),
                    ("required", "false"),
                ]
                .map(|(field, value)| (format!("{prefix}.{field}"), value))
            })
            .enumerate()
        {
            command
                .env(format!("GIT_CONFIG_KEY_{index}"), key)
                .env(format!("GIT_CONFIG_VALUE_{index}"), value);
        }
        let mut hooks = OsString::from("core.hooksPath=");
        hooks.push(path_arg(&self.scratch.path().join("hooks")));
        command
            .arg("-c")
            .arg(hooks)
            .args(args)
            .stdin(input.map_or_else(Stdio::null, Stdio::from));
        let id = crate::temporary::unique_id()?;
        let out_path = self.scratch.path().join(format!("{id}.out"));
        let err_path = self.scratch.path().join(format!("{id}.err"));
        let out = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&out_path)?;
        let err = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&err_path)?;
        command.stdout(Stdio::from(out)).stderr(Stdio::from(err));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        } // CREATE_NO_WINDOW
        let child = command
            .spawn()
            .map_err(|e| format!("cannot start Git (install Git 2.48+ for Git mode): {e}"))?;
        let mut child = ManagedChild::new(child)?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.child.try_wait()? {
                break status;
            }
            control.check()?;
            if start.elapsed() >= control.timeout {
                return Err(
                    "Git command timed out; inspect repository state before retrying".into(),
                );
            }
            if std::fs::metadata(&out_path)?.len() > OUTPUT_LIMIT
                || std::fs::metadata(&err_path)?.len() > ERROR_LIMIT
            {
                return Err("Git command exceeded bounded output limit".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let read = |path: &Path, limit| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > limit {
                return Err("Git command exceeded bounded output limit".into());
            }
            Ok(bytes)
        };
        let stdout = read(&out_path, OUTPUT_LIMIT)?;
        let stderr = String::from_utf8_lossy(&read(&err_path, ERROR_LIMIT)?)
            .chars()
            .take(4096)
            .collect();
        std::fs::remove_file(out_path)?;
        std::fs::remove_file(err_path)?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
}
struct ManagedChild {
    child: Child,
    #[cfg(windows)]
    job: windows::Win32::Foundation::HANDLE,
}
impl ManagedChild {
    fn new(child: Child) -> Result<Self> {
        #[cfg(windows)]
        {
            use std::{mem::size_of, os::windows::io::AsRawHandle};
            use windows::Win32::{Foundation::HANDLE, System::JobObjects::*};
            let mut guard = Self {
                child,
                job: HANDLE::default(),
            };
            // Own a Job handle and assign only our child. Closing kills its helper tree.
            unsafe {
                guard.job = CreateJobObjectW(None, None)?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    guard.job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )?;
                AssignProcessToJobObject(guard.job, HANDLE(guard.child.as_raw_handle()))?;
            }
            Ok(guard)
        }
        #[cfg(not(windows))]
        {
            Ok(Self { child })
        }
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            #[cfg(unix)]
            {
                // Child is still owned/unreaped and started in its own process group.
                // Negative PID targets only this operation and its Git helpers.
                unsafe {
                    libc::kill(-(self.child.id() as i32), libc::SIGKILL);
                }
            }
            let _ = self.child.kill();
        }
        #[cfg(windows)]
        {
            if !self.job.is_invalid() {
                // This struct exclusively owns the job handle.
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(self.job);
                }
            }
        }
        let _ = self.child.wait();
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn git_path_arguments_handle_verbatim_drive_and_unc_paths() {
        for (input, expected) in [
            (r"\\?\C:\", r"C:\"),
            (
                r"\\?\C:\work space\文件\update.bundle",
                r"C:\work space\文件\update.bundle",
            ),
            (r"\\?\UNC\server\share", r"\\server\share"),
            (r"\\?\UNC\server\share\", r"\\server\share\"),
            (
                r"\\?\UNC\server\share\work space\update.bundle",
                r"\\server\share\work space\update.bundle",
            ),
            (
                r"C:\work space\update.bundle",
                r"C:\work space\update.bundle",
            ),
            (
                r"\\server\share\update.bundle",
                r"\\server\share\update.bundle",
            ),
            (r"relative\update.bundle", r"relative\update.bundle"),
        ] {
            assert_eq!(path_arg(Path::new(input)), OsStr::new(expected), "{input}");
        }
    }

    #[test]
    fn git_path_arguments_preserve_non_unicode_names() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let mut input: Vec<_> = r"\\?\C:\work\".encode_utf16().collect();
        input.push(0xd800);
        let path = PathBuf::from(OsString::from_wide(&input));
        assert_eq!(
            path_arg(&path).encode_wide().collect::<Vec<_>>(),
            input[4..]
        );
    }
}
