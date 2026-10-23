//! 面向工程旁置文件的版本化 Mesh 编解码；不依赖 VTK / Netgen 内存布局。
//!
//! v3 布局（全部小端）：
//!
//! ```text
//! [0..56)   头部：magic、版本、kind、单位、保留字节、payload 长度、目录 SHA-256
//! [56..)    目录：块数 u64 + 每块 64 B 条目（流、编码、过滤、LOD、时间步、偏移、长度、记录数、块 SHA-256）
//! [对齐 64) 数据块：每块独立压缩 / 校验，可单独定位、按需解码或并行解码
//! ```
//!
//! 资产摘要是目录的 SHA-256；目录又含各块摘要，因此逐块校验后与整文件摘要等价，
//! 而读取端无需先对整个文件做一次串行哈希。LOD 层级与时间步字段在 v3 中必须为 0，
//! 读取端遇到非 0 值显式拒绝，后续格式版本再赋予语义。

mod v2;

use crate::{SurfaceMesh, SurfaceTriangle, TetMesh, Tetrahedron, validate_tet_mesh};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::io::{self, Read};
use std::ops::Range;

const MAGIC: &[u8; 8] = b"PNTMESH\0";
pub const MESH_ASSET_FORMAT_VERSION: u32 = 3;
const HEADER_SIZE: usize = 56;
const TOC_PREFIX_BYTES: usize = 8;
const TOC_ENTRY_BYTES: usize = 64;
/// 数据块起点按 64 B 对齐，便于未压缩块今后直接映射为 f64 / u32 切片。
const CHUNK_ALIGNMENT: u64 = 64;
/// 单块未压缩目标约 4 MiB：足够摊薄 Zstandard 帧开销，又让并行解码和按需读取粒度够细。
const CHUNK_RAW_TARGET_BYTES: usize = 4 * 1024 * 1024;
/// 读取端拒绝超过 64 MiB 的单块，限制损坏目录导致的单次分配。
const CHUNK_RAW_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const ZSTD_LEVEL: i32 = 3;
const SURFACE_KIND: u8 = 1;
const TET_KIND: u8 = 2;
const MILLIMETERS: u8 = 1;

const CODEC_NONE: u8 = 0;
const CODEC_ZSTD: u8 = 1;

/// 块内记录流；数值即目录中的 stream 字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Stream {
    /// 未焊接三角形：9 × f64。
    SurfaceTriangles = 1,
    /// 体网格计数：region_count u32 + boundary_group_count u32，恰好一条记录。
    VolumeCounts = 2,
    /// 节点：3 × f64。
    VolumeNodes = 3,
    /// 四面体：4 × u32 节点 + u32 region。
    VolumeTets = 4,
    /// 边界三角形：3 × u32 节点 + u32 group。
    VolumeBoundary = 5,
}

impl Stream {
    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            1 => Self::SurfaceTriangles,
            2 => Self::VolumeCounts,
            3 => Self::VolumeNodes,
            4 => Self::VolumeTets,
            5 => Self::VolumeBoundary,
            _ => return None,
        })
    }

    fn record_bytes(self) -> usize {
        match self {
            Self::SurfaceTriangles => 72,
            Self::VolumeCounts => 8,
            Self::VolumeNodes => 24,
            Self::VolumeTets => 20,
            Self::VolumeBoundary => 16,
        }
    }

    fn records_per_chunk(self) -> usize {
        (CHUNK_RAW_TARGET_BYTES / self.record_bytes()).max(1)
    }
}

