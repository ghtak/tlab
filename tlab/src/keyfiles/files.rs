use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, bail};
use tempfile::NamedTempFile;

pub(super) struct FilePair<'a> {
    private: &'a Path,
    companion: &'a Path,
}

impl<'a> FilePair<'a> {
    pub(super) fn new(private: &'a Path, companion: &'a Path) -> Self {
        Self { private, companion }
    }

    /// Returns None only when both files are absent and generation is allowed.
    pub(super) fn read_existing(&self, generate: bool) -> anyhow::Result<Option<(String, String)>> {
        match (path_exists(self.private)?, path_exists(self.companion)?) {
            (true, true) => Ok(Some((read_pem(self.private)?, read_pem(self.companion)?))),
            (false, false) if generate => Ok(None),
            (false, false) => bail!("key files are missing and generation is disabled"),
            _ => bail!("key file pair is incomplete; existing files will not be replaced"),
        }
    }

    pub(super) fn persist(&self, private: &str, companion: &str) -> anyhow::Result<()> {
        let private_temp = prepare_file(self.private, private)?;
        let companion_temp = prepare_file(self.companion, companion)?;
        publish(private_temp, self.private)?;
        publish(companion_temp, self.companion)?;
        Ok(())
    }
}

pub(super) fn validate_paths(private: &Path, companion: &Path) -> crate::Result<()> {
    if private.as_os_str().is_empty() || companion.as_os_str().is_empty() {
        return Err(crate::Error::InvalidInput(
            "key file paths must not be empty".into(),
        ));
    }
    if path_identity(private).context("failed to resolve private key path")?
        == path_identity(companion).context("failed to resolve companion file path")?
    {
        return Err(crate::Error::InvalidInput(
            "key file paths must be distinct".into(),
        ));
    }
    Ok(())
}

fn path_identity(path: &Path) -> io::Result<PathBuf> {
    let mut resolved = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            _ => {
                resolved.push(component.as_os_str());
                match fs::canonicalize(&resolved) {
                    Ok(canonical) => resolved = canonical,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

fn path_exists(path: &Path) -> anyhow::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect `{}`", path.display())),
    }
}

fn read_pem(path: &Path) -> anyhow::Result<String> {
    fs::read_to_string(path)
        .with_context(|| format!("failed to read PEM file `{}`", path.display()))
}

fn prepare_file(path: &Path, contents: &str) -> anyhow::Result<NamedTempFile> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create key file directory `{}`", parent.display()))?;
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        builder.permissions(fs::Permissions::from_mode(0o600));
    }
    let mut temporary = builder
        .tempfile_in(parent)
        .with_context(|| format!("failed to create temporary file for `{}`", path.display()))?;
    #[cfg(unix)]
    {
        // Creation is already restricted to 0600. Restore owner permissions
        // that an unusually restrictive umask may have removed.
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| {
                format!(
                    "failed to set key file permissions for `{}`",
                    path.display()
                )
            })?;
    }
    temporary
        .write_all(contents.as_bytes())
        .with_context(|| format!("failed to write temporary file for `{}`", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to sync temporary file for `{}`", path.display()))?;
    Ok(temporary)
}

fn publish(temporary: NamedTempFile, path: &Path) -> anyhow::Result<()> {
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "failed to publish key file `{}` without overwriting",
                path.display()
            )
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_collision_keeps_existing_private_key_and_cleans_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let private = directory.path().join("private.pem");
        let public = directory.path().join("public.pem");
        fs::write(&private, "existing private key").unwrap();
        // Simulate a competitor publishing after the initial existence check.
        assert!(
            FilePair::new(&private, &public)
                .persist("new private key", "new public key")
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(&private).unwrap(),
            "existing private key"
        );
        assert!(!public.exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn companion_collision_leaves_private_key_and_existing_companion_for_manual_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let private = directory.path().join("private.pem");
        let public = directory.path().join("public.pem");
        fs::write(&public, "existing public key").unwrap();
        assert!(
            FilePair::new(&private, &public)
                .persist("new private key", "new public key")
                .is_err()
        );
        assert_eq!(fs::read_to_string(&private).unwrap(), "new private key");
        assert_eq!(fs::read_to_string(&public).unwrap(), "existing public key");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}
