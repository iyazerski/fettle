//! Small JSON files Fettle keeps: read whole, and
//! replaced whole and atomically.

use std::io::{ErrorKind, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Read and parse the file at `path`; `None` if there is none.
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Write `value` to `path` atomically, readable only by the user (some of
/// these files hold secrets).
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let writing = || format!("writing {}", path.display());
    // Beside the file, so the rename stays on one volume; tempfile makes it 0600.
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir).with_context(writing)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.as_file().sync_all()?;
    file.persist(path).with_context(writing)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn files_round_trip_privately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        assert_eq!(load::<BTreeMap<String, u32>>(&path).unwrap(), None);
        let value = BTreeMap::from([("a".to_owned(), 1)]);
        save(&path, &value).unwrap();
        assert_eq!(load(&path).unwrap(), Some(value));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::write(&path, "not json").unwrap();
        assert!(load::<BTreeMap<String, u32>>(&path).is_err());
    }
}