/// 未压缩表面 Mesh 的展开字节数（头部 + 计数 + 每面 72 B），不估算进程总内存。
/// 超出 `u64` 时饱和，供资源提醒使用；不据此分配或限制网格规模。
pub fn surface_mesh_expanded_bytes(triangle_count: u64) -> u64 {
    triangle_count
        .saturating_mul(Stream::SurfaceTriangles.record_bytes() as u64)
        .saturating_add((HEADER_SIZE + TOC_PREFIX_BYTES) as u64)
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeshAsset {
    Surface(SurfaceMesh),
    Volume(TetMesh),
}

pub enum MeshAssetRef<'a> {
    Surface(&'a SurfaceMesh),
    Volume(&'a TetMesh),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshAssetError {
    InvalidHeader,
    UnsupportedVersion(u32),
    UnsupportedKind(u8),
    UnsupportedUnits(u8),
    InvalidLength,
    ChecksumMismatch,
    UnsupportedCodec(u8),
    /// 目录中的 LOD 层级 / 时间步 / 过滤器 / 流不属于当前版本定义。
    UnsupportedLayout,
    CodecFailed,
    InvalidMesh,
    SizeOverflow,
    AllocationFailed,
    /// 流式读取时底层 I/O 失败；格式错误不使用该变体。
    Io(io::ErrorKind),
}

impl Display for MeshAssetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MeshAssetError {}

/// 目录中一个数据块的位置与校验信息。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChunkEntry {
    stream: Stream,
    codec: u8,
    offset: u64,
    stored_len: usize,
    record_count: usize,
    digest: [u8; 32],
}

impl ChunkEntry {
    fn raw_len(&self) -> usize {
        // 解析目录时已确认不溢出且不超过单块上限。
        self.record_count * self.stream.record_bytes()
    }
}

/// 已通过校验的头部与目录；数据块尚未读取。
#[derive(Debug, Clone)]
struct Layout {
    kind: u8,
    total_len: u64,
    chunks: Vec<ChunkEntry>,
    digest: String,
}

// ---------------------------------------------------------------- 编码

pub fn encode_mesh_asset(mesh: &MeshAsset) -> Result<(Vec<u8>, String), MeshAssetError> {
    match mesh {
        MeshAsset::Surface(mesh) => encode_mesh_asset_ref(MeshAssetRef::Surface(mesh)),
        MeshAsset::Volume(mesh) => encode_mesh_asset_ref(MeshAssetRef::Volume(mesh)),
    }
}

/// 以 v3 分块布局编码；各块并行重排、压缩与哈希，返回完整文件字节与资产摘要。
pub fn encode_mesh_asset_ref(mesh: MeshAssetRef<'_>) -> Result<(Vec<u8>, String), MeshAssetError> {
    match mesh {
        MeshAssetRef::Surface(mesh) => {
            if mesh.triangles.is_empty()
                || mesh
                    .triangles
                    .iter()
                    .flatten()
                    .flatten()
                    .any(|coordinate| !coordinate.is_finite())
            {
                return Err(MeshAssetError::InvalidMesh);
            }
            let jobs = chunk_jobs(Stream::SurfaceTriangles, mesh.triangles.len());
            let chunks = encode_chunks(&jobs, |(_, range), raw| {
                for triangle in &mesh.triangles[range] {
                    for coordinate in triangle.iter().flatten() {
                        raw.extend_from_slice(&coordinate.to_le_bytes());
                    }
                }
            })?;
            assemble(SURFACE_KIND, chunks)
        }
        MeshAssetRef::Volume(mesh) => {
            if !validate_tet_mesh(mesh).is_ok() {
                return Err(MeshAssetError::InvalidMesh);
            }
            let mut jobs = vec![(Stream::VolumeCounts, 0..1)];
            jobs.extend(chunk_jobs(Stream::VolumeNodes, mesh.nodes.len()));
            jobs.extend(chunk_jobs(Stream::VolumeTets, mesh.tets.len()));
            jobs.extend(chunk_jobs(Stream::VolumeBoundary, mesh.boundary.len()));
            let chunks = encode_chunks(&jobs, |job, raw| {
                encode_volume_records(mesh, job, raw);
            })?;
            assemble(TET_KIND, chunks)
        }
    }
}

fn chunk_jobs(stream: Stream, records: usize) -> Vec<(Stream, Range<usize>)> {
    let step = stream.records_per_chunk();
    (0..records)
        .step_by(step)
        .map(|start| (stream, start..(start + step).min(records)))
        .collect()
}

fn encode_volume_records(
    mesh: &TetMesh,
    (stream, range): (Stream, Range<usize>),
    raw: &mut Vec<u8>,
) {
    match stream {
        Stream::VolumeCounts => {
            raw.extend_from_slice(&mesh.region_count.to_le_bytes());
            raw.extend_from_slice(&mesh.boundary_group_count.to_le_bytes());
        }
        Stream::VolumeNodes => {
            for coordinate in mesh.nodes[range].iter().flatten() {
                raw.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        Stream::VolumeTets => {
            for tet in &mesh.tets[range] {
                for node in tet.nodes {
                    raw.extend_from_slice(&node.to_le_bytes());
                }
                raw.extend_from_slice(&tet.region.to_le_bytes());
            }
        }
        Stream::VolumeBoundary => {
            for triangle in &mesh.boundary[range] {
                for node in triangle.nodes {
                    raw.extend_from_slice(&node.to_le_bytes());
                }
                raw.extend_from_slice(&triangle.group.to_le_bytes());
            }
        }
        Stream::SurfaceTriangles => {}
    }
}

struct EncodedChunk {
    stream: Stream,
    codec: u8,
    record_count: usize,
    stored: Vec<u8>,
    digest: [u8; 32],
}

fn encode_chunks<F>(
    jobs: &[(Stream, Range<usize>)],
    fill: F,
) -> Result<Vec<EncodedChunk>, MeshAssetError>
where
    F: Fn((Stream, Range<usize>), &mut Vec<u8>) + Sync,
{
    let fill = &fill;
    parallel_map(jobs.len(), |index| {
        let (stream, range) = jobs[index].clone();
        let record_count = range.len();
        let raw_len = record_count
            .checked_mul(stream.record_bytes())
            .ok_or(MeshAssetError::SizeOverflow)?;
        let mut raw = Vec::new();
        raw.try_reserve_exact(raw_len)
            .map_err(|_| MeshAssetError::AllocationFailed)?;
        fill((stream, range), &mut raw);
        if raw.len() != raw_len {
            return Err(MeshAssetError::InvalidMesh);
        }
        // 未焊接三角形中相邻面共享的顶点是逐字节重复，直接压缩即可命中；
        // 实测字节平面重排会打断这些重复，使体积约翻倍，因此不做过滤。
        let compressed = zstd::bulk::compress(&raw, ZSTD_LEVEL).map_err(map_codec_error)?;
        // 不可压缩的数据原样保存，保留可直接映射的自然字节序。
        let (codec, stored) = if compressed.len() < raw.len() {
            (CODEC_ZSTD, compressed)
        } else {
            (CODEC_NONE, raw)
        };
        let digest = Sha256::digest(&stored).into();
        Ok(EncodedChunk {
            stream,
            codec,
            record_count,
            stored,
            digest,
        })
    })
}

fn assemble(kind: u8, chunks: Vec<EncodedChunk>) -> Result<(Vec<u8>, String), MeshAssetError> {
    let toc_len = chunks
        .len()
        .checked_mul(TOC_ENTRY_BYTES)
        .and_then(|len| len.checked_add(TOC_PREFIX_BYTES))
        .ok_or(MeshAssetError::SizeOverflow)?;
    let mut offsets = Vec::with_capacity(chunks.len());
    let mut cursor = (HEADER_SIZE + toc_len) as u64;
    for chunk in &chunks {
        cursor = align_up(cursor).ok_or(MeshAssetError::SizeOverflow)?;
        offsets.push(cursor);
        cursor = cursor
            .checked_add(chunk.stored.len() as u64)
            .ok_or(MeshAssetError::SizeOverflow)?;
    }
    let total_len = usize::try_from(cursor).map_err(|_| MeshAssetError::SizeOverflow)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total_len)
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    bytes.resize(HEADER_SIZE, 0);
    bytes.extend_from_slice(&(chunks.len() as u64).to_le_bytes());
    for (chunk, offset) in chunks.iter().zip(&offsets) {
        let mut entry = [0_u8; TOC_ENTRY_BYTES];
        entry[0] = chunk.stream as u8;
        entry[1] = chunk.codec;
        // entry[2]：块内过滤器；entry[3]：LOD 层级；entry[4..8]：时间步。v3 均固定为 0。
        entry[8..16].copy_from_slice(&offset.to_le_bytes());
        entry[16..24].copy_from_slice(&(chunk.stored.len() as u64).to_le_bytes());
        entry[24..32].copy_from_slice(&(chunk.record_count as u64).to_le_bytes());
        entry[32..64].copy_from_slice(&chunk.digest);
        bytes.extend_from_slice(&entry);
    }
    let digest = Sha256::digest(&bytes[HEADER_SIZE..]);
    for (chunk, offset) in chunks.iter().zip(&offsets) {
        bytes.resize(*offset as usize, 0);
        bytes.extend_from_slice(&chunk.stored);
    }
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..12].copy_from_slice(&MESH_ASSET_FORMAT_VERSION.to_le_bytes());
    bytes[12] = kind;
    bytes[13] = MILLIMETERS;
    bytes[16..24].copy_from_slice(&((total_len - HEADER_SIZE) as u64).to_le_bytes());
    bytes[24..56].copy_from_slice(&digest);
    Ok((bytes, format!("sha256:{digest:x}")))
}

fn align_up(value: u64) -> Option<u64> {
    value
        .checked_add(CHUNK_ALIGNMENT - 1)
        .map(|value| value / CHUNK_ALIGNMENT * CHUNK_ALIGNMENT)
}

fn map_codec_error(error: io::Error) -> MeshAssetError {
    if error.kind() == io::ErrorKind::OutOfMemory {
        MeshAssetError::AllocationFailed
    } else {
        MeshAssetError::CodecFailed
    }
}

// ---------------------------------------------------------------- 头部与目录

/// 工程清单可登记的 sidecar 版本：当前写入版本，以及仍可只读的旧版本。
pub fn is_readable_mesh_asset_version(version: u32) -> bool {
    // COMPAT(task 011; remove-task 012): 清单中 v2 资产仍可激活，见 v2.rs。
    version == MESH_ASSET_FORMAT_VERSION || version == v2::VERSION
}

/// 从完整固定长度头部验证格式并返回总字节长度，以便流式读取端先拒绝异常文件。
pub fn mesh_asset_encoded_len(header: &[u8]) -> Result<usize, MeshAssetError> {
    if header.len() != HEADER_SIZE || &header[..8] != MAGIC {
        return Err(MeshAssetError::InvalidHeader);
    }
    let version = read_u32_at(header, 8);
    if !matches!(header[12], SURFACE_KIND | TET_KIND) {
        return Err(MeshAssetError::UnsupportedKind(header[12]));
    }
    if header[13] != MILLIMETERS {
        return Err(MeshAssetError::UnsupportedUnits(header[13]));
    }
    match version {
        MESH_ASSET_FORMAT_VERSION => {
            if header[14] != 0 || header[15] != 0 {
                return Err(MeshAssetError::InvalidHeader);
            }
        }
        // COMPAT(task 011; remove-task 012): v2 头部仍可读，见 v2.rs。
        v2::VERSION => v2::check_header(header)?,
        other => return Err(MeshAssetError::UnsupportedVersion(other)),
    }
    let payload_len =
        usize::try_from(read_u64_at(header, 16)).map_err(|_| MeshAssetError::InvalidLength)?;
    HEADER_SIZE
        .checked_add(payload_len)
        .ok_or(MeshAssetError::InvalidLength)
}

fn read_u32_at(bytes: &[u8], offset: usize) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(value)
}

