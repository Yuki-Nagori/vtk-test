//! 只读 STL 预检：后台拥有字节 / 网格，服务只接收仍有效的请求结果。
use super::{ProjectError, ProjectService, StlImportPreview, import};
use panta_io::import::CheckedReadError;
use panta_io::import::StlImportSession;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::{path::Path, sync::Arc};

#[derive(Debug)]
pub(super) struct PendingPreview {
    request: u64,
    cancelled: Arc<AtomicBool>,
    progress: Arc<panta_mesh::Progress>,
    receiver: Receiver<Result<(StlImportSession, StlImportPreview), ProjectError>>,
}

impl Drop for PendingPreview {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl ProjectService {
    /// 发起只读预检，替换前一请求；不在调用线程读取或解析文件。
    /// 返回运行期请求编号；由 finish 拉取，服务销毁会请求取消并丢弃迟到结果。
    pub fn begin_stl_preview(&mut self, source: &Path) -> Result<u64, ProjectError> {
        let request = self
            .preview_request
            .checked_add(1)
            .ok_or_else(|| ProjectError::CommandInvalid("preview request exhausted".to_owned()))?;
        self.clear_stl_preview();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let progress = Arc::new(panta_mesh::Progress::new());
        let worker_progress = Arc::clone(&progress);
        let source = source.to_path_buf();
        let (sender, receiver) = mpsc::channel();
        self.execution
            .reads
            .submit(move || {
                let mut session = StlImportSession::default();
                let result = session.preview_with_progress(
                    &source,
                    || worker_cancelled.load(Ordering::Acquire),
                    &|value| worker_progress.set_fraction(value),
                );
                let result = match result {
                    Ok(preview) => Ok((session, preview)),
                    Err(CheckedReadError::Failed(error)) => Err(import::map_import_error(error)),
                    Err(CheckedReadError::Cancelled(_)) => return,
                };
                // 取消时接收端已移除；失败发送会在后台释放大快照。
                if worker_cancelled.load(Ordering::Acquire) {
                    return;
                }
                let _ = sender.send(result);
            })
            .map_err(|error| ProjectError::Io(format!("start STL preview: {error}")))?;
        self.preview_request = request;
        self.pending_preview = Some(PendingPreview {
            request,
            cancelled,
            progress,
            receiver,
        });
        Ok(request)
    }

    /// 预检进度 0..=1（读取 0–50%，解析 50–100%）；无匹配进行中请求时为 None。
    pub fn stl_preview_progress(&self, request: u64) -> Option<f64> {
        self.pending_preview
            .as_ref()
            .filter(|pending| pending.request == request)
            .map(|pending| pending.progress.fraction())
    }

    /// 拉取指定请求：None 表示尚未完成；成功会保存同一来源快照供确认导入。
    /// 旧编号被拒绝，不消费当前请求；失败保留当前工程与已提交 Mesh。
    pub fn finish_stl_preview(
        &mut self,
        request: u64,
    ) -> Result<Option<StlImportPreview>, ProjectError> {
        let pending = self
            .pending_preview
            .as_ref()
            .filter(|job| job.request == request)
            .ok_or_else(|| {
                ProjectError::CommandInvalid("no matching STL preview pending".to_owned())
            })?;
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => Err(ProjectError::Io(
                "STL preview worker disconnected".to_owned(),
            )),
        };
        self.pending_preview = None;
        let (session, preview) = result?;
        self.import_session = session;
        Ok(Some(preview))
    }

    /// 取消当前编号的请求或清除其已完成快照，不等待线程。
    /// 返回编号是否匹配；旧编号不能清除后续请求或其导入快照。
    pub fn cancel_stl_preview(&mut self, request: u64) -> bool {
        if request != 0 && self.preview_request == request {
            self.clear_stl_preview();
            return true;
        }
        false
    }

