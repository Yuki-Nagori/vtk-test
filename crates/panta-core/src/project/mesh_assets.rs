//! Mesh sidecar 的工程内发布与安全读取边界。

use super::ProjectError;
use panta_mesh::{MeshAssetRef, SurfaceMesh, encode_mesh_asset_ref};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// 崩溃后重试可复用内容完全相同的孤立 sidecar；不同内容不得覆盖不可变路径。
pub(super) fn write_surface_mesh_asset_or_reuse(
    path: &Path,
    mesh: &SurfaceMesh,
) -> Result<Option<(String, bool)>, ProjectError> {
    let (bytes, digest) = encode_mesh_asset_ref(MeshAssetRef::Surface(mesh)).map_err(|error| {
        ProjectError::ImportAssetWriteFailed(format!("encode mesh asset: {error}"))
    })?;
    if path.exists() {
        let existing_len = fs::metadata(path)
            .map_err(|error| ProjectError::Io(format!("{}: {error}", path.display())))?
            .len();
        if usize::try_from(existing_len).ok() == Some(bytes.len()) {
            let existing = fs::read(path)
                .map_err(|error| ProjectError::Io(format!("{}: {error}", path.display())))?;
            if existing == bytes {
                return Ok(Some((digest, false)));
            }
        }
        return Ok(None);
    }
    publish_mesh_bytes(path, &bytes)?;
    Ok(Some((digest, true)))
}

fn publish_mesh_bytes(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    let parent = path
        .parent()
        .ok_or_else(|| ProjectError::Io("mesh asset has no parent directory".to_owned()))?;
    fs::create_dir_all(parent)
        .map_err(|error| ProjectError::Io(format!("{}: {error}", parent.display())))?;
    let temporary = create_temporary(path)?;
    let mut guard = TemporaryAsset {
        path: temporary.0.clone(),
        file: Some(temporary.1),
        committed: false,
    };
    let file = guard
        .file
        .as_mut()
        .ok_or_else(|| ProjectError::Io("mesh staging file closed".to_owned()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| ProjectError::Io(format!("{}: {error}", guard.path.display())))?;
    drop(guard.file.take());
    if path.exists() {
        return Err(ProjectError::Io(format!(
            "immutable mesh asset already exists: {}",
            path.display()
        )));
    }
    fs::rename(&guard.path, path)
        .map_err(|error| ProjectError::Io(format!("{}: {error}", path.display())))?;
    guard.committed = true;
    Ok(())
}

fn create_temporary(target: &Path) -> Result<(PathBuf, File), ProjectError> {
    for _ in 0..32 {
        let id = NEXT_TEMPORARY
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| ProjectError::Io("mesh staging identifiers exhausted".to_owned()))?;
        let name = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                ProjectError::ManifestInvalid("invalid mesh asset filename".to_owned())
            })?;
        let path = target.with_file_name(format!(".{name}-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ProjectError::Io(format!("{}: {error}", path.display()))),
        }
    }
    Err(ProjectError::Io(
        "mesh staging collision limit reached".to_owned(),
    ))
}

struct TemporaryAsset {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl Drop for TemporaryAsset {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