fn read_u64_at(bytes: &[u8], offset: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}

/// 目录字节数（含块数前缀）；`prefix` 为目录开头 8 B。
fn toc_len(prefix: &[u8], total_len: u64) -> Result<usize, MeshAssetError> {
    let count = read_u64_at(prefix, 0);
    let len = count
        .checked_mul(TOC_ENTRY_BYTES as u64)
        .and_then(|len| len.checked_add(TOC_PREFIX_BYTES as u64))
        .ok_or(MeshAssetError::InvalidLength)?;
    if count == 0 || len > total_len.saturating_sub(HEADER_SIZE as u64) {
        return Err(MeshAssetError::InvalidLength);
    }
    usize::try_from(len).map_err(|_| MeshAssetError::InvalidLength)
}

/// 校验目录摘要、条目字段、块边界与流顺序。
fn parse_layout(header: &[u8], toc: &[u8], total_len: u64) -> Result<Layout, MeshAssetError> {
    let digest = Sha256::digest(toc);
    if digest[..] != header[24..56] {
        return Err(MeshAssetError::ChecksumMismatch);
    }
    let kind = header[12];
    let mut chunks = Vec::new();
    let mut cursor = (HEADER_SIZE + toc.len()) as u64;
    let mut previous_stream = None;
    for entry in toc[TOC_PREFIX_BYTES..].chunks(TOC_ENTRY_BYTES) {
        let stream = Stream::from_byte(entry[0]).ok_or(MeshAssetError::UnsupportedLayout)?;
        let stream_allowed = match kind {
            SURFACE_KIND => stream == Stream::SurfaceTriangles,
            _ => stream != Stream::SurfaceTriangles,
        };
        if !stream_allowed || previous_stream.is_some_and(|previous| previous > stream) {
            return Err(MeshAssetError::UnsupportedLayout);
        }
        previous_stream = Some(stream);
        let codec = entry[1];
        if !matches!(codec, CODEC_NONE | CODEC_ZSTD) {
            return Err(MeshAssetError::UnsupportedCodec(codec));
        }
        // 过滤器、LOD 层级与时间步为后续版本预留：v3 只定义无过滤、层级 0、时间步 0。
        if entry[2] != 0 || entry[3] != 0 || read_u32_at(entry, 4) != 0 {
            return Err(MeshAssetError::UnsupportedLayout);
        }
        let offset = read_u64_at(entry, 8);
        let stored_len = read_u64_at(entry, 16);
        let record_count =
            usize::try_from(read_u64_at(entry, 24)).map_err(|_| MeshAssetError::InvalidLength)?;
        let raw_len = record_count
            .checked_mul(stream.record_bytes())
            .ok_or(MeshAssetError::InvalidLength)?;
        let end = offset
            .checked_add(stored_len)
            .ok_or(MeshAssetError::InvalidLength)?;
        if record_count == 0
            || raw_len > CHUNK_RAW_LIMIT_BYTES
            || !offset.is_multiple_of(CHUNK_ALIGNMENT)
            || offset < cursor
            || end > total_len
            || (codec == CODEC_NONE && stored_len != raw_len as u64)
        {
            return Err(MeshAssetError::InvalidLength);
        }
        cursor = end;
        let mut chunk_digest = [0; 32];
        chunk_digest.copy_from_slice(&entry[32..64]);
        chunks.push(ChunkEntry {
            stream,
            codec,
            offset,
            stored_len: usize::try_from(stored_len).map_err(|_| MeshAssetError::InvalidLength)?,
            record_count,
            digest: chunk_digest,
        });
    }
    if cursor != total_len {
        return Err(MeshAssetError::InvalidLength);
    }
    if kind == TET_KIND {
        let counts = chunks
            .iter()
            .filter(|chunk| chunk.stream == Stream::VolumeCounts)
            .map(|chunk| chunk.record_count)
            .sum::<usize>();
        if counts != 1 {
            return Err(MeshAssetError::UnsupportedLayout);
        }
    }
    Ok(Layout {
        kind,
        total_len,
        chunks,
        digest: format!("sha256:{digest:x}"),
    })
}