    pub(super) fn clear_stl_preview(&mut self) {
        self.pending_preview = None;
        self.import_session.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::super::temp_directory;
    use super::{PendingPreview, ProjectService, StlImportPreview};
    use panta_io::import::StlImportSession;
    use std::{
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };

    #[test]
    fn pending_poll_and_stale_request_do_not_consume_the_current_reply()
    -> Result<(), Box<dyn std::error::Error>> {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut service = ProjectService::new();
        service.preview_request = 2;
        service.pending_preview = Some(PendingPreview {
            request: 2,
            cancelled: Arc::clone(&cancelled),
            progress: Arc::default(),
            receiver,
        });
        assert_eq!(service.stl_preview_progress(2), Some(0.0));
        assert_eq!(service.stl_preview_progress(1), None);
        assert!(service.finish_stl_preview(2)?.is_none());
        assert!(service.finish_stl_preview(1).is_err());
        sender.send(Ok((
            StlImportSession::default(),
            StlImportPreview {
                source_name: "current.stl".into(),
                triangle_count: 1,
                expanded_mesh_bytes: 136,
                large_mesh_warning: false,
                dimensions: [1.0, 1.0, 0.0],
            },
        )))?;
        assert_eq!(
            service
                .finish_stl_preview(2)?
                .ok_or("preview reply missing")?
                .source_name,
            "current.stl"
        );
        assert!(cancelled.load(Ordering::Acquire));
        Ok(())
    }

    #[test]
    fn disconnected_worker_and_request_exhaustion_are_recoverable() {
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let mut service = ProjectService::new();
        service.preview_request = 1;
        service.pending_preview = Some(PendingPreview {
            request: 1,
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::default(),
            receiver,
        });
        assert_eq!(
            service
                .finish_stl_preview(1)
                .err()
                .map(|error| error.code()),
            Some("project.io")
        );
        assert!(service.pending_preview.is_none());
        service.preview_request = u64::MAX;
        assert_eq!(
            service
                .begin_stl_preview(Path::new("unused.stl"))
                .err()
                .map(|error| error.code()),
            Some("project.command_invalid")
        );
    }

    #[test]
    fn full_read_executor_rejects_preview_without_publishing_pending_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let executor = crate::execution::Executor::new("preview-capacity-test", 1, 1);
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        executor.submit(move || {
            let _ = started.send(());
            let _ = gate.recv();
        })?;
        ready.recv_timeout(std::time::Duration::from_secs(2))?;

        let mut service = ProjectService::new();
        service.execution.reads = Arc::clone(&executor);
        let error = service
            .begin_stl_preview(Path::new("unused.stl"))
            .err()
            .ok_or("preview unexpectedly entered a full executor")?;
        assert_eq!(error.code(), "project.io");
        assert_eq!(service.preview_request, 0);
        assert!(service.pending_preview.is_none());

        release.send(())?;
        executor.close_and_wait();
        Ok(())
    }

    #[test]
    fn queued_replacement_cancels_old_work_and_preserves_the_latest_reply()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let source = fixture.root.join("queued.stl");
        std::fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
        let executor = crate::execution::Executor::new("preview-queue-test", 1, 4);
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        executor.submit(move || {
            let _ = started.send(());
            let _ = gate.recv();
        })?;
        ready.recv_timeout(std::time::Duration::from_secs(2))?;
        let mut service = ProjectService::new();
        service.execution.reads = executor;
        let first = service.begin_stl_preview(&source)?;
        let cancelled = Arc::clone(
            &service
                .pending_preview
                .as_ref()
                .ok_or("no preview")?
                .cancelled,
        );
        let second = service.begin_stl_preview(&source)?;
        assert!(cancelled.load(Ordering::Acquire));
        assert!(service.finish_stl_preview(first).is_err());
        release.send(())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(preview) = service.finish_stl_preview(second)? {
                assert_eq!(preview.triangle_count, 1);
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err("latest reply missing".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    }
}
