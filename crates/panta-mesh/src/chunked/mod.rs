//! 单机 out-of-core 分块体网格（task 027 原型）。
//!
//! - 存储（[`format`]）：导入时按 k-d 空间划分把四面体分成若干块，每块独立 zstd 压缩；
//!   块表记录包围盒、场值范围（预留）、16³ 占据体素，块内保留原始单元顺序并存全局 ID。
//!   外表面与简化外表面单独成段，打开时常驻。字节读取经 [`ChunkSource`]：默认只读映射（[`MmapChunkSource`]，task 028），
//!   回退为合并定位读（[`FileChunkSource`]）；以后可换远程源。
//! - 构建（[`build`]）：从随机访问的 [`TetSource`] 流式分块，不驻留整个网格。
//! - 调度（[`cache`]）：按字节预算的 LRU 块缓存，加后台预取线程。
//! - 计算（[`volume`]）：切面粗级别只用常驻数据；精细级别按命中块并行，结果按（块，平面）缓存；
//!   质量统计按块并行，每块结果与平面无关，只算一次。

mod build;
mod cache;
mod format;
#[cfg(test)]
mod tests;
mod volume;

use std::fmt::{Display, Formatter};

pub use build::{ChunkBuildOptions, ChunkBuildSummary, TetSource, build_chunked_volume};
pub use cache::{ChunkCache, Prefetcher};
pub use format::{
    ChunkBytes, ChunkCells, ChunkHeader, ChunkReadMode, ChunkSource, FileChunkSource, IoCounts,
    MmapChunkSource, VOXELS_PER_AXIS, open_chunk_source,
};
pub use volume::{ChunkedVolume, SectionPlane};

/// 分块体网格错误：读写失败或格式不符。
#[derive(Debug, Clone, PartialEq)]
pub enum ChunkError {
    Io(String),
    Format(String),
    Invalid(String),
}

impl Display for ChunkError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(detail) => write!(formatter, "chunk io failed: {detail}"),
            Self::Format(detail) => write!(formatter, "chunk format invalid: {detail}"),
            Self::Invalid(detail) => write!(formatter, "chunk request invalid: {detail}"),
        }
    }
}

impl std::error::Error for ChunkError {}

impl From<std::io::Error> for ChunkError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// 工作线程数：可用并行度，至少 1。
fn worker_count() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}