// ---------------------------------------------------------------- 解码

/// 验证格式、长度与内容摘要后解码，并对所有浮点坐标和体网格连接关系校验。
pub fn decode_mesh_asset(bytes: &[u8]) -> Result<MeshAsset, MeshAssetError> {
    decode_mesh_asset_with_digest(bytes).map(|(mesh, _)| mesh)
}

pub fn decode_mesh_asset_with_digest(bytes: &[u8]) -> Result<(MeshAsset, String), MeshAssetError> {
    if bytes.len() < HEADER_SIZE {
        return Err(MeshAssetError::InvalidHeader);
    }
    let header = &bytes[..HEADER_SIZE];
    let declared_len = mesh_asset_encoded_len(header)?;
    if declared_len != bytes.len() {
        return Err(MeshAssetError::InvalidLength);
    }
    if read_u32_at(header, 8) == v2::VERSION {
        // COMPAT(task 011; remove-task 012): v2 整段解码，见 v2.rs。
        return v2::decode(bytes);
    }
    let total_len = bytes.len() as u64;
    let toc_end = HEADER_SIZE
        + toc_len(
            bytes
                .get(HEADER_SIZE..HEADER_SIZE + TOC_PREFIX_BYTES)
                .ok_or(MeshAssetError::InvalidLength)?,
            total_len,
        )?;
    let layout = parse_layout(header, &bytes[HEADER_SIZE..toc_end], total_len)?;
    let stored: Vec<&[u8]> = layout
        .chunks
        .iter()
        .map(|chunk| {
            let start = chunk.offset as usize;
            &bytes[start..start + chunk.stored_len]
        })
        .collect();
    let mesh = decode_layout(&layout, &stored)?;
    Ok((mesh, layout.digest))
}

