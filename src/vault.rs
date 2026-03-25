use std::path::{Path, PathBuf};

const PROJECT_MARKERS: &[&str] = &[".carddown", ".git", ".hg", ".jj"];
const VAULT_DIR: &str = ".carddown";

/// Resolved vault paths — all data lives in `.carddown/carddown.db` at the vault root.
pub struct VaultPaths {
    pub root: PathBuf,
    pub db_path: PathBuf,
    pub lock_file: PathBuf,
}

impl VaultPaths {
    fn new(root: PathBuf) -> Self {
        let dir = root.join(VAULT_DIR);
        Self {
            db_path: dir.join("carddown.db"),
            lock_file: dir.join("lock"),
            root,
        }
    }

    /// Ensure the `.carddown/` directory exists.
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        let dir = self.root.join(VAULT_DIR);
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
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
    fn test_ensure_dir_creates_carddown() {
        let tmp = TempDir::new().unwrap();
        let paths = VaultPaths::new(tmp.path().to_path_buf());
        assert!(!tmp.path().join(".carddown").exists());
        paths.ensure_dir().unwrap();
        assert!(tmp.path().join(".carddown").exists());
    }
}
