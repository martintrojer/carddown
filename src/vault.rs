use std::path::{Path, PathBuf};

const PROJECT_MARKERS: &[&str] = &[".carddown", ".git", ".hg", ".jj"];
const VAULT_DIR: &str = ".carddown";

/// Resolved vault paths for vault-local config and persistent state files.
pub struct VaultPaths {
    pub root: PathBuf,
    pub vault_dir: PathBuf,
    pub state_dir: PathBuf,
    pub db_path: PathBuf,
    pub lock_file: PathBuf,
}

impl VaultPaths {
    fn new(root: PathBuf) -> Self {
        let vault_dir = root.join(VAULT_DIR);
        Self {
            db_path: vault_dir.join("carddown.db"),
            lock_file: vault_dir.join("lock"),
            state_dir: vault_dir.clone(),
            vault_dir,
            root,
        }
    }

    pub fn with_state_dir(mut self, state_dir: impl AsRef<Path>) -> Self {
        let state_dir = if state_dir.as_ref().is_absolute() {
            state_dir.as_ref().to_path_buf()
        } else {
            self.root.join(state_dir)
        };
        self.db_path = state_dir.join("carddown.db");
        self.lock_file = state_dir.join("lock");
        self.state_dir = state_dir;
        self
    }

    /// Ensure `.carddown/` and storage directories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        if !self.vault_dir.exists() {
            std::fs::create_dir_all(&self.vault_dir)?;
        }
        if !self.state_dir.exists() {
            std::fs::create_dir_all(&self.state_dir)?;
        }
        Ok(())
    }
}

/// Find the vault root by walking up from `start` looking for project markers.
///
/// Priority: `.carddown/` first (explicit vault), then `.git/`, `.hg/`, `.jj/`.
/// Stops at the user's home directory. Falls back to `start` if nothing found.
pub fn find_vault_root(start: &Path) -> VaultPaths {
    let start_canon = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    let home = home_dir();
    let mut current = start_canon.as_path();

    loop {
        for marker in PROJECT_MARKERS {
            if current.join(marker).exists() {
                return VaultPaths::new(current.to_path_buf());
            }
        }
        if home.as_deref() == Some(current) {
            break;
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent,
            _ => break,
        }
    }

    // Nothing found — use the start directory as vault root
    VaultPaths::new(start_canon)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_find_vault_root_with_carddown_dir() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(tmp.path().join(".carddown")).unwrap();

        let paths = find_vault_root(&sub);
        assert_eq!(paths.root, tmp.path().canonicalize().unwrap());
        assert!(paths.db_path.ends_with(".carddown/carddown.db"));
        assert_eq!(
            paths.vault_dir,
            tmp.path().canonicalize().unwrap().join(".carddown")
        );
    }

    #[test]
    fn test_find_vault_root_with_git_dir() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();

        let paths = find_vault_root(&sub);
        assert_eq!(paths.root, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn test_find_vault_root_carddown_takes_priority() {
        let tmp = TempDir::new().unwrap();
        let inner = tmp.path().join("inner");
        let sub = inner.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(inner.join(".carddown")).unwrap();

        let paths = find_vault_root(&sub);
        assert_eq!(paths.root, inner.canonicalize().unwrap());
    }

    #[test]
    fn test_find_vault_root_falls_back_to_start() {
        let tmp = TempDir::new().unwrap();
        let paths = find_vault_root(tmp.path());
        assert_eq!(paths.root, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn test_ensure_dirs_creates_carddown() {
        let tmp = TempDir::new().unwrap();
        let paths = VaultPaths::new(tmp.path().to_path_buf());
        assert!(!tmp.path().join(".carddown").exists());
        paths.ensure_dirs().unwrap();
        assert!(tmp.path().join(".carddown").exists());
    }

    #[test]
    fn test_with_state_dir_relative_to_root() {
        let tmp = TempDir::new().unwrap();
        let paths = VaultPaths::new(tmp.path().to_path_buf()).with_state_dir("../state");
        assert_eq!(paths.state_dir, tmp.path().join("../state"));
        assert_eq!(paths.db_path, tmp.path().join("../state/carddown.db"));
    }
}