/// 已打开的 sidecar：头部与目录已校验，数据块尚未读取。
///
/// 读取端按块顺序批量读入并并行解码，峰值内存约为解码结果加一个批次的压缩块，
/// 而不是整份压缩文件加解码结果；批次之间响应取消。
pub struct MeshAssetReader<R> {
    reader: R,
    state: ReaderState,
}

enum ReaderState {
    Chunked(Layout),
    // COMPAT(task 011; remove-task 012): v2 无目录，只能整段读取后解码。
    LegacyV2 { header: Vec<u8>, total_len: usize },
}

impl<R: Read> MeshAssetReader<R> {
    /// 读取并校验头部（v3 还包括目录），`file_len` 必须与头部声明的总长度一致。
    pub fn open(mut reader: R, file_len: u64) -> Result<Self, MeshAssetError> {
        let mut header = vec![0; HEADER_SIZE];
        read_exact_io(&mut reader, &mut header)?;
        let total_len = mesh_asset_encoded_len(&header)?;
        if total_len as u64 != file_len {
            return Err(MeshAssetError::InvalidLength);
        }
        if read_u32_at(&header, 8) == v2::VERSION {
            return Ok(Self {
                reader,
                state: ReaderState::LegacyV2 { header, total_len },
            });
        }
        let mut toc = vec![0; TOC_PREFIX_BYTES];
        read_exact_io(&mut reader, &mut toc)?;
        let toc_len = toc_len(&toc, file_len)?;
        toc.resize(toc_len, 0);
        read_exact_io(&mut reader, &mut toc[TOC_PREFIX_BYTES..])?;
        let layout = parse_layout(&header, &toc, file_len)?;
        Ok(Self {
            reader,
            state: ReaderState::Chunked(layout),
        })
    }

