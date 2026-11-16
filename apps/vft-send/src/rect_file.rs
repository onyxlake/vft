//! Reserve telemetry once; repainting must never reopen or truncate an unrelated path.
use std::{
    fs::{File, OpenOptions},
    io::{self, Seek, Write},
    path::Path,
};

pub struct RectFile {
    file: File,
    last: Option<[i32; 4]>,
}

impl RectFile {
    pub fn create(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: OpenOptions::new().write(true).create_new(true).open(path)?,
            last: None,
        })
    }

    pub fn update(&mut self, rect: [i32; 4]) -> io::Result<()> {
        if self.last == Some(rect) {
            return Ok(());
        }
        let [x, y, width, height] = rect;
        let text = format!("{x} {y} {width} {height}\n");
        self.file.rewind()?;
        self.file.write_all(text.as_bytes())?;
        self.file.set_len(text.len() as u64)?;
        self.file.flush()?;
        self.last = Some(rect);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf, time::SystemTime};

    fn directory() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../output/rect-file-tests");
        fs::create_dir_all(&root).unwrap();
        let directory = root.join(format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn refuses_existing_input_and_hard_link_without_changing_bytes() {
        let dir = directory();
        let input = dir.join("input.bin");
        fs::write(&input, b"keep the original input").unwrap();
        let _source = File::open(&input).unwrap();
        let alias = dir.join("alias.bin");
        fs::hard_link(&input, &alias).unwrap();
        for path in [&input, &alias] {
            assert_eq!(
                RectFile::create(path).err().unwrap().kind(),
                io::ErrorKind::AlreadyExists
            );
            assert_eq!(fs::read(path).unwrap(), b"keep the original input");
        }
    }

    #[test]
    fn reserves_new_file_and_replaces_longer_coordinates_without_stale_bytes() {
        let path = directory().join("region.txt");
        let mut telemetry = RectFile::create(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"");
        assert_eq!(
            RectFile::create(&path).err().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        telemetry.update([-2000, -1000, 1536, 864]).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"-2000 -1000 1536 864\n");
        telemetry.update([0, 0, 384, 216]).unwrap();
        telemetry.update([0, 0, 384, 216]).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"0 0 384 216\n");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_and_keeps_writes_on_the_reserved_file_after_path_replacement() {
        let dir = directory();
        let path = dir.join("region.txt");
        let original = dir.join("original.txt");
        let replacement = dir.join("input.bin");
        fs::write(&replacement, b"unrelated input").unwrap();
        std::os::unix::fs::symlink(&replacement, &path).unwrap();
        assert_eq!(
            RectFile::create(&path).err().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        fs::remove_file(&path).unwrap();
        let mut telemetry = RectFile::create(&path).unwrap();
        telemetry.update([0, 0, 384, 216]).unwrap();
        fs::rename(&path, &original).unwrap();
        std::os::unix::fs::symlink(&replacement, &path).unwrap();
        telemetry.update([40, 60, 768, 432]).unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"40 60 768 432\n");
        assert_eq!(fs::read(&replacement).unwrap(), b"unrelated input");
    }
}
