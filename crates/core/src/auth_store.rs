//! Where the launcher token lives: the OS credential store (Windows Credential
//! Manager, the Secret Service on Linux), or a private file when there is
//! none (e.g. a Linux session without a keyring).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::util::{read_json_or_default, write_atomic};

const SERVICE: &str = "UNBEATEN Launcher";

pub struct TokenStore {
    file: PathBuf,
    use_keyring: bool,
}

impl TokenStore {
    /// Tokens go to the OS store, falling back to `<dir>/tokens.json`.
    pub fn new(dir: &Path) -> Self {
        TokenStore {
            file: dir.join("tokens.json"),
            use_keyring: true,
        }
    }

    /// File only (the CLI's `--token-file`, tests).
    pub fn file_only(file: PathBuf) -> Self {
        TokenStore {
            file,
            use_keyring: false,
        }
    }

    fn read_file(&self) -> BTreeMap<String, String> {
        read_json_or_default(&self.file)
    }

    fn write_file(&self, map: &BTreeMap<String, String>) -> std::io::Result<()> {
        let data = serde_json::to_vec_pretty(map).map_err(std::io::Error::other)?;
        write_atomic(&self.file, &data)?;
        restrict_permissions(&self.file);
        Ok(())
    }

    pub fn get(&self, server: &str) -> Option<String> {
        if self.use_keyring {
            if let Ok(entry) = keyring::Entry::new(SERVICE, server) {
                if let Ok(token) = entry.get_password() {
                    return Some(token);
                }
            }
        }
        self.read_file().remove(server)
    }

    pub fn set(&self, server: &str, token: &str) -> std::io::Result<()> {
        if self.use_keyring {
            if let Ok(entry) = keyring::Entry::new(SERVICE, server) {
                if entry.set_password(token).is_ok()
                    && entry.get_password().ok().as_deref() == Some(token)
                {
                    // Stored securely: make sure no stale copy stays on disk.
                    let mut map = self.read_file();
                    if map.remove(server).is_some() {
                        self.write_file(&map)?;
                    }
                    return Ok(());
                }
            }
            log::warn!(
                "no usable OS credential store; keeping the launcher token in {}",
                self.file.display()
            );
        }
        let mut map = self.read_file();
        map.insert(server.to_string(), token.to_string());
        self.write_file(&map)
    }

    pub fn delete(&self, server: &str) {
        if self.use_keyring {
            if let Ok(entry) = keyring::Entry::new(SERVICE, server) {
                let _ = entry.delete_credential();
            }
        }
        let mut map = self.read_file();
        if map.remove(server).is_some() {
            let _ = self.write_file(&map);
        }
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::fs::{set_permissions, Permissions};
    use std::os::unix::fs::PermissionsExt;
    let _ = set_permissions(path, Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::file_only(dir.path().join("tokens.json"));
        assert_eq!(store.get("https://a"), None);
        store.set("https://a", "ubl_1").unwrap();
        store.set("https://b", "ubl_2").unwrap();
        assert_eq!(store.get("https://a").as_deref(), Some("ubl_1"));
        store.delete("https://a");
        assert_eq!(store.get("https://a"), None);
        assert_eq!(store.get("https://b").as_deref(), Some("ubl_2"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("tokens.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
