//! v3 表面资产的流式写入与按块读取（task 019）：内存只保留一个批次的块，
//! 使 1 亿～10 亿三角面的资产可以在远小于展开体积的内存中生成、遍历与简化。

use super::{
    CHUNK_ALIGNMENT, ChunkEntry, HEADER_SIZE, Layout, MAGIC, MESH_ASSET_FORMAT_VERSION,
    MILLIMETERS, MeshAssetError, SURFACE_KIND, Stream, TOC_ENTRY_BYTES, TOC_PREFIX_BYTES, align_up,
    chunk_jobs, chunk_raw, decode_surface_records, encode_chunks, mesh_asset_encoded_len,
    parallel_map, parse_layout, read_exact_io, toc_len, worker_count,
};
use crate::Progress;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::Path;

/// 一个三角面：3 个顶点 × xyz，单位 mm。
pub type Triangle = [[f64; 3]; 3];

/// 流式写入 v3 表面资产：按块调用 `fill(range, out)` 生成三角面（可并行），
/// 每批压缩后立即写出，最后回填目录与头部。返回 `(文件字节数, 资产摘要)`；
/// `progress` 被取消时返回 `Ok(None)`（已写出的不完整文件由调用方删除）。
pub fn write_surface_asset_streaming<W, F>(
    mut writer: W,
    triangle_count: usize,
    fill: F,
    progress: Option<&Progress>,
) -> Result<Option<(u64, String)>, MeshAssetError>
where
    W: Write + Seek,
    F: Fn(Range<usize>, &mut Vec<Triangle>) + Sync,
{
    if triangle_count == 0 {
        return Err(MeshAssetError::InvalidMesh);
    }
    let jobs = chunk_jobs(Stream::SurfaceTriangles, triangle_count);
    let toc_bytes = jobs
        .len()
        .checked_mul(TOC_ENTRY_BYTES)
        .and_then(|len| len.checked_add(TOC_PREFIX_BYTES))
        .ok_or(MeshAssetError::SizeOverflow)?;
    let io = |error: std::io::Error| MeshAssetError::Io(error.kind());
    writer.seek(SeekFrom::Start(0)).map_err(io)?;
    writer
        .write_all(&vec![0; HEADER_SIZE + toc_bytes])
        .map_err(io)?;
    let mut cursor = (HEADER_SIZE + toc_bytes) as u64;
    let mut toc = Vec::with_capacity(toc_bytes);
    toc.extend_from_slice(&(jobs.len() as u64).to_le_bytes());
    let batch = worker_count(jobs.len()) * 2;
    let fill = &fill;
    for (batch_index, batch_jobs) in jobs.chunks(batch).enumerate() {
        if progress.is_some_and(Progress::is_cancelled) {
            return Ok(None);
        }
        let chunks = encode_chunks(batch_jobs, |(_, range), raw| {
            let mut triangles = Vec::with_capacity(range.len());
            fill(range.clone(), &mut triangles);
            if triangles.len() != range.len() {
                return;
            }
            for coordinate in triangles.iter().flatten().flatten() {
                if !coordinate.is_finite() {
                    return;
                }
                raw.extend_from_slice(&coordinate.to_le_bytes());
            }
        })?;
        for chunk in chunks {
            let offset = align_up(cursor).ok_or(MeshAssetError::SizeOverflow)?;
            let padding = (offset - cursor) as usize;
            writer
                .write_all(&[0; CHUNK_ALIGNMENT as usize][..padding])
                .map_err(io)?;
            writer.write_all(&chunk.stored).map_err(io)?;
            let mut entry = [0_u8; TOC_ENTRY_BYTES];
            entry[0] = chunk.stream as u8;
            entry[1] = chunk.codec;
            entry[8..16].copy_from_slice(&offset.to_le_bytes());
            entry[16..24].copy_from_slice(&(chunk.stored.len() as u64).to_le_bytes());
            entry[24..32].copy_from_slice(&(chunk.record_count as u64).to_le_bytes());
            entry[32..64].copy_from_slice(&chunk.digest);
            toc.extend_from_slice(&entry);
            cursor = offset + chunk.stored.len() as u64;
        }
        if let Some(progress) = progress {
            let done = ((batch_index + 1) * batch).min(jobs.len());
            progress.report(0.0, 0.99, done as u64, jobs.len() as u64);
        }
    }
    let digest = Sha256::digest(&toc);
    let mut header = [0_u8; HEADER_SIZE];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&MESH_ASSET_FORMAT_VERSION.to_le_bytes());
    header[12] = SURFACE_KIND;
    header[13] = MILLIMETERS;
    header[16..24].copy_from_slice(&(cursor - HEADER_SIZE as u64).to_le_bytes());
    header[24..56].copy_from_slice(&digest);
    writer.seek(SeekFrom::Start(0)).map_err(io)?;
    writer.write_all(&header).map_err(io)?;
    writer.write_all(&toc).map_err(io)?;
    writer.flush().map_err(io)?;
    if let Some(progress) = progress {
        progress.set_fraction(1.0);
    }
    Ok(Some((cursor, format!("sha256:{digest:x}"))))
}

