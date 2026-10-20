//! 工程包提交租约：协作式排他、磁盘修订复核与独立临时文件。
use super::{PROJECT_SCHEMA_VERSION, ProjectError, ProjectManifest, ProjectState};
use fs4::{FileExt, TryLockError};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);
const TEMPORARY_ATTEMPTS: usize = 32;

/// 在工程提交租约内原子替换清单，并复核持久化修订。
pub(super) fn write_manifest(state: &mut ProjectState) -> Result<(), ProjectError> {
    WriteLease::acquire(state)?.commit(state)
}

/// 租约覆盖资产写入、清单提交及失败回滚；销毁时先显式解锁再关闭句柄。
/// 稳定锁文件不能删除，否则另一个等待者可能持有不同 inode。
pub(super) struct WriteLease {
    target: PathBuf,
    expected_revision: Option<u64>,
    lock: File,
}

impl WriteLease {
    pub(super) fn acquire(state: &ProjectState) -> Result<Self, ProjectError> {
        let path = state.path.with_extension("panta.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| ProjectError::Io(format!("{}: {error}", path.display())))?;
        FileExt::try_lock(&lock).map_err(|error| match error {
            TryLockError::WouldBlock => {
                ProjectError::CommandInvalid("project write pending".to_owned())
            }
            TryLockError::Error(error) => ProjectError::Io(format!("{}: {error}", path.display())),
        })?;
        let lease = Self {
            target: state.path.clone(),
            expected_revision: state.persisted_revision,
            lock,
        };
        lease.check_disk_revision()?;
        Ok(lease)
    }

    fn check_disk_revision(&self) -> Result<(), ProjectError> {
        let Some(expected) = self.expected_revision else {
            return if self.target.exists() {
                Err(ProjectError::AlreadyExists(
                    self.target.display().to_string(),
                ))
            } else {
                Ok(())
            };
        };
        let bytes = fs::read(&self.target)
            .map_err(|error| ProjectError::Io(format!("{}: {error}", self.target.display())))?;
        let manifest: ProjectManifest = serde_json::from_slice(&bytes).map_err(|error| {
            ProjectError::ManifestInvalid(format!("{}: {error}", self.target.display()))
        })?;
        if manifest.revision != expected {
            return Err(ProjectError::CommandInvalid(
                "project changed on disk".to_owned(),
            ));
        }
        Ok(())
    }

    /// rename 成功就是提交点；其后不再执行会改变提交结果的可失败操作。
    /// sync_all 保证文件内容同步，不承诺各平台断电后的目录项持久性。
    pub(super) fn commit(&self, state: &mut ProjectState) -> Result<(), ProjectError> {
        self.commit_with_before_replace(state, || Ok(()))
    }

    /// 在同目录候选已写入并同步后、原子替换前执行提交裁决。
    /// 取消调用方可在此处关闭取消窗口，避免提交成功但服务丢弃结果。
    pub(super) fn commit_with_before_replace(
        &self,
        state: &mut ProjectState,
        before_replace: impl FnOnce() -> Result<(), ProjectError>,
    ) -> Result<(), ProjectError> {
        if state.path != self.target || state.persisted_revision != self.expected_revision {
            return Err(ProjectError::CommandInvalid(
                "project write lease changed".to_owned(),
            ));
        }
        self.check_disk_revision()?;
        let manifest = ProjectManifest {
            schema: PROJECT_SCHEMA_VERSION,
            name: state.name.clone(),
            revision: state.revision,
            imports: state.imports.clone(),
            mesh_assets: state.mesh_assets.clone(),
            analysis_sequences: state.analysis_sequences.clone(),
            materials: state.materials.clone(),
            fill_settings: state.fill_settings.clone(),
            gate_location_settings: state.gate_location_settings.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| ProjectError::Io(format!("serialize manifest: {error}")))?;
        let mut temporary =
            Temporary::create(self.target.with_extension("panta.write"), &NEXT_TEMPORARY)?;
        let file = temporary
            .file
            .as_mut()
            .ok_or_else(|| ProjectError::Io("temporary file closed".to_owned()))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| ProjectError::Io(format!("{}: {error}", temporary.path.display())))?;
        drop(temporary.file.take());
        before_replace()?;
        fs::rename(&temporary.path, &self.target)
            .map_err(|error| ProjectError::Io(format!("{}: {error}", self.target.display())))?;
        temporary.committed = true;
        state.persisted_revision = Some(state.revision);
        Ok(())
    }
}

impl Drop for WriteLease {
    fn drop(&mut self) {
        // fork / dup 可保留同一 open file description，单独 close 不能及时释放锁。
        // 提交点已由 rename 裁定，解锁失败仅记录诊断；不把已提交状态改报失败。
        if let Err(error) = FileExt::unlock(&self.lock) {
            let _ = writeln!(
                std::io::stderr().lock(),
                "panta: release project write lease {}: {error}",
                self.target.display()
            );
        }
    }
}

