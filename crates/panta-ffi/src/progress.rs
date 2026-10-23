//! 后台任务进度句柄：C++ 为每个任务新建一个并把共享副本交给工作线程。

use std::sync::Arc;

/// `Arc<panta_mesh::Progress>` 的 opaque 句柄；所有方法均可跨线程并发调用。
#[derive(Default)]
pub struct TaskProgress(pub(crate) Arc<panta_mesh::Progress>);

pub(super) fn task_progress_new() -> Box<TaskProgress> {
    Box::default()
}

/// 指向同一进度状态的新句柄，供工作线程独占持有。
pub(super) fn task_progress_share(progress: &TaskProgress) -> Box<TaskProgress> {
    Box::new(TaskProgress(Arc::clone(&progress.0)))
}

pub(super) fn task_progress_fraction(progress: &TaskProgress) -> f64 {
    progress.0.fraction()
}

pub(super) fn task_progress_cancel(progress: &TaskProgress) {
    progress.0.cancel();
}

/// 导入进度 0..=1；没有匹配的进行中导入时返回 -1。
pub(super) fn project_service_stl_import_progress(
    service: &crate::ProjectService,
    request: u64,
) -> f64 {
    service.service.stl_import_progress(request).unwrap_or(-1.0)
}

/// 导入预检进度 0..=1；没有匹配的进行中预检时返回 -1。
pub(super) fn project_service_stl_preview_progress(
    service: &crate::ProjectService,
    request: u64,
) -> f64 {
    service
        .service
        .stl_preview_progress(request)
        .unwrap_or(-1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_handles_observe_the_same_progress_and_cancel() {
        let host = task_progress_new();
        let worker = task_progress_share(&host);
        worker.0.set_fraction(0.5);
        assert_eq!(task_progress_fraction(&host), 0.5);
        task_progress_cancel(&host);
        assert!(worker.0.is_cancelled());
    }
}
