//! Recently opened project folders, newest first, for the welcome screen.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{CoreError, Result};

/// How many folders are remembered.
pub const MAX_RECENT: usize = 12;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProjects {
    #[serde(default)]
    pub folders: Vec<PathBuf>,
}

impl RecentProjects {
    pub fn load(path: &Path) -> Result<RecentProjects> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| CoreError::Config(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RecentProjects::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| CoreError::Config(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Moves `dir` to the front, dropping the oldest past [`MAX_RECENT`].
    pub fn touch(&mut self, dir: PathBuf) {
        self.folders.retain(|f| *f != dir);
        self.folders.insert(0, dir);
        self.folders.truncate(MAX_RECENT);
    }

    pub fn remove(&mut self, dir: &Path) {
        self.folders.retain(|f| f != dir);
    }

    /// Folders that still exist on disk.
    pub fn existing(&self) -> Vec<PathBuf> {
        self.folders
            .iter()
            .filter(|f| f.is_dir())
            .cloned()
            .collect()
    }
}

/// `path` with the home folder shown as `~`.
pub fn tildify(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_RECENT, RecentProjects, tildify};
    use std::path::{Path, PathBuf};

    #[test]
    fn touch_moves_to_front_without_duplicates() {
        let mut r = RecentProjects::default();
        r.touch("/a".into());
        r.touch("/b".into());
        r.touch("/a".into());
        assert_eq!(r.folders, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn keeps_only_the_newest() {
        let mut r = RecentProjects::default();
        for i in 0..MAX_RECENT + 3 {
            r.touch(format!("/p{i}").into());
        }
        assert_eq!(r.folders.len(), MAX_RECENT);
        assert_eq!(r.folders[0], PathBuf::from(format!("/p{}", MAX_RECENT + 2)));
    }

    #[test]
    fn roundtrip_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/recent.toml");
        assert_eq!(
            RecentProjects::load(&file).unwrap(),
            RecentProjects::default()
        );
        let mut r = RecentProjects::default();
        r.touch(dir.path().to_path_buf());
        r.touch("/does/not/exist".into());
        r.save(&file).unwrap();
        let loaded = RecentProjects::load(&file).unwrap();
        assert_eq!(loaded, r);
        assert_eq!(loaded.existing(), vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn remove_forgets_a_folder() {
        let mut r = RecentProjects::default();
        r.touch("/a".into());
        r.touch("/b".into());
        r.remove(Path::new("/a"));
        assert_eq!(r.folders, vec![PathBuf::from("/b")]);
    }

    #[test]
    fn home_is_shown_as_tilde() {
        let home = Some(Path::new("/Users/me"));
        assert_eq!(tildify(Path::new("/Users/me/code/x"), home), "~/code/x");
        assert_eq!(tildify(Path::new("/Users/me"), home), "~");
        assert_eq!(tildify(Path::new("/opt/x"), home), "/opt/x");
        assert_eq!(tildify(Path::new("/opt/x"), None), "/opt/x");
    }
}