struct Temporary {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl Temporary {
    fn create(directory: PathBuf, counter: &AtomicU64) -> Result<Self, ProjectError> {
        fs::create_dir_all(&directory)
            .map_err(|error| ProjectError::Io(format!("{}: {error}", directory.display())))?;
        // create_new 不覆盖遗留文件；碰撞重试有上限，避免在途租约无限等待。
        for _ in 0..TEMPORARY_ATTEMPTS {
            let id = counter
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| ProjectError::Io("temporary file identifiers exhausted".to_owned()))?;
            let path = directory.join(format!("{}-{id}.tmp", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                        committed: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(ProjectError::Io(format!("{}: {error}", path.display()))),
            }
        }
        Err(ProjectError::Io(
            "temporary file collision limit reached".to_owned(),
        ))
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ProjectError, ProjectService, temp_directory};
    use super::{TEMPORARY_ATTEMPTS, Temporary};
    use std::fs;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn temporary_candidates_are_distinct_and_cleanup_preserves_unowned_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let directory = fixture.root.join("staging");
        fs::create_dir(&directory)?;
        let unowned = directory.join("foreign.tmp");
        fs::write(&unowned, "preserve")?;
        let counter = AtomicU64::new(0);
        let first = Temporary::create(directory.clone(), &counter)?;
        let second = Temporary::create(directory, &counter)?;
        assert_ne!(first.path, second.path);
        let first_path = first.path.clone();
        let second_path = second.path.clone();
        drop(first);
        assert!(!first_path.exists());
        assert!(second_path.exists());
        assert_eq!(fs::read_to_string(&unowned)?, "preserve");
        drop(second);
        assert!(!second_path.exists());
        assert_eq!(fs::read_to_string(unowned)?, "preserve");
        Ok(())
    }

