use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub path: PathBuf,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projects {
    pub projects: Vec<Project>,
}
struct Lock(fs::File);
impl Drop for Lock {
    fn drop(&mut self) {
        // SAFETY: the descriptor remains owned by this lock until after drop.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
impl Projects {
    pub fn path() -> PathBuf {
        crate::config::Config::path().with_file_name("projects.json")
    }
    pub fn load() -> Result<Self> {
        Self::load_at(&Self::path())
    }
    fn load_at(path: &Path) -> Result<Self> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let data: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("reading saved projects {}", path.display()))?;
        if data.projects.len() > 128 {
            bail!("project list exceeds 128 entries");
        }
        let mut ids = std::collections::BTreeSet::new();
        for project in &data.projects {
            if project.id == 0
                || !ids.insert(project.id)
                || project.name.trim().is_empty()
                || project.name.chars().count() > 80
                || project.name.chars().any(char::is_control)
                || !project.path.is_absolute()
            {
                bail!("invalid saved project entry");
            }
        }
        Ok(data)
    }
    fn lock(path: &Path) -> Result<Lock> {
        fs::create_dir_all(path.parent().context("missing project storage directory")?)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(path.with_extension("lock"))?;
        // SAFETY: flock receives a live descriptor; nonblocking avoids freezing the desktop.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!("projects are being updated by another session; try again");
        }
        Ok(Lock(file))
    }
    pub fn add(&mut self, path: &Path, name: Option<String>) -> Result<Project> {
        self.add_at(&Self::path(), path, name)
    }
    fn add_at(&mut self, store: &Path, path: &Path, name: Option<String>) -> Result<Project> {
        let path = fs::canonicalize(path)
            .with_context(|| format!("opening project directory {}", path.display()))?;
        if !path.is_dir() {
            bail!("project path is not a directory");
        }
        let name = name.unwrap_or_else(|| {
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned()
        });
        if name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control)
        {
            bail!("project name must be printable and at most 80 characters");
        }
        let _lock = Self::lock(store)?;
        let mut latest = Self::load_at(store)?;
        if let Some(existing) = latest.projects.iter().find(|p| p.path == path).cloned() {
            *self = latest;
            return Ok(existing);
        }
        if latest.projects.len() >= 128 {
            bail!("at most 128 saved projects are supported");
        }
        let id = latest
            .projects
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("project ID limit reached")?;
        let project = Project { id, name, path };
        latest.projects.push(project.clone());
        latest.save_at(store)?;
        *self = latest;
        Ok(project)
    }
    pub fn remove(&mut self, id: u64) -> Result<()> {
        let store = Self::path();
        let _lock = Self::lock(&store)?;
        let mut latest = Self::load_at(&store)?;
        if !latest.projects.iter().any(|p| p.id == id) {
            bail!("unknown project {id}");
        }
        latest.projects.retain(|p| p.id != id);
        latest.save_at(&store)?;
        *self = latest;
        Ok(())
    }
    fn save_at(&self, path: &Path) -> Result<()> {
        let temp = path.with_extension(format!("tmp-{}", std::process::id()));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    pub fn get(&self, value: &str) -> Result<&Project> {
        self.projects
            .iter()
            .find(|p| p.id.to_string() == value || p.name == value)
            .ok_or_else(|| anyhow::anyhow!("unknown project {value}"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projects_persist_deduplicate_and_merge_separate_sessions() {
        let root = std::env::temp_dir().join(format!("sx-projects-{}", std::process::id()));
        fs::create_dir_all(root.join("one")).unwrap();
        fs::create_dir_all(root.join("two")).unwrap();
        let store = root.join("projects.json");
        let mut first = Projects::default();
        let mut second = Projects::default();
        let one = first
            .add_at(&store, &root.join("one"), Some("One".into()))
            .unwrap();
        second.add_at(&store, &root.join("two"), None).unwrap();
        assert_eq!(
            first
                .add_at(&store, &root.join("one/../one"), None)
                .unwrap()
                .id,
            one.id
        );
        assert_eq!(Projects::load_at(&store).unwrap().projects.len(), 2);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&store).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }
}
