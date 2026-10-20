//! 进程崩溃设施与后台写入收尾的安全转发。

/// 仅转发退出收尾；领域关闭和已接受写入的所有权由 core 管理。
pub(super) fn finish_background_writes() {
    panta_core::finish_background_writes();
}

/// 任务 047：错误以文本跨边界（C++ 侧 qWarning 呈现，不静默）。
pub(super) fn install_crash_handler(log_dir: &str) -> Result<String, String> {
    panta_foundation::crash::install_crash_handler(log_dir)
        .map(|path| path.display().to_string())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::finish_background_writes;
    use crate::install_crash_handler;
    use crate::temp_directory::Fixture;

    #[test]
    fn crash_handler_rejects_a_file_used_as_log_directory() -> std::io::Result<()> {
        let path =
            std::env::temp_dir().join(format!("panta-ffi-crash-not-dir-{}", std::process::id()));
        std::fs::write(&path, b"not a directory")?;
        let text = path.to_str().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "临时路径不是 UTF-8")
        })?;
        let error = install_crash_handler(text)
            .err()
            .ok_or_else(|| std::io::Error::other("文件路径不应成为日志目录"))?;
        assert!(!error.is_empty());
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn crash_handler_returns_log_path_on_success() -> Result<(), Box<dyn std::error::Error>> {
        const ROOT_ENV: &str = "PANTA_FFI_TEST_CRASH_HANDLER";
        const CHILD_ENV: &str = "PANTA_FFI_TEST_CRASH_HANDLER_CHILD";

        if std::env::var_os(CHILD_ENV).is_some() {
            let root = match std::env::var_os(ROOT_ENV) {
                Some(root) => root,
                None => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "child fixture path is missing",
                    )
                    .into());
                }
            };
            let root = std::path::PathBuf::from(root);
            let directory = match root.to_str() {
                Some(directory) => directory,
                None => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "fixture path should be UTF-8",
                    )
                    .into());
                }
            };
            let returned = install_crash_handler(directory).map_err(std::io::Error::other)?;
            let returned = std::path::PathBuf::from(returned);
            assert_eq!(
                panta_foundation::crash::crash_log_path().as_deref(),
                Some(returned.as_path())
            );
            assert!(returned.is_file(), "{}", returned.display());
            return Ok(());
        }

        let fixture = Fixture::new()?;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "support::tests::crash_handler_returns_log_path_on_success",
                "--nocapture",
            ])
            .env(ROOT_ENV, &fixture.root)
            .env(CHILD_ENV, "1")
            .output()?;
        assert!(
            output.status.success(),
            "crash handler child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    #[test]
    fn shutdown_bridge_finishes_an_accepted_project_write() -> Result<(), Box<dyn std::error::Error>>
    {
        const ROOT_ENV: &str = "PANTA_FFI_TEST_WRITE_SHUTDOWN";

        if let Some(root) = std::env::var_os(ROOT_ENV) {
            let root = std::path::PathBuf::from(root);
            let mut service = panta_core::project::ProjectService::new();
            let project = service.create(&root, "Shutdown")?;
            service.execute(panta_core::project::ProjectCommand::Rename {
                name: "Saved during shutdown".into(),
            })?;
            let revision = service.current()?.revision;
            let request =
                service.begin_project_operation(panta_core::project::ProjectOperation::Save)?;
            drop(service);
            drop(request);

            finish_background_writes();

            let mut reopened = panta_core::project::ProjectService::new();
            let saved = reopened.open(&project.path)?;
            assert_eq!(saved.name, "Saved during shutdown");
            assert_eq!(saved.revision, revision);
            assert!(!saved.dirty);
            return Ok(());
        }

        let fixture = Fixture::new()?;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "support::tests::shutdown_bridge_finishes_an_accepted_project_write",
                "--nocapture",
            ])
            .env(ROOT_ENV, &fixture.root)
            .output()?;
        assert!(
            output.status.success(),
            "shutdown child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }
}