    #[test]
    fn temporary_creation_retries_collisions_without_overwriting()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let counter = AtomicU64::new(0);
        let occupied = fixture.root.join(format!("{}-0.tmp", std::process::id()));
        fs::write(&occupied, "preserve")?;
        let temporary = Temporary::create(fixture.root.clone(), &counter)?;
        assert_eq!(
            temporary.path,
            fixture.root.join(format!("{}-1.tmp", std::process::id()))
        );
        drop(temporary);
        assert_eq!(fs::read_to_string(occupied)?, "preserve");
        Ok(())
    }

    #[test]
    fn temporary_creation_reports_a_non_directory_parent_without_overwriting()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let blocked_directory = fixture.root.join("staging");
        fs::write(&blocked_directory, "preserve")?;
        let error = Temporary::create(blocked_directory.clone(), &AtomicU64::new(0))
            .err()
            .ok_or("file unexpectedly accepted as a staging directory")?;

        assert!(matches!(error, ProjectError::Io(_)));
        assert_eq!(fs::read_to_string(blocked_directory)?, "preserve");
        Ok(())
    }

    #[test]
    fn commit_rejects_state_that_no_longer_matches_its_lease()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        let snapshot = service.create(&fixture.root, "Demo")?;
        let manifest_before = fs::read(&snapshot.path)?;
        let mut state = service
            .current
            .take()
            .ok_or("created project state missing")?;
        let lease = super::WriteLease::acquire(&state)?;
        state.persisted_revision = Some(state.revision + 1);

        assert!(matches!(
            lease.commit(&mut state),
            Err(ProjectError::CommandInvalid(message))
                if message == "project write lease changed"
        ));
        assert_eq!(fs::read(&snapshot.path)?, manifest_before);
        Ok(())
    }

    #[test]
    fn commit_does_not_recreate_a_manifest_removed_during_lease()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        let snapshot = service.create(&fixture.root, "Removed manifest")?;
        let mut state = service
            .current
            .take()
            .ok_or("created project state missing")?;
        let lease = super::WriteLease::acquire(&state)?;
        fs::remove_file(&snapshot.path)?;
        state.name = "Must not be written".into();
        state.revision += 1;

        assert!(matches!(lease.commit(&mut state), Err(ProjectError::Io(_))));
        assert!(!snapshot.path.exists());

        Ok(())
    }

    #[test]
    fn commit_rejects_a_manifest_revision_changed_during_lease()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        let snapshot = service.create(&fixture.root, "Changed revision")?;
        let mut state = service
            .current
            .take()
            .ok_or("created project state missing")?;
        let lease = super::WriteLease::acquire(&state)?;
        let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&snapshot.path)?)?;
        manifest["revision"] = serde_json::Value::from(state.revision + 1);
        let external_update = serde_json::to_vec_pretty(&manifest)?;
        fs::write(&snapshot.path, &external_update)?;
        state.name = "Must not overwrite disk".into();
        state.revision += 1;

        let error = lease
            .commit(&mut state)
            .err()
            .ok_or("stale write lease unexpectedly committed")?;

        assert!(matches!(error, ProjectError::CommandInvalid(_)));
        assert_eq!(error.code(), "project.command_invalid");
        assert_eq!(error.detail(), "project changed on disk");
        assert_eq!(fs::read(snapshot.path)?, external_update);
        Ok(())
    }

    #[test]
    fn temporary_creation_bounds_collisions_and_identifier_exhaustion()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        for id in 0..TEMPORARY_ATTEMPTS {
            fs::write(
                fixture
                    .root
                    .join(format!("{}-{id}.tmp", std::process::id())),
                "preserve",
            )?;
        }
        let counter = AtomicU64::new(0);
        let error = Temporary::create(fixture.root.clone(), &counter)
            .err()
            .ok_or("colliding candidates unexpectedly succeeded")?;
        assert_eq!(error.detail(), "temporary file collision limit reached");
        assert_eq!(
            fs::read_dir(&fixture.root)?
                .collect::<Result<Vec<_>, _>>()?
                .len(),
            TEMPORARY_ATTEMPTS
        );
        let exhausted = AtomicU64::new(u64::MAX);
        let error = Temporary::create(fixture.root.clone(), &exhausted)
            .err()
            .ok_or("exhausted identifiers unexpectedly succeeded")?;
        assert_eq!(error.detail(), "temporary file identifiers exhausted");
        for id in 0..TEMPORARY_ATTEMPTS {
            assert_eq!(
                fs::read_to_string(
                    fixture
                        .root
                        .join(format!("{}-{id}.tmp", std::process::id()))
                )?,
                "preserve"
            );
        }
        Ok(())
    }

    #[test]
    fn lease_release_is_not_prolonged_by_a_duplicated_file_handle()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = super::super::ProjectService::new();
        let project = service.create(&fixture.root, "Duplicated")?;
        let mut state = service.current.as_ref().ok_or("missing project")?.clone();
        let lease = super::WriteLease::acquire(&state)?;
        // dup 与 fork 继承共享 open file description，控制其存活期即可复现竞态。
        let duplicated = lease.lock.try_clone()?;
        let probe = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(project.path.with_extension("panta.lock"))?;
        assert!(matches!(
            fs4::FileExt::try_lock(&probe),
            Err(fs4::TryLockError::WouldBlock)
        ));
        state.name = "Committed".into();
        state.revision += 1;
        lease.commit(&mut state)?;
        assert!(matches!(
            fs4::FileExt::try_lock(&probe),
            Err(fs4::TryLockError::WouldBlock)
        ));
        drop(lease);
        fs4::FileExt::try_lock(&probe)?;
        drop(duplicated);
        let competing = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(project.path.with_extension("panta.lock"))?;
        assert!(matches!(
            fs4::FileExt::try_lock(&competing),
            Err(fs4::TryLockError::WouldBlock)
        ));
        fs4::FileExt::unlock(&probe)?;
        let mut reopened = super::super::ProjectService::new();
        reopened.open(&project.path)?;
        assert_eq!(reopened.current()?.name, "Committed");
        assert_eq!(reopened.current()?.revision, 1);
        Ok(())
    }

    #[test]
    fn lease_acquisition_rejects_a_target_created_after_project_creation()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        service.create(&fixture.root, "Raced create")?;
        let mut state = service.current.as_ref().ok_or("missing project")?.clone();
        state.persisted_revision = None;

        let error = super::WriteLease::acquire(&state)
            .err()
            .ok_or("existing project target unexpectedly acquired a create lease")?;

        assert!(matches!(error, ProjectError::AlreadyExists(_)));
        Ok(())
    }

    #[test]
    fn lease_acquisition_rejects_a_corrupted_project_manifest()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Corrupted manifest")?;
        let state = service.current.as_ref().ok_or("missing project")?.clone();
        fs::write(&project.path, b"not a project manifest")?;

        let error = super::WriteLease::acquire(&state)
            .err()
            .ok_or("corrupted project manifest unexpectedly acquired a lease")?;

        assert!(matches!(error, ProjectError::ManifestInvalid(_)));
        Ok(())
    }

    #[test]
    fn lease_acquisition_reports_an_unopenable_lock_path() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Blocked lock")?;
        let state = service.current.as_ref().ok_or("missing project")?.clone();
        let lock_path = project.path.with_extension("panta.lock");

        fs::remove_file(&lock_path)?;
        fs::create_dir(&lock_path)?;
        let error = super::WriteLease::acquire(&state)
            .err()
            .ok_or("directory unexpectedly opened as a lock file")?;

        assert!(matches!(error, ProjectError::Io(_)));
        assert!(error.detail().contains(&lock_path.display().to_string()));
        assert!(project.path.is_file());
        Ok(())
    }
}
