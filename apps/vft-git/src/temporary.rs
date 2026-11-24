use crate::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn unique_id() -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("OS random generator: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Owns only a newly created private directory. Persisted artifacts outlive the operation.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
    keep: bool,
}
impl TempDir {
    pub fn new(prefix: &str) -> Result<Self> {
        Self::in_directory(&std::env::temp_dir(), prefix)
    }
    pub fn in_directory(parent: &Path, prefix: &str) -> Result<Self> {
        Self::allocate(parent, prefix, unique_id)
    }
    fn allocate(
        parent: &Path,
        prefix: &str,
        mut nonce: impl FnMut() -> Result<String>,
    ) -> Result<Self> {
        if !matches!(prefix, "vft-git" | "vft-git-recv" | "vft-git-work") {
            return Err("invalid temporary directory prefix".into());
        }
        let parent = parent.canonicalize()?;
        for _ in 0..32 {
            let path = parent.join(format!("{prefix}-{}", nonce()?));
            #[cfg(unix)]
            let mut builder = fs::DirBuilder::new();
            #[cfg(not(unix))]
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path, keep: false }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(format!(
                        "cannot create temporary directory in {}: {e}",
                        parent.display()
                    )
                    .into())
                }
            }
        }
        Err("temporary directory name collisions exhausted".into())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn preserve(&mut self) {
        self.keep = true;
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.keep {
            // The directory was exclusively created above, with an absolute canonical parent.
            // Do not follow a replacement symlink or recursively delete a different path.
            if self.path.canonicalize().ok().as_ref() == Some(&self.path) {
                let _ = fs::remove_dir_all(&self.path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collisions_cleanup_and_unwritable_destination() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../output/git-temp-tests");
        fs::create_dir_all(&root).unwrap();
        let parent = TempDir::in_directory(&root, "vft-git-work").unwrap();
        let existing = parent.path().join("vft-git-collision");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("keep"), b"keep").unwrap();
        let mut count = 0;
        let created = TempDir::allocate(parent.path(), "vft-git", || {
            count += 1;
            Ok(if count == 1 { "collision" } else { "fresh" }.into())
        })
        .unwrap();
        let created_path = created.path().to_owned();
        drop(created);
        assert!(!created_path.exists());
        assert_eq!(fs::read(existing.join("keep")).unwrap(), b"keep");
        assert!(TempDir::allocate(parent.path(), "vft-git", || Ok("collision".into())).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o500)).unwrap();
            let result = TempDir::in_directory(parent.path(), "vft-git");
            fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
            assert!(result.is_err());
        }
    }
}
