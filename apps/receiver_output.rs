//! Shared std-only destination handling for the native receiver adapters.
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

pub struct OutputDestination {
    pub file: Option<PathBuf>,
    pub directory: PathBuf,
    resolved: Option<PathBuf>,
}

impl Default for OutputDestination {
    fn default() -> Self {
        Self {
            file: None,
            directory: PathBuf::from("."),
            resolved: None,
        }
    }
}

impl OutputDestination {
    #[cfg(test)]
    pub fn explicit(file: PathBuf) -> Self {
        Self {
            file: Some(file),
            ..Default::default()
        }
    }

    /// Validate CLI paths before opening a capture session. An explicit file wins
    /// over the directory, including when that unused directory does not exist.
    pub fn validate(&mut self, preview: Option<&Path>, stats: Option<&Path>) -> io::Result<()> {
        if self.file.is_none() {
            if self.directory.as_os_str().is_empty() || !self.directory.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--output-dir must be an existing directory",
                ));
            }
            self.directory = self.directory.canonicalize()?;
        }
        let paths: Vec<_> = self
            .file
            .as_deref()
            .into_iter()
            .chain(preview)
            .chain(stats)
            .collect();
        for path in &paths {
            require_new(path)?;
        }
        require_distinct(&paths)
    }

    /// Resolve once from the accepted manifest, without creating the output.
    /// Diagnostic files may already exist by this point; only the output must
    /// still be new. The final writer must also use create_new to close races.
    pub fn resolve(
        &mut self,
        filename: &str,
        preview: Option<&Path>,
        stats: Option<&Path>,
    ) -> io::Result<&Path> {
        if self.resolved.is_none() {
            let path = if let Some(file) = &self.file {
                file.clone()
            } else {
                validate_filename(filename)?;
                self.directory.join(filename)
            };
            require_distinct(
                &[Some(path.as_path()), preview, stats]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>(),
            )?;
            require_new(&path)?;
            self.resolved = Some(path);
        }
        Ok(self.resolved.as_deref().expect("output was resolved"))
    }

    pub fn path(&self) -> Option<&Path> {
        self.resolved.as_deref()
    }
}

fn require_new(path: &Path) -> io::Result<()> {
    // Unlike exists(), this also rejects dangling symlinks and propagates errors.
    match fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("refusing to overwrite {}", path.display()),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn require_distinct(paths: &[&Path]) -> io::Result<()> {
    let mut resolved = Vec::new();
    for path in paths {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let filename = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "output path needs a filename")
        })?;
        let canonical = parent.canonicalize()?.join(filename);
        // Windows canonicalizes the existing parents, but the new leaf can still
        // differ in case. Conservatively reject case aliases on that platform.
        #[cfg(windows)]
        let canonical = canonical
            .to_string_lossy()
            .trim_end_matches(['.', ' '])
            .to_lowercase();
        if resolved.contains(&canonical) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output, preview and stats paths must differ",
            ));
        }
        resolved.push(canonical);
    }
    Ok(())
}