    /// 资产摘要；v3 打开后即可得到，不需要读取数据块。
    pub fn digest(&self) -> Option<&str> {
        match &self.state {
            ReaderState::Chunked(layout) => Some(&layout.digest),
            ReaderState::LegacyV2 { .. } => None,
        }
    }

    /// 读取并解码全部数据块；`cancelled` 返回 true 时尽快返回 `Ok(None)`。
    pub fn read(
        mut self,
        cancelled: impl Fn() -> bool,
    ) -> Result<Option<(MeshAsset, String)>, MeshAssetError> {
        match self.state {
            ReaderState::LegacyV2 { header, total_len } => {
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(total_len)
                    .map_err(|_| MeshAssetError::AllocationFailed)?;
                bytes.extend_from_slice(&header);
                let mut buffer = vec![0; 256 * 1024];
                while bytes.len() < total_len {
                    if cancelled() {
                        return Ok(None);
                    }
                    let wanted = (total_len - bytes.len()).min(buffer.len());
                    read_exact_io(&mut self.reader, &mut buffer[..wanted])?;
                    bytes.extend_from_slice(&buffer[..wanted]);
                }
                ensure_reader_eof(&mut self.reader)?;
                v2::decode(&bytes).map(Some)
            }
            ReaderState::Chunked(layout) => {
                let mut sink = DecodeSink::new(&layout)?;
                let batch_size = worker_count(layout.chunks.len()) * 2;
                let mut position = (HEADER_SIZE as u64)
                    + (TOC_PREFIX_BYTES + layout.chunks.len() * TOC_ENTRY_BYTES) as u64;
                for (batch_index, batch) in layout.chunks.chunks(batch_size).enumerate() {
                    if cancelled() {
                        return Ok(None);
                    }
                    let mut stored = Vec::with_capacity(batch.len());
                    for chunk in batch {
                        skip_padding(&mut self.reader, chunk.offset - position)?;
                        let mut bytes = Vec::new();
                        bytes
                            .try_reserve_exact(chunk.stored_len)
                            .map_err(|_| MeshAssetError::AllocationFailed)?;
                        bytes.resize(chunk.stored_len, 0);
                        read_exact_io(&mut self.reader, &mut bytes)?;
                        position = chunk.offset + chunk.stored_len as u64;
                        stored.push(bytes);
                    }
                    let slices: Vec<&[u8]> = stored.iter().map(Vec::as_slice).collect();
                    sink.decode_batch(batch_index * batch_size, batch, &slices)?;
                }
                debug_assert_eq!(position, layout.total_len);
                ensure_reader_eof(&mut self.reader)?;
                if cancelled() {
                    return Ok(None);
                }
                let mesh = sink.finish()?;
                Ok(Some((mesh, layout.digest)))
            }
        }
    }
}

fn read_exact_io<R: Read>(reader: &mut R, bytes: &mut [u8]) -> Result<(), MeshAssetError> {
    reader.read_exact(bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            MeshAssetError::InvalidLength
        } else {
            MeshAssetError::Io(error.kind())
        }
    })
}

fn skip_padding<R: Read>(reader: &mut R, len: u64) -> Result<(), MeshAssetError> {
    let len = usize::try_from(len).map_err(|_| MeshAssetError::InvalidLength)?;
    if len >= CHUNK_ALIGNMENT as usize {
        return Err(MeshAssetError::InvalidLength);
    }
    let mut padding = [0; CHUNK_ALIGNMENT as usize];
    read_exact_io(reader, &mut padding[..len])
}