/// 已打开的 v3 表面资产：只读入头部与目录，数据块按批次按需读取与校验。
pub struct SurfaceAssetChunks {
    file: File,
    layout: Layout,
}

impl SurfaceAssetChunks {
    /// 打开并校验头部与目录；体网格或 v2 资产返回 `UnsupportedLayout`。
    pub fn open(path: &Path) -> Result<Self, MeshAssetError> {
        let io = |error: std::io::Error| MeshAssetError::Io(error.kind());
        let mut file = File::open(path).map_err(io)?;
        let file_len = file.metadata().map_err(io)?.len();
        let mut header = vec![0; HEADER_SIZE];
        read_exact_io(&mut file, &mut header)?;
        if mesh_asset_encoded_len(&header)? as u64 != file_len {
            return Err(MeshAssetError::InvalidLength);
        }
        if u32::from_le_bytes([header[8], header[9], header[10], header[11]])
            != MESH_ASSET_FORMAT_VERSION
            || header[12] != SURFACE_KIND
        {
            return Err(MeshAssetError::UnsupportedLayout);
        }
        let mut toc = vec![0; TOC_PREFIX_BYTES];
        read_exact_io(&mut file, &mut toc)?;
        let len = toc_len(&toc, file_len)?;
        toc.resize(len, 0);
        read_exact_io(&mut file, &mut toc[TOC_PREFIX_BYTES..])?;
        let layout = parse_layout(&header, &toc, file_len)?;
        Ok(Self { file, layout })
    }

    pub fn triangle_count(&self) -> u64 {
        self.layout
            .chunks
            .iter()
            .map(|chunk| chunk.record_count as u64)
            .sum()
    }

    pub fn chunk_count(&self) -> usize {
        self.layout.chunks.len()
    }

    pub fn file_len(&self) -> u64 {
        self.layout.total_len
    }

    pub fn digest(&self) -> &str {
        &self.layout.digest
    }

    /// 按目录顺序批量读取、并行解压校验，每批调用一次 `visit(第一面下标, 各块三角面)`。
    /// `progress` 按 `[start, end]` 报告；取消时返回 `Ok(false)`。
    pub fn visit_batches(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        mut visit: impl FnMut(u64, &[Vec<Triangle>]),
    ) -> Result<bool, MeshAssetError> {
        let io = |error: std::io::Error| MeshAssetError::Io(error.kind());
        let chunks: &[ChunkEntry] = &self.layout.chunks;
        let batch = worker_count(chunks.len()) * 2;
        let mut first = 0_u64;
        for (batch_index, batch_chunks) in chunks.chunks(batch).enumerate() {
            if progress.is_some_and(|(progress, _, _)| progress.is_cancelled()) {
                return Ok(false);
            }
            let mut stored = Vec::with_capacity(batch_chunks.len());
            for chunk in batch_chunks {
                self.file.seek(SeekFrom::Start(chunk.offset)).map_err(io)?;
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(chunk.stored_len)
                    .map_err(|_| MeshAssetError::AllocationFailed)?;
                bytes.resize(chunk.stored_len, 0);
                self.file.read_exact(&mut bytes).map_err(io)?;
                stored.push(bytes);
            }
            let decoded = parallel_map(batch_chunks.len(), |index| {
                let raw = chunk_raw(&batch_chunks[index], &stored[index])?;
                let mut triangles = vec![[[0.0; 3]; 3]; batch_chunks[index].record_count];
                decode_surface_records(&raw, &mut triangles)?;
                Ok(triangles)
            })?;
            drop(stored);
            visit(first, &decoded);
            first += decoded.iter().map(|chunk| chunk.len() as u64).sum::<u64>();
            if let Some((progress, start, end)) = progress {
                let done = ((batch_index + 1) * batch).min(chunks.len());
                progress.report(start, end, done as u64, chunks.len() as u64);
            }
        }
        Ok(true)
    }
}