fn validate_filename(filename: &str) -> io::Result<()> {
    let mut components = Path::new(filename).components();
    let invalid = filename.is_empty()
        || filename.len() > 255
        || filename.contains(['\0', '/', '\\', ':'])
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some();
    #[cfg(windows)]
    let invalid = invalid
        || filename.chars().any(|c| c < ' ' || "<>\"|?*".contains(c))
        || filename.ends_with(['.', ' '])
        || {
            let stem = filename
                .split('.')
                .next()
                .unwrap_or("")
                .trim_end_matches(' ')
                .to_ascii_uppercase();
            matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ) || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|n| {
                    matches!(
                        n,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            })
        };
    if invalid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "metadata filename is unsafe on this platform; choose --output FILE",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_directory() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../output/receiver-output-tests");
    fs::create_dir_all(&root).unwrap();
    let path = root.join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path.canonicalize().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::OpenOptions,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn defaults_and_directory_use_metadata_while_explicit_output_wins() {
        let name = format!(
            "vft-metadata-{}.bin",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let mut output = OutputDestination::default();
        output.validate(None, None).unwrap();
        assert_eq!(
            output.resolve(&name, None, None).unwrap(),
            std::env::current_dir()
                .unwrap()
                .canonicalize()
                .unwrap()
                .join(&name)
        );
        assert!(!output.path().unwrap().exists());
        let dir = test_directory();
        let mut output = OutputDestination {
            directory: dir.clone(),
            ..Default::default()
        };
        output.validate(None, None).unwrap();
        assert_eq!(
            output.resolve("文件 name.bin", None, None).unwrap(),
            dir.join("文件 name.bin")
        );
        let file = dir.join("override.bin");
        let mut output = OutputDestination::explicit(file.clone());
        output.directory = dir.join("missing");
        output.validate(None, None).unwrap();
        assert_eq!(
            output.resolve("../unsafe metadata", None, None).unwrap(),
            file
        );
    }

    #[test]
    fn invalid_directories_and_unsafe_names_are_rejected_without_writes() {
        let dir = test_directory();
        for path in [PathBuf::new(), dir.join("missing")] {
            assert!(OutputDestination {
                directory: path,
                ..Default::default()
            }
            .validate(None, None)
            .is_err());
        }
        let mut output = OutputDestination {
            directory: dir.clone(),
            ..Default::default()
        };
        output.validate(None, None).unwrap();
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "/absolute",
            "nested/file",
            "nested\\file",
            "C:drive",
            "a\0b",
        ] {
            assert!(output.resolve(name, None, None).is_err(), "{name:?}");
        }
        assert!(fs::read_dir(dir).unwrap().next().is_none());
    }

    #[test]
    fn existing_files_and_alias_collisions_are_rejected_without_overwriting() {
        let dir = test_directory();
        let file = dir.join("received.bin");
        let mut output = OutputDestination {
            directory: dir.clone(),
            ..Default::default()
        };
        output.validate(None, None).unwrap();
        for diagnostic in [file.clone(), dir.join("./received.bin")] {
            assert!(output
                .resolve("received.bin", Some(&diagnostic), None)
                .is_err());
            assert!(output
                .resolve("received.bin", None, Some(&diagnostic))
                .is_err());
        }
        output.resolve("received.bin", None, None).unwrap();
        let mut handle = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .unwrap();
        handle.write_all(b"existing bytes").unwrap();
        drop(handle);
        let mut another = OutputDestination {
            directory: dir,
            ..Default::default()
        };
        another.validate(None, None).unwrap();
        assert!(another.resolve("received.bin", None, None).is_err());
        assert_eq!(fs::read(file).unwrap(), b"existing bytes");
    }

    #[cfg(windows)]
    #[test]
    fn windows_device_names_and_case_aliases_are_rejected() {
        for name in [
            "CON",
            "nul.txt",
            "COM1.bin",
            "LPT²",
            "photo.png.",
            "photo.png ",
            "a*b",
            "a?b",
            "a<b",
            "a\nb",
        ] {
            assert!(validate_filename(name).is_err(), "{name:?}");
        }
        let dir = test_directory();
        let mut output = OutputDestination {
            directory: dir.clone(),
            ..Default::default()
        };
        output.validate(None, None).unwrap();
        assert!(output
            .resolve("photo.png", Some(&dir.join("PHOTO.PNG")), None)
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parent_aliases_and_dangling_destinations_are_rejected() {
        let dir = test_directory();
        std::os::unix::fs::symlink(&dir, dir.join("alias")).unwrap();
        let mut output = OutputDestination {
            directory: dir.clone(),
            ..Default::default()
        };
        output.validate(None, None).unwrap();
        assert!(output
            .resolve("photo.png", Some(&dir.join("alias/photo.png")), None)
            .is_err());
        std::os::unix::fs::symlink(dir.join("missing"), dir.join("dangling")).unwrap();
        assert!(output.resolve("dangling", None, None).is_err());
    }
}