/// 文件在读取期间增长同样视为长度不一致。
fn ensure_reader_eof<R: Read>(reader: &mut R) -> Result<(), MeshAssetError> {
    let mut extra = [0; 1];
    match reader.read(&mut extra) {
        Ok(0) => Ok(()),
        Ok(_) => Err(MeshAssetError::InvalidLength),
        Err(error) => Err(MeshAssetError::Io(error.kind())),
    }
}

fn decode_layout(layout: &Layout, stored: &[&[u8]]) -> Result<MeshAsset, MeshAssetError> {
    let mut sink = DecodeSink::new(layout)?;
    sink.decode_batch(0, &layout.chunks, stored)?;
    sink.finish()
}

/// 解码目标：表面网格直接写入预分配的三角形数组；体网格按流收集原始记录。
enum DecodeSink {
    Surface {
        triangles: Vec<[[f64; 3]; 3]>,
        /// 第 i 块在 `triangles` 中的起始下标。
        starts: Vec<usize>,
    },
    Volume {
        raw: [Vec<u8>; 4],
    },
}

impl DecodeSink {
    fn new(layout: &Layout) -> Result<Self, MeshAssetError> {
        if layout.kind == SURFACE_KIND {
            let mut starts = Vec::with_capacity(layout.chunks.len());
            let mut total = 0_usize;
            for chunk in &layout.chunks {
                starts.push(total);
                total = total
                    .checked_add(chunk.record_count)
                    .ok_or(MeshAssetError::SizeOverflow)?;
            }
            let mut triangles = Vec::new();
            triangles
                .try_reserve_exact(total)
                .map_err(|_| MeshAssetError::AllocationFailed)?;
            triangles.resize(total, [[0.0; 3]; 3]);
            Ok(Self::Surface { triangles, starts })
        } else {
            Ok(Self::Volume {
                raw: Default::default(),
            })
        }
    }

    /// `first` 为本批第一块在目录中的下标。
    fn decode_batch(
        &mut self,
        first: usize,
        chunks: &[ChunkEntry],
        stored: &[&[u8]],
    ) -> Result<(), MeshAssetError> {
        match self {
            Self::Surface { triangles, starts } => {
                let begin = starts[first];
                let end = begin + chunks.iter().map(|chunk| chunk.record_count).sum::<usize>();
                let mut targets = Vec::with_capacity(chunks.len());
                let mut rest = &mut triangles[begin..end];
                for chunk in chunks {
                    let (head, tail) = rest.split_at_mut(chunk.record_count);
                    targets.push(std::sync::Mutex::new(head));
                    rest = tail;
                }
                parallel_map(chunks.len(), |index| {
                    let raw = chunk_raw(&chunks[index], stored[index])?;
                    let mut target = targets[index]
                        .lock()
                        .map_err(|_| MeshAssetError::CodecFailed)?;
                    decode_surface_records(&raw, &mut target)
                })?;
                Ok(())
            }
            Self::Volume { raw } => {
                let decoded = parallel_map(chunks.len(), |index| {
                    chunk_raw(&chunks[index], stored[index])
                })?;
                for (chunk, bytes) in chunks.iter().zip(decoded) {
                    let target = &mut raw[chunk.stream as usize - Stream::VolumeCounts as usize];
                    target
                        .try_reserve(bytes.len())
                        .map_err(|_| MeshAssetError::AllocationFailed)?;
                    target.extend_from_slice(&bytes);
                }
                Ok(())
            }
        }
    }

    fn finish(self) -> Result<MeshAsset, MeshAssetError> {
        match self {
            Self::Surface { triangles, .. } => Ok(MeshAsset::Surface(SurfaceMesh { triangles })),
            Self::Volume { raw } => decode_volume(&raw).map(MeshAsset::Volume),
        }
    }
}

/// 校验块摘要并解压，结果长度恰为 `record_count × record_bytes`。
fn chunk_raw(chunk: &ChunkEntry, stored: &[u8]) -> Result<Vec<u8>, MeshAssetError> {
    if stored.len() != chunk.stored_len || Sha256::digest(stored)[..] != chunk.digest {
        return Err(MeshAssetError::ChecksumMismatch);
    }
    let raw_len = chunk.raw_len();
    match chunk.codec {
        CODEC_ZSTD => {
            let mut raw = Vec::new();
            raw.try_reserve_exact(raw_len)
                .map_err(|_| MeshAssetError::AllocationFailed)?;
            raw.resize(raw_len, 0);
            let written = zstd::bulk::decompress_to_buffer(stored, &mut raw)
                .map_err(|_| MeshAssetError::CodecFailed)?;
            if written != raw_len {
                return Err(MeshAssetError::InvalidLength);
            }
            Ok(raw)
        }
        _ => Ok(stored.to_vec()),
    }
}

