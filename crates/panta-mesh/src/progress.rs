//! 后台任务进度协议：工作线程写入、宿主线程轮询的无锁进度与取消标志。
//!
//! 进度为 0..=1 的整体比例（以万分之一存储）；任务按阶段把局部比例映射到
//! `[start, end]` 区间，保证单调不减。取消是协作式的：工作线程在检查点读取
//! [`Progress::is_cancelled`] 并自行返回取消错误，已越过提交点的任务不受影响。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const SCALE: u32 = 10_000;

#[derive(Debug, Default)]
pub struct Progress {
    basis_points: AtomicU32,
    cancelled: AtomicBool,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前整体比例，范围 0..=1。
    pub fn fraction(&self) -> f64 {
        f64::from(self.basis_points.load(Ordering::Acquire)) / f64::from(SCALE)
    }

    /// 把阶段内 `done / total` 映射到整体 `[start, end]`；只前进，不回退。
    pub fn report(&self, start: f64, end: f64, done: u64, total: u64) {
        let local = if total == 0 {
            1.0
        } else {
            (done as f64 / total as f64).clamp(0.0, 1.0)
        };
        self.set_fraction(start + (end - start) * local);
    }

    /// 直接设置整体比例（会被夹到 0..=1 且只前进）。
    pub fn set_fraction(&self, fraction: f64) {
        let value = (fraction.clamp(0.0, 1.0) * f64::from(SCALE)).round() as u32;
        self.basis_points.fetch_max(value, Ordering::AcqRel);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// 逐项循环的检查点间隔：每处理这么多项更新一次进度并查询取消。
pub const PROGRESS_STRIDE: usize = 1 << 16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_maps_stages_monotonically_and_cancel_is_sticky() {
        let progress = Progress::new();
        assert_eq!(progress.fraction(), 0.0);
        progress.report(0.0, 0.5, 1, 2);
        assert!((progress.fraction() - 0.25).abs() < 1e-9);
        progress.report(0.0, 0.5, 0, 2);
        assert!((progress.fraction() - 0.25).abs() < 1e-9, "不回退");
        progress.report(0.5, 1.0, 0, 0);
        assert_eq!(progress.fraction(), 1.0);
        assert!(!progress.is_cancelled());
        progress.cancel();
        assert!(progress.is_cancelled());
    }
}
