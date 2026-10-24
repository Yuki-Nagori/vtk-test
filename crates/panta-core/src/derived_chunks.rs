//! 派生分块缓存（task 028）：超过阈值的 v3 体网格在激活时构建或复用 `.pchunk`，
//! 之后的切面与质量命令按需读块，不再逐次全量扫描 v3 资产。
//!
//! - 位置：宿主设置的缓存目录（桌面端为 `QStandardPaths::CacheLocation/volume-chunks`），
//!   文件名 `<资产 SHA-256 十六进制>.pchunk`；工程目录与 v3 资产都不改动。
//! - 写入：临时文件写完再 rename；已存在且通过校验的文件从不重写（其他窗口可能正映射它）。
//! - 复用：打开时校验头部、目录（`ChunkedVolume::open`）以及节点 / 四面体数与资产一致；
//!   格式版本写在头部，版本不符即视为无效并重建。
//! - 清理：保留修改时间最新的 [`KEEP_FILES`] 个文件，删除失败（Windows 上被映射的文件）忽略。
//! - 失败不致命：缓存目录缺失、内存不足以构建、写入失败都退回 task 025 的逐次流式扫描。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use panta_mesh::VolumeStreamCounts;
use panta_mesh::chunked::{ChunkBuildOptions, ChunkedVolume, build_chunked_from_asset};

use crate::activation::{ActivationFailure, LoadOptions, with_cancellation};

/// 缓存目录中保留的派生文件数。
pub(crate) const KEEP_FILES: usize = 4;
/// 构建峰值内存估计：四面体数组 20 B、重心与排序约 28 B / 四面体，节点 24 B，边界三角 16 B。
const BUILD_BYTES_PER_TET: u64 = 48;
/// 残留临时文件（进程崩溃）超过此时长后清理。
const STALE_TEMP: Duration = Duration::from_secs(24 * 3600);

/// 估计的构建峰值内存（B）。
pub(crate) fn build_memory_estimate(counts: VolumeStreamCounts) -> u64 {
    (counts.tets as u64)
        .saturating_mul(BUILD_BYTES_PER_TET)
        .saturating_add((counts.nodes as u64).saturating_mul(24))
        .saturating_add((counts.boundary as u64).saturating_mul(16))
}

/// `sha256:<64 位十六进制>` → 文件名；其他摘要形式不缓存。
pub(crate) fn file_name(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| format!("{}.pchunk", hex.to_ascii_lowercase()))
}

/// 打开已有派生文件或构建一个；不可用时返回 `Ok(None)`，仅工作线程 panic 返回错误。
pub(crate) fn open_or_build(
    asset_path: &Path,
    digest: &str,
    counts: VolumeStreamCounts,
    options: &LoadOptions,
    cancelled: &impl Fn() -> bool,
) -> Result<Option<Arc<ChunkedVolume>>, ActivationFailure> {
    let Some(directory) = options.environment.derived_dir.as_deref() else {
        return Ok(None);
    };
    let Some(name) = file_name(digest) else {
        return Ok(None);
    };
    let target = directory.join(name);
    let budget = options.limits.chunk_cache_bytes();
    if let Some(volume) = open_valid(&target, counts, budget) {
        touch(&target);
        prune(directory, &target);
        return Ok(Some(volume));
    }
    let available = options.environment.available_memory;
    if available > 0 && build_memory_estimate(counts) > available / 2 {
        return Ok(None);
    }
    if std::fs::create_dir_all(directory).is_err() {
        return Ok(None);
    }
    let temp = temp_path(&target);
    let built = with_cancellation(cancelled, |progress| {
        build_to(asset_path, digest, &temp, progress)
    })?;
    if !built || cancelled() {
        let _ = std::fs::remove_file(&temp);
        return Ok(None);
    }
    // 其他窗口可能已先完成同一资产：保留已有有效文件，丢弃本次结果。
    if open_valid(&target, counts, budget).is_none() && std::fs::rename(&temp, &target).is_err() {
        let _ = std::fs::remove_file(&temp);
        return Ok(None);
    }
    let _ = std::fs::remove_file(&temp);
    prune(directory, &target);
    Ok(open_valid(&target, counts, budget))
}

fn open_valid(
    path: &Path,
    counts: VolumeStreamCounts,
    budget: usize,
) -> Option<Arc<ChunkedVolume>> {
    let volume = ChunkedVolume::open_file(path, budget).ok()?;
    (volume.tet_count() == counts.tets as u64 && volume.node_count() == counts.nodes as u64)
        .then(|| Arc::new(volume))
}

/// 重新打开资产（独立读取器）并写入临时文件；成功且未取消时返回 true。
fn build_to(asset_path: &Path, digest: &str, temp: &Path, progress: &panta_mesh::Progress) -> bool {
    let Ok(mut asset) = panta_mesh::VolumeAssetChunks::open(asset_path) else {
        return false;
    };
    if asset.digest() != digest {
        return false;
    }
    let Ok(file) = std::fs::File::create(temp) else {
        return false;
    };
    let sink = std::io::BufWriter::with_capacity(8 << 20, file);
    matches!(
        build_chunked_from_asset(&mut asset, ChunkBuildOptions::default(), sink, progress),
        Ok(Some(_))
    )
}

fn temp_path(target: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}-{nanos}", std::process::id()));
    target.with_file_name(name)
}

/// 复用时刷新修改时间（LRU）；Windows 上文件被其他进程映射时打开写会失败，忽略。
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

pub(crate) fn prune(directory: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let now = SystemTime::now();
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains(".pchunk.tmp-") {
            if now
                .duration_since(modified)
                .is_ok_and(|age| age > STALE_TEMP)
            {
                let _ = std::fs::remove_file(&path);
            }
        } else if name.ends_with(".pchunk") && path != keep {
            files.push((modified, path));
        }
    }
    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    for (_, path) in files.into_iter().skip(KEEP_FILES.saturating_sub(1)) {
        let _ = std::fs::remove_file(path);
    }
}