fn decode_surface_records(
    raw: &[u8],
    triangles: &mut [[[f64; 3]; 3]],
) -> Result<(), MeshAssetError> {
    let (records, rest) = raw.as_chunks::<72>();
    if !rest.is_empty() || records.len() != triangles.len() {
        return Err(MeshAssetError::InvalidLength);
    }
    for (record, triangle) in records.iter().zip(triangles) {
        let (values, _) = record.as_chunks::<8>();
        for (value, coordinate) in values.iter().zip(triangle.iter_mut().flatten()) {
            *coordinate = f64::from_le_bytes(*value);
            if !coordinate.is_finite() {
                return Err(MeshAssetError::InvalidMesh);
            }
        }
    }
    Ok(())
}

fn decode_volume(raw: &[Vec<u8>; 4]) -> Result<TetMesh, MeshAssetError> {
    let [counts, nodes, tets, boundary] = raw;
    let (counts, _) = counts.as_chunks::<4>();
    let [region_count, boundary_group_count] = [0, 1].map(|index| {
        counts
            .get(index)
            .map(|value| u32::from_le_bytes(*value))
            .unwrap_or_default()
    });
    let (node_records, _) = nodes.as_chunks::<24>();
    let mut decoded_nodes = Vec::new();
    decoded_nodes
        .try_reserve_exact(node_records.len())
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    for record in node_records {
        let (values, _) = record.as_chunks::<8>();
        let node = [0, 1, 2].map(|axis| f64::from_le_bytes(values[axis]));
        if node.iter().any(|coordinate| !coordinate.is_finite()) {
            return Err(MeshAssetError::InvalidMesh);
        }
        decoded_nodes.push(node);
    }
    let (tet_records, _) = tets.as_chunks::<20>();
    let mut decoded_tets = Vec::new();
    decoded_tets
        .try_reserve_exact(tet_records.len())
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    for record in tet_records {
        let (values, _) = record.as_chunks::<4>();
        decoded_tets.push(Tetrahedron {
            nodes: [0, 1, 2, 3].map(|slot| u32::from_le_bytes(values[slot])),
            region: u32::from_le_bytes(values[4]),
        });
    }
    let (boundary_records, _) = boundary.as_chunks::<16>();
    let mut decoded_boundary = Vec::new();
    decoded_boundary
        .try_reserve_exact(boundary_records.len())
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    for record in boundary_records {
        let (values, _) = record.as_chunks::<4>();
        decoded_boundary.push(SurfaceTriangle {
            nodes: [0, 1, 2].map(|slot| u32::from_le_bytes(values[slot])),
            group: u32::from_le_bytes(values[3]),
        });
    }
    let mesh = TetMesh {
        nodes: decoded_nodes,
        tets: decoded_tets,
        boundary: decoded_boundary,
        region_count,
        boundary_group_count,
    };
    if !validate_tet_mesh(&mesh).is_ok() {
        return Err(MeshAssetError::InvalidMesh);
    }
    Ok(mesh)
}

// ---------------------------------------------------------------- 并行

fn worker_count(jobs: usize) -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(jobs)
        .max(1)
}

/// 用作用域线程按下标并行执行 `job`，结果保持下标顺序；任一失败返回首个错误。
fn parallel_map<T, F>(jobs: usize, job: F) -> Result<Vec<T>, MeshAssetError>
where
    T: Send,
    F: Fn(usize) -> Result<T, MeshAssetError> + Sync,
{
    let workers = worker_count(jobs);
    if workers <= 1 {
        return (0..jobs).map(&job).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let job = &job;
    let next = &next;
    let mut results: Vec<(usize, Result<T, MeshAssetError>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(move || {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if index >= jobs {
                            break done;
                        }
                        let result = job(index);
                        let failed = result.is_err();
                        done.push((index, result));
                        if failed {
                            next.store(jobs, std::sync::atomic::Ordering::Relaxed);
                            break done;
                        }
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect()
    });
    results.sort_by_key(|(index, _)| *index);
    if results.len() != jobs {
        // 某个 worker 失败后提前停止；返回已记录的错误。
        return Err(results
            .into_iter()
            .find_map(|(_, result)| result.err())
            .unwrap_or(MeshAssetError::CodecFailed));
    }
    results.into_iter().map(|(_, result)| result).collect()
}

#[cfg(test)]
mod tests;
