use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Returns (success_count, error_messages).
pub fn copy_entries(sources: &[PathBuf], dest: &Path) -> (usize, Vec<String>) {
    let mut ok = 0;
    let mut errors = vec![];
    for src in sources {
        let Some(name) = src.file_name() else {
            errors.push(format!("no filename: {}", src.display()));
            continue;
        };
        let target = dest.join(name);
        let result = if src.is_dir() {
            copy_dir(src, &target)
        } else {
            fs::copy(src, &target)
                .map(|_| ())
                .map_err(anyhow::Error::from)
        };
        match result {
            Ok(()) => ok += 1,
            Err(e) => errors.push(format!("{}: {e}", src.display())),
        }
    }
    (ok, errors)
}

fn copy_dir(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Returns (success_count, error_messages).
pub fn move_entries(sources: &[PathBuf], dest: &Path) -> (usize, Vec<String>) {
    let mut ok = 0;
    let mut errors = vec![];
    for src in sources {
        let Some(name) = src.file_name() else {
            errors.push(format!("no filename: {}", src.display()));
            continue;
        };
        let target = dest.join(name);
        let result: Result<()> = fs::rename(src, &target).or_else(|_| {
            if src.is_dir() {
                copy_dir(src, &target)?;
                fs::remove_dir_all(src)?;
            } else {
                fs::copy(src, &target)?;
                fs::remove_file(src)?;
            }
            Ok(())
        });
        match result {
            Ok(()) => ok += 1,
            Err(e) => errors.push(format!("{}: {e}", src.display())),
        }
    }
    (ok, errors)
}

/// Returns (success_count, error_messages).
pub fn delete_entries(sources: &[PathBuf]) -> (usize, Vec<String>) {
    let mut ok = 0;
    let mut errors = vec![];
    for src in sources {
        let result = if src.is_dir() {
            fs::remove_dir_all(src).map_err(anyhow::Error::from)
        } else {
            fs::remove_file(src).map_err(anyhow::Error::from)
        };
        match result {
            Ok(()) => ok += 1,
            Err(e) => errors.push(format!("{}: {e}", src.display())),
        }
    }
    (ok, errors)
}

pub fn mkdir(parent: &Path, name: &str) -> Result<()> {
    fs::create_dir(parent.join(name))?;
    Ok(())
}

pub fn rename(path: &Path, new_name: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent directory"))?;
    fs::rename(path, parent.join(new_name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_move_delete_mkdir_rename() {
        let root = std::env::temp_dir().join(format!("sx-ops-{}", std::process::id()));
        let src_dir = root.join("src");
        let dst_dir = root.join("dst");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&dst_dir).unwrap();

        // copy file
        fs::write(src_dir.join("a.txt"), b"hello").unwrap();
        let (ok, errs) = copy_entries(&[src_dir.join("a.txt")], &dst_dir);
        assert_eq!(ok, 1);
        assert!(errs.is_empty());
        assert_eq!(fs::read(dst_dir.join("a.txt")).unwrap(), b"hello");

        // move file
        fs::write(src_dir.join("b.txt"), b"world").unwrap();
        let (ok, errs) = move_entries(&[src_dir.join("b.txt")], &dst_dir);
        assert_eq!(ok, 1);
        assert!(errs.is_empty());
        assert!(!src_dir.join("b.txt").exists());
        assert!(dst_dir.join("b.txt").exists());

        // delete file
        let (ok, errs) = delete_entries(&[dst_dir.join("a.txt")]);
        assert_eq!(ok, 1);
        assert!(errs.is_empty());
        assert!(!dst_dir.join("a.txt").exists());

        // mkdir
        mkdir(&root, "newdir").unwrap();
        assert!(root.join("newdir").is_dir());

        // rename
        rename(&dst_dir.join("b.txt"), "c.txt").unwrap();
        assert!(!dst_dir.join("b.txt").exists());
        assert!(dst_dir.join("c.txt").exists());

        fs::remove_dir_all(root).unwrap();
    }
}
