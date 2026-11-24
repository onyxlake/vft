//! Shared Git-only switches; ordinary file mode never starts Git.
use crate::{prepare, Control, Prepared, ReceiveSession, Result};
use std::path::{Path, PathBuf};
pub const SEND_HELP: &str = "Git: --git (repository containing current directory) or --git-repo PATH\n  --git-base FULL_OID [--git-ref HEAD] [--git-package FILE] [--git-prepare-only]\n  Explicit --git-repo wins. Packages default to a private system temporary directory\n  and remain available after sending; prepare-only prints the path without opening a window.";
pub const RECV_HELP: &str = "Git: --git or --git-repo PATH [--git-branch CURRENT_BRANCH]\n  Receive with --auto/--rect, or retry offline with --git-apply FILE (no capture).\n  With no explicit output or output-dir, save in a private system temporary directory.\n  Only a successful repository update completes Git mode; complete packages are retained.";
#[derive(Default, Debug)]
pub struct SenderOptions {
    pub repo: Option<PathBuf>,
    pub base: Option<String>,
    pub target: Option<String>,
    pub package: Option<PathBuf>,
    pub prepare_only: bool,
}
#[derive(Default, Debug)]
pub struct ReceiverOptions {
    pub repo: Option<PathBuf>,
    pub branch: Option<String>,
    pub apply: Option<PathBuf>,
}
type Extracted = (Vec<(String, String)>, Vec<String>);
fn extract(args: &[String], sender: bool) -> Result<Extracted> {
    let mut values = Vec::new();
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let flag = matches!(arg.as_str(), "--git" | "--git-prepare-only");
        let recognized = matches!(arg.as_str(), "--git" | "--git-repo")
            || if sender {
                matches!(
                    arg.as_str(),
                    "--git-base" | "--git-ref" | "--git-package" | "--git-prepare-only"
                )
            } else {
                matches!(arg.as_str(), "--git-branch" | "--git-apply")
            };
        if recognized {
            if values.iter().any(|(k, _)| k == arg) {
                return Err(format!("duplicate {arg}").into());
            }
            let value = if flag {
                String::new()
            } else {
                let value = iter
                    .next()
                    .ok_or_else(|| format!("missing value for {arg}"))?;
                if value.is_empty() || value.starts_with("--") {
                    return Err(format!("missing/invalid value for {arg}").into());
                }
                value.clone()
            };
            values.push((arg.clone(), value));
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((values, rest))
}
impl SenderOptions {
    pub fn extract(args: &[String]) -> Result<(Self, Vec<String>)> {
        let (values, rest) = extract(args, true)?;
        let mut out = Self::default();
        let mut enabled = false;
        for (key, value) in &values {
            match key.as_str() {
                "--git" => enabled = true,
                "--git-repo" => out.repo = Some(value.into()),
                "--git-base" => out.base = Some(value.clone()),
                "--git-ref" => out.target = Some(value.clone()),
                "--git-package" => out.package = Some(value.into()),
                "--git-prepare-only" => out.prepare_only = true,
                _ => unreachable!(),
            }
        }
        if out.repo.is_none() && enabled {
            out.repo = Some(".".into());
        }
        if out.repo.is_none() && !values.is_empty() {
            return Err("Git options require --git or --git-repo PATH".into());
        }
        if out.repo.is_some() && out.base.is_none() {
            return Err("Git sending requires --git-base with the receiver's full HEAD".into());
        }
        Ok((out, rest))
    }
    pub fn validate_source(&self, file: bool, pattern: bool) -> Result<()> {
        if self.repo.is_some() && (file || pattern) {
            return Err("Git sending cannot be combined with FILE or --test-pattern".into());
        }
        Ok(())
    }
    pub fn prepare(&self, control: &Control) -> Result<Prepared> {
        prepare(
            self.repo.as_deref().ok_or("Git mode is disabled")?,
            self.base.as_deref().ok_or("missing --git-base")?,
            self.target.as_deref().unwrap_or("HEAD"),
            self.package.as_deref(),
            control,
        )
    }
}
impl ReceiverOptions {
    pub fn extract(args: &[String]) -> Result<(Self, Vec<String>)> {
        let (values, rest) = extract(args, false)?;
        let mut out = Self::default();
        let mut enabled = false;
        for (key, value) in &values {
            match key.as_str() {
                "--git" => enabled = true,
                "--git-repo" => out.repo = Some(value.into()),
                "--git-branch" => out.branch = Some(value.clone()),
                "--git-apply" => out.apply = Some(value.into()),
                _ => unreachable!(),
            }
        }
        if out.repo.is_none() && enabled {
            out.repo = Some(".".into());
        }
        if out.repo.is_none() && !values.is_empty() {
            return Err("Git options require --git or --git-repo PATH".into());
        }
        Ok((out, rest))
    }
    pub fn open(&self, control: &Control) -> Result<Option<ReceiveSession>> {
        self.repo
            .as_deref()
            .map(|path| ReceiveSession::open(path, self.branch.as_deref(), control))
            .transpose()
    }
    pub fn is_offline(&self) -> bool {
        self.apply.is_some()
    }
    pub fn apply_path(&self) -> Option<&Path> {
        self.apply.as_deref()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|v| (*v).into()).collect()
    }
    #[test]
    fn git_flag_explicit_override_and_strict_arguments() {
        for a in [
            vec!["--git", "--git-repo", "repo", "--git-base", "abc"],
            vec!["--git-repo", "repo", "--git", "--git-base", "abc"],
        ] {
            assert_eq!(
                SenderOptions::extract(&args(&a)).unwrap().0.repo.unwrap(),
                PathBuf::from("repo")
            );
        }
        assert_eq!(
            ReceiverOptions::extract(&args(&["--git"]))
                .unwrap()
                .0
                .repo
                .unwrap(),
            PathBuf::from(".")
        );
        for a in [
            vec!["--git"],
            vec!["--git-base", "abc"],
            vec!["--git", "--git-base"],
            vec!["--git", "--git", "--git-base", "abc"],
        ] {
            assert!(SenderOptions::extract(&args(&a)).is_err());
        }
        assert!(ReceiverOptions::extract(&args(&["--git-apply", "a"])).is_err());
    }
}
