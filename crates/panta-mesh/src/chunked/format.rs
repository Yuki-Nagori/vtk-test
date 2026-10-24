//! 分块体网格单文件格式（task 027）。
//!
//! 布局：`头 16 B | 单元段（各块压缩单元，按 Morton 顺序连续）| 场段（v2，task 029）|
//! 表面段（各块外表面 + 简化外表面，连续）| 块表（zstd）| 尾 32 B`。
//! 场段按场排列：每个场先是各块的块内值（节点场按块内局部节点顺序、单元场按块内单元顺序），
//! 再是各块外表面三角形上的值（节点场每三角形 3 个顶点元组，单元场每三角形 1 个所属单元元组），
//! 都按 Morton 顺序连续，因此“选中一个场”只需一次读入它的外表面值区段，命中块的块内值可合并读。打开时一次读尾部窗口拿到块表，再一次大块读入整个表面段；
//! 单元按需读，相邻块合并成大块顺序读（见 [`crate::chunked::ChunkCache`]）。
//! 所有整数小端；每个压缩段都是带校验和的 zstd 帧。
//!
//! v3（task 030）：分块以原始单元为单位（混合单元整体落在同一块），块表与块头加原始单元数；
//! 块内每个拆分四面体的 `global_cells` 记原始单元 ID（非降序，同一单元的拆分四面体相邻），
//! 混合块另存块内原始单元布局（类型 + 局部节点连接），供逐类型质量与单元场使用；
//! 单元场按块内原始单元顺序存值。v2 文件视为无效，派生缓存会自动重建。

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::ChunkError;
use crate::mapped::MappedFile;
use crate::{CellType, SurfaceMesh, Tetrahedron};

pub(crate) const HEAD_MAGIC: &[u8; 8] = b"PANTAOC1";
pub(crate) const TAIL_MAGIC: &[u8; 8] = b"PANTAOCE";
/// v2（task 029）：加入场段与文件级场表；v3（task 030）：原始单元 ID 与混合单元布局。
/// 旧版本文件视为无效，派生缓存会重建。
pub(crate) const FORMAT_VERSION: u32 = 3;
pub(crate) const HEAD_LEN: u64 = 16;
pub(crate) const TAIL_LEN: u64 = 32;
/// 打开时一次读入的尾部窗口；块表不超过它时打开只需一次读。
const TAIL_WINDOW: u64 = 1 << 20;
/// 每块占据体素的每轴分辨率（粗级别代理）。
pub const VOXELS_PER_AXIS: usize = 16;
const VOXEL_BYTES: usize = VOXELS_PER_AXIS * VOXELS_PER_AXIS * VOXELS_PER_AXIS / 8;

/// 块源读取结果：定位读为自有缓冲，映射为文件映射上的零拷贝切片。
pub enum ChunkBytes {
    Owned(Vec<u8>),
    Mapped {
        file: Arc<MappedFile>,
        start: usize,
        len: usize,
    },
}

impl std::ops::Deref for ChunkBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            // 构造时已校验区间在映射内。
            Self::Mapped { file, start, len } => &file.bytes()[*start..*start + *len],
        }
    }
}

/// 块源 IO 计数（基准报告用）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IoCounts {
    /// 读系统调用次数（映射实现为 0）。
    pub reads: u64,
    /// 读取或访问的字节数。
    pub bytes: u64,
    /// 打开的文件数。
    pub files: u64,
    /// 映射区间访问次数（定位读实现为 0）；实际缺页 IO 由 OS 决定，不在这里计数。
    pub mapped: u64,
}

/// 按字节区间读取的块源。实现必须线程安全。
pub trait ChunkSource: Send + Sync {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// 读取 `[offset, offset + len)`；越界返回 `Format` 错误。
    fn read_at(&self, offset: u64, len: usize) -> Result<ChunkBytes, ChunkError>;
    fn io_counts(&self) -> IoCounts;
    /// 实现名（`mmap` / `pread`），用于诊断与基准。
    fn kind(&self) -> &'static str;
}

fn check_range(offset: u64, len: usize, file_len: u64) -> Result<(), ChunkError> {
    if offset
        .checked_add(len as u64)
        .is_none_or(|end| end > file_len)
    {
        return Err(ChunkError::Format(format!(
            "range {offset}+{len} beyond file length {file_len}"
        )));
    }
    Ok(())
}

/// 块读取实现选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChunkReadMode {
    /// 优先映射，映射失败（锁冲突、平台不支持等）时回退定位读。
    #[default]
    Auto,
    /// 只用映射。
    Mmap,
    /// 只用合并定位读。
    Read,
}

/// 按 `mode` 打开单文件块源。
pub fn open_chunk_source(
    path: &Path,
    mode: ChunkReadMode,
) -> Result<Arc<dyn ChunkSource>, ChunkError> {
    match mode {
        ChunkReadMode::Read => Ok(Arc::new(FileChunkSource::open(path)?)),
        ChunkReadMode::Mmap => Ok(Arc::new(MmapChunkSource::open(path)?)),
        ChunkReadMode::Auto => match MmapChunkSource::open(path) {
            Ok(source) => Ok(Arc::new(source)),
            Err(_) => Ok(Arc::new(FileChunkSource::open(path)?)),
        },
    }
}

/// 映射块源：整文件只读映射（见 [`crate::mapped`]），读取为零拷贝切片；合并后的区段先提示内核预读。
pub struct MmapChunkSource {
    file: Arc<MappedFile>,
    accesses: AtomicU64,
    bytes: AtomicU64,
}

impl MmapChunkSource {
    pub fn open(path: &Path) -> Result<Self, ChunkError> {
        Ok(Self {
            file: Arc::new(MappedFile::open(path)?),
            accesses: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
        })
    }
}

impl ChunkSource for MmapChunkSource {
    fn len(&self) -> u64 {
        self.file.len() as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<ChunkBytes, ChunkError> {
        check_range(offset, len, self.len())?;
        let start = offset as usize;
        self.file.will_need(start, len);
        self.accesses.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(len as u64, Ordering::Relaxed);
        Ok(ChunkBytes::Mapped {
            file: self.file.clone(),
            start,
            len,
        })
    }

    fn io_counts(&self) -> IoCounts {
        IoCounts {
            reads: 0,
            bytes: self.bytes.load(Ordering::Relaxed),
            files: 1,
            mapped: self.accesses.load(Ordering::Relaxed),
        }
    }

    fn kind(&self) -> &'static str {
        "mmap"
    }
}

/// 单文件块源：打开一次，之后用定位读（Unix `pread`，Windows `seek_read`），不移动共享游标，
/// 多线程并发读不加锁。映射不可用时的回退实现。
pub struct FileChunkSource {
    file: File,
    len: u64,
    reads: AtomicU64,
    bytes: AtomicU64,
}

impl FileChunkSource {
    pub fn open(path: &Path) -> Result<Self, ChunkError> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            file,
            len,
            reads: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
        })
    }
}

impl ChunkSource for FileChunkSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<ChunkBytes, ChunkError> {
        check_range(offset, len, self.len)?;
        let mut buffer = vec![0; len];
        positioned_read(&self.file, offset, &mut buffer)?;
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(len as u64, Ordering::Relaxed);
        Ok(ChunkBytes::Owned(buffer))
    }

    fn io_counts(&self) -> IoCounts {
        IoCounts {
            reads: self.reads.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            files: 1,
            mapped: 0,
        }
    }

    fn kind(&self) -> &'static str {
        "pread"
    }
}

#[cfg(unix)]
fn positioned_read(file: &File, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buffer, offset)
}

#[cfg(windows)]
fn positioned_read(file: &File, mut offset: u64, mut buffer: &mut [u8]) -> std::io::Result<()> {
    while !buffer.is_empty() {
        let read = std::os::windows::fs::FileExt::seek_read(file, buffer, offset)?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        offset += read as u64;
        buffer = &mut buffer[read..];
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn positioned_read(file: &File, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut handle = file.try_clone()?;
    handle.seek(SeekFrom::Start(offset))?;
    handle.read_exact(buffer)
}

/// 块表中一块的元数据（打开时常驻）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkHeader {
    /// 节点包围盒 `[min, max]`。
    pub bounds: [[f64; 3]; 2],
    /// 块内拆分四面体数。
    pub tet_count: u32,
    pub node_count: u32,
    /// 块内原始单元数（纯四面体块等于 `tet_count`）；单元场按它计元组。
    pub cell_count: u32,
    pub(crate) cells_offset: u64,
    pub(crate) cells_len: u32,
    pub(crate) cells_raw_len: u32,
    pub(crate) surface_offset: u64,
    pub(crate) surface_len: u32,
    pub(crate) surface_raw_len: u32,
    /// 16³ 占据位图，体素下标 `(k * 16 + j) * 16 + i`。
    pub(crate) voxels: Vec<u8>,
    /// 场值范围（min，max），按文件级场表顺序：标量取值、矢量取模长，忽略缺失值；
    /// 块内没有有限值时为 `[NaN, NaN]`。
    pub field_ranges: Vec<[f64; 2]>,
    /// 各场在场段中的区间，与 `field_ranges` 同序。
    pub(crate) field_blocks: Vec<FieldBlock>,
}

/// 一块一个场的两个压缩区段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FieldBlock {
    pub values_offset: u64,
    pub values_len: u32,
    pub values_raw_len: u32,
    pub boundary_offset: u64,
    pub boundary_len: u32,
    pub boundary_raw_len: u32,
}

impl FieldBlock {
    pub fn values_range(&self) -> (u64, u64) {
        (
            self.values_offset,
            self.values_offset + u64::from(self.values_len),
        )
    }

    pub fn boundary_range(&self) -> (u64, u64) {
        (
            self.boundary_offset,
            self.boundary_offset + u64::from(self.boundary_len),
        )
    }
}

/// 文件级场表的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkFieldInfo {
    pub name: String,
    pub association: crate::FieldAssociation,
    pub components: u8,
}

impl ChunkHeader {
    pub fn voxel_occupied(&self, index: usize) -> bool {
        self.voxels
            .get(index / 8)
            .is_some_and(|byte| byte & (1 << (index % 8)) != 0)
    }

    pub(crate) fn mark_voxel(&mut self, index: usize) {
        if let Some(byte) = self.voxels.get_mut(index / 8) {
            *byte |= 1 << (index % 8);
        }
    }

    pub(crate) fn empty(bounds: [[f64; 3]; 2], tet_count: u32, node_count: u32) -> Self {
        Self {
            bounds,
            tet_count,
            node_count,
            cell_count: tet_count,
            cells_offset: 0,
            cells_len: 0,
            cells_raw_len: 0,
            surface_offset: 0,
            surface_len: 0,
            surface_raw_len: 0,
            voxels: vec![0; VOXEL_BYTES],
            field_ranges: Vec::new(),
            field_blocks: Vec::new(),
        }
    }

    pub(crate) fn cells_range(&self) -> (u64, u64) {
        (
            self.cells_offset,
            self.cells_offset + u64::from(self.cells_len),
        )
    }
}

/// 解码后的一块单元：节点、四面体用块内局部下标；全局 ID 与原始顺序一致（升序）。
/// `global_cells` 是每个拆分四面体所属的原始单元 ID（纯四面体时严格升序，混合时非降序）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkCells {
    pub global_nodes: Vec<u64>,
    pub nodes: Vec<[f64; 3]>,
    pub tets: Vec<Tetrahedron>,
    pub global_cells: Vec<u64>,
    /// 混合块的原始单元布局；纯四面体块为 `None`（块内单元即四面体）。
    pub layout: Option<ChunkCellLayout>,
}

/// 块内原始单元布局（task 030）：连接用块内局部节点下标，`tet_cells` 为各拆分四面体的块内单元下标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkCellLayout {
    pub types: Vec<CellType>,
    pub offsets: Vec<u32>,
    pub connectivity: Vec<u32>,
    pub tet_cells: Vec<u32>,
}

impl ChunkCellLayout {
    pub fn cell_nodes(&self, cell: usize) -> &[u32] {
        &self.connectivity[self.offsets[cell] as usize..self.offsets[cell + 1] as usize]
    }
}

impl ChunkCells {
    /// 块内原始单元数。
    pub fn cell_count(&self) -> usize {
        self.layout
            .as_ref()
            .map_or(self.tets.len(), |layout| layout.types.len())
    }

    /// 第 `tet` 个拆分四面体的块内原始单元下标。
    pub fn tet_cell(&self, tet: usize) -> usize {
        self.layout
            .as_ref()
            .map_or(tet, |layout| layout.tet_cells[tet] as usize)
    }

    /// 驻留字节数估算（缓存预算用）。
    pub fn resident_bytes(&self) -> usize {
        self.global_nodes.len() * 8
            + self.nodes.len() * 24
            + self.tets.len() * std::mem::size_of::<Tetrahedron>()
            + self.global_cells.len() * 8
            + self.layout.as_ref().map_or(0, |layout| {
                layout.types.len() * 5 + layout.connectivity.len() * 4 + layout.tet_cells.len() * 4
            })
    }
}

/// 一块的常驻表面：外表面三角形（含边界组）与简化外表面。
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkSurface {
    pub boundary: Vec<[[f64; 3]; 3]>,
    pub groups: Vec<u32>,
    pub simplified: SurfaceMesh,
}

/// 外表面值元组数：节点场每三角形 3 个，单元场 1 个。
pub(crate) fn boundary_tuple_count(
    triangles: usize,
    association: crate::FieldAssociation,
) -> usize {
    match association {
        crate::FieldAssociation::Node => triangles * 3,
        crate::FieldAssociation::Cell => triangles,
    }
}

/// 解码 f64 小端数组（长度已由块表校验）。
pub(crate) fn decode_f64s(raw: &[u8]) -> Result<Vec<f64>, ChunkError> {
    if !raw.len().is_multiple_of(8) {
        return Err(ChunkError::Format("value block length".to_owned()));
    }
    Ok(raw
        .as_chunks::<8>()
        .0
        .iter()
        .map(|bytes| f64::from_le_bytes(*bytes))
        .collect())
}

/// 文件级块表。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChunkTable {
    pub tet_count: u64,
    pub node_count: u64,
    /// 原始单元数（纯四面体等于 `tet_count`）。
    pub cell_count: u64,
    pub fields: Vec<ChunkFieldInfo>,
    pub surface_offset: u64,
    pub surface_len: u64,
    pub chunks: Vec<ChunkHeader>,
}

pub(crate) fn compress(raw: &[u8]) -> Result<Vec<u8>, ChunkError> {
    let mut compressor = zstd::bulk::Compressor::new(1)?;
    compressor.include_checksum(true)?;
    Ok(compressor.compress(raw)?)
}

pub(crate) fn decompress(data: &[u8], raw_len: usize) -> Result<Vec<u8>, ChunkError> {
    let raw = zstd::bulk::decompress(data, raw_len)
        .map_err(|error| ChunkError::Format(format!("zstd: {error}")))?;
    if raw.len() != raw_len {
        return Err(ChunkError::Format(
            "decompressed length mismatch".to_owned(),
        ));
    }
    Ok(raw)
}

/// 小端写入器。
#[derive(Default)]
pub(crate) struct Writer(pub Vec<u8>);

impl Writer {
    pub fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    pub fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    pub fn f64(&mut self, value: f64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    pub fn point(&mut self, point: [f64; 3]) {
        point.iter().for_each(|&value| self.f64(value));
    }
}

/// 小端读取器，越界返回格式错误。
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    fn take<const N: usize>(&mut self) -> Result<[u8; N], ChunkError> {
        let bytes = self
            .data
            .get(self.at..self.at + N)
            .ok_or_else(|| ChunkError::Format("truncated".to_owned()))?;
        self.at += N;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }
    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8], ChunkError> {
        let bytes = self
            .data
            .get(self.at..self.at + len)
            .ok_or_else(|| ChunkError::Format("truncated".to_owned()))?;
        self.at += len;
        Ok(bytes)
    }
    pub fn u32(&mut self) -> Result<u32, ChunkError> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    pub fn u64(&mut self) -> Result<u64, ChunkError> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    pub fn f64(&mut self) -> Result<f64, ChunkError> {
        Ok(f64::from_le_bytes(self.take()?))
    }
    pub fn point(&mut self) -> Result<[f64; 3], ChunkError> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }
    /// 声明的元素数不能超过剩余字节可容纳的数量，防止恶意长度触发超大分配。
    pub fn count(&mut self, element_bytes: usize) -> Result<usize, ChunkError> {
        let count = self.u32()? as usize;
        if count.saturating_mul(element_bytes) > self.data.len() - self.at {
            return Err(ChunkError::Format("count exceeds data".to_owned()));
        }
        Ok(count)
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.at
    }
    pub fn finish(&self) -> Result<(), ChunkError> {
        if self.at == self.data.len() {
            Ok(())
        } else {
            Err(ChunkError::Format("trailing bytes".to_owned()))
        }
    }
}

pub(crate) fn encode_cells(cells: &ChunkCells) -> Vec<u8> {
    let mut out = Writer::default();
    out.u32(cells.nodes.len() as u32);
    out.u32(cells.tets.len() as u32);
    // 全局 ID 升序，差分后 zstd 压缩率高。
    let mut previous = 0;
    for &id in &cells.global_nodes {
        out.u64(id - previous);
        previous = id;
    }
    cells.nodes.iter().for_each(|&point| out.point(point));
    for tet in &cells.tets {
        tet.nodes.iter().for_each(|&node| out.u32(node));
    }
    cells.tets.iter().for_each(|tet| out.u32(tet.region));
    let mut previous = 0;
    for &id in &cells.global_cells {
        out.u64(id - previous);
        previous = id;
    }
    // v3：混合块布局；0 表示纯四面体块。
    match &cells.layout {
        None => out.u32(0),
        Some(layout) => {
            out.u32(layout.types.len() as u32);
            out.0
                .extend(layout.types.iter().map(|kind| kind.vtk_code()));
            layout.connectivity.iter().for_each(|&node| out.u32(node));
        }
    }
    out.0
}

pub(crate) fn decode_cells(raw: &[u8]) -> Result<ChunkCells, ChunkError> {
    let mut reader = Reader::new(raw);
    let node_count = reader.count(32)?;
    let tet_count = reader.count(28)?;
    let mut global_nodes = Vec::with_capacity(node_count);
    let mut previous = 0u64;
    for _ in 0..node_count {
        previous = previous
            .checked_add(reader.u64()?)
            .ok_or_else(|| ChunkError::Format("node id overflow".to_owned()))?;
        global_nodes.push(previous);
    }
    let nodes = (0..node_count)
        .map(|_| reader.point())
        .collect::<Result<Vec<_>, _>>()?;
    let mut tets = Vec::with_capacity(tet_count);
    for _ in 0..tet_count {
        let mut corner = [0; 4];
        for slot in &mut corner {
            *slot = reader.u32()?;
            if *slot as usize >= node_count {
                return Err(ChunkError::Format("tet node out of range".to_owned()));
            }
        }
        tets.push(Tetrahedron {
            nodes: corner,
            region: 0,
        });
    }
    for tet in &mut tets {
        tet.region = reader.u32()?;
    }
    let mut global_cells = Vec::with_capacity(tet_count);
    let mut previous = 0u64;
    for _ in 0..tet_count {
        previous = previous
            .checked_add(reader.u64()?)
            .ok_or_else(|| ChunkError::Format("cell id overflow".to_owned()))?;
        global_cells.push(previous);
    }
    let layout = match reader.count(1)? {
        0 => None,
        cell_count => Some(decode_layout(
            &mut reader,
            cell_count,
            node_count,
            &global_cells,
        )?),
    };
    reader.finish()?;
    Ok(ChunkCells {
        global_nodes,
        nodes,
        tets,
        global_cells,
        layout,
    })
}

/// 解码混合块布局；拆分四面体按原始单元 ID 成段排列，段数必须等于单元数。
fn decode_layout(
    reader: &mut Reader<'_>,
    cell_count: usize,
    node_count: usize,
    global_cells: &[u64],
) -> Result<ChunkCellLayout, ChunkError> {
    let types = reader
        .bytes(cell_count)?
        .iter()
        .map(|&code| {
            CellType::from_vtk(code)
                .ok_or_else(|| ChunkError::Format(format!("cell type {code} unknown")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut offsets = Vec::with_capacity(cell_count + 1);
    offsets.push(0u32);
    let mut total = 0usize;
    for kind in &types {
        total += kind
            .fixed_node_count()
            .ok_or_else(|| ChunkError::Format("polyhedron cells unsupported".to_owned()))?;
        offsets.push(
            u32::try_from(total)
                .map_err(|_| ChunkError::Format("cell connectivity too long".to_owned()))?,
        );
    }
    if total.saturating_mul(4) > reader.remaining() {
        return Err(ChunkError::Format("count exceeds data".to_owned()));
    }
    let mut connectivity = Vec::with_capacity(total);
    for _ in 0..total {
        let node = reader.u32()?;
        if node as usize >= node_count {
            return Err(ChunkError::Format("cell node out of range".to_owned()));
        }
        connectivity.push(node);
    }
    let mut tet_cells = Vec::with_capacity(global_cells.len());
    let mut cell = 0u32;
    for (index, &id) in global_cells.iter().enumerate() {
        if index > 0 && id != global_cells[index - 1] {
            cell += 1;
        }
        tet_cells.push(cell);
    }
    if global_cells.is_empty() || cell as usize + 1 != cell_count {
        return Err(ChunkError::Format(
            "cell layout does not match tet owners".to_owned(),
        ));
    }
    Ok(ChunkCellLayout {
        types,
        offsets,
        connectivity,
        tet_cells,
    })
}

pub(crate) fn encode_surface(surface: &ChunkSurface) -> Vec<u8> {
    let mut out = Writer::default();
    out.u32(surface.boundary.len() as u32);
    out.u32(surface.simplified.triangles.len() as u32);
    for (triangle, &group) in surface.boundary.iter().zip(&surface.groups) {
        triangle.iter().for_each(|&point| out.point(point));
        out.u32(group);
    }
    for triangle in &surface.simplified.triangles {
        triangle.iter().for_each(|&point| out.point(point));
    }
    out.0
}

pub(crate) fn decode_surface(raw: &[u8]) -> Result<ChunkSurface, ChunkError> {
    let mut reader = Reader::new(raw);
    let boundary_count = reader.count(76)?;
    let simplified_count = reader.u32()? as usize;
    let mut boundary = Vec::with_capacity(boundary_count);
    let mut groups = Vec::with_capacity(boundary_count);
    for _ in 0..boundary_count {
        boundary.push([reader.point()?, reader.point()?, reader.point()?]);
        groups.push(reader.u32()?);
    }
    if simplified_count.saturating_mul(72) > raw.len() {
        return Err(ChunkError::Format("count exceeds data".to_owned()));
    }
    let triangles = (0..simplified_count)
        .map(|_| Ok([reader.point()?, reader.point()?, reader.point()?]))
        .collect::<Result<Vec<_>, ChunkError>>()?;
    reader.finish()?;
    Ok(ChunkSurface {
        boundary,
        groups,
        simplified: SurfaceMesh { triangles },
    })
}

pub(crate) fn encode_table(table: &ChunkTable) -> Vec<u8> {
    let mut out = Writer::default();
    out.u64(table.tet_count);
    out.u64(table.node_count);
    out.u64(table.cell_count);
    out.u32(table.chunks.len() as u32);
    out.u32(table.fields.len() as u32);
    out.u64(table.surface_offset);
    out.u64(table.surface_len);
    for field in &table.fields {
        out.u32(field.name.len() as u32);
        out.0.extend_from_slice(field.name.as_bytes());
        out.u32(match field.association {
            crate::FieldAssociation::Node => 0,
            crate::FieldAssociation::Cell => 1,
        });
        out.u32(u32::from(field.components));
    }
    for chunk in &table.chunks {
        chunk.bounds.iter().for_each(|&point| out.point(point));
        out.u32(chunk.tet_count);
        out.u32(chunk.node_count);
        out.u32(chunk.cell_count);
        out.u64(chunk.cells_offset);
        out.u32(chunk.cells_len);
        out.u32(chunk.cells_raw_len);
        out.u64(chunk.surface_offset);
        out.u32(chunk.surface_len);
        out.u32(chunk.surface_raw_len);
        out.0.extend_from_slice(&chunk.voxels);
        for (range, block) in chunk.field_ranges.iter().zip(&chunk.field_blocks) {
            out.f64(range[0]);
            out.f64(range[1]);
            out.u64(block.values_offset);
            out.u32(block.values_len);
            out.u32(block.values_raw_len);
            out.u64(block.boundary_offset);
            out.u32(block.boundary_len);
            out.u32(block.boundary_raw_len);
        }
    }
    out.0
}

pub(crate) fn decode_table(raw: &[u8]) -> Result<ChunkTable, ChunkError> {
    let mut reader = Reader::new(raw);
    let tet_count = reader.u64()?;
    let node_count = reader.u64()?;
    let cell_count = reader.u64()?;
    let entry_floor = 48 + 44 + VOXEL_BYTES;
    let chunk_count = reader.count(entry_floor)?;
    let field_count = reader.count(12)?;
    let surface_offset = reader.u64()?;
    let surface_len = reader.u64()?;
    let mut fields = Vec::with_capacity(field_count);
    for _ in 0..field_count {
        let name_len = reader.count(1)?;
        let name = std::str::from_utf8(reader.bytes(name_len)?)
            .map_err(|_| ChunkError::Format("field name is not UTF-8".to_owned()))?
            .to_owned();
        let association = match reader.u32()? {
            0 => crate::FieldAssociation::Node,
            1 => crate::FieldAssociation::Cell,
            other => {
                return Err(ChunkError::Format(format!(
                    "field association {other} unknown"
                )));
            }
        };
        let components = match reader.u32()? {
            value @ (1 | 3) => value as u8,
            other => {
                return Err(ChunkError::Format(format!(
                    "field components {other} unsupported"
                )));
            }
        };
        fields.push(ChunkFieldInfo {
            name,
            association,
            components,
        });
    }
    let mut chunks = Vec::with_capacity(chunk_count);
    for _ in 0..chunk_count {
        let bounds = [reader.point()?, reader.point()?];
        let mut chunk = ChunkHeader::empty(bounds, reader.u32()?, reader.u32()?);
        chunk.cell_count = reader.u32()?;
        if chunk.cell_count > chunk.tet_count || (chunk.cell_count == 0) != (chunk.tet_count == 0) {
            return Err(ChunkError::Format("chunk cell count invalid".to_owned()));
        }
        chunk.cells_offset = reader.u64()?;
        chunk.cells_len = reader.u32()?;
        chunk.cells_raw_len = reader.u32()?;
        chunk.surface_offset = reader.u64()?;
        chunk.surface_len = reader.u32()?;
        chunk.surface_raw_len = reader.u32()?;
        chunk.voxels = reader.bytes(VOXEL_BYTES)?.to_vec();
        if field_count.saturating_mul(48) > raw.len() {
            return Err(ChunkError::Format("field count exceeds data".to_owned()));
        }
        for _ in 0..field_count {
            chunk.field_ranges.push([reader.f64()?, reader.f64()?]);
            chunk.field_blocks.push(FieldBlock {
                values_offset: reader.u64()?,
                values_len: reader.u32()?,
                values_raw_len: reader.u32()?,
                boundary_offset: reader.u64()?,
                boundary_len: reader.u32()?,
                boundary_raw_len: reader.u32()?,
            });
        }
        if u64::from(chunk.surface_len) + chunk.surface_offset > surface_len {
            return Err(ChunkError::Format(
                "surface range out of section".to_owned(),
            ));
        }
        chunks.push(chunk);
    }
    reader.finish()?;
    let (tets, cells) = chunks.iter().fold((0u64, 0u64), |(tets, cells), chunk| {
        (
            tets + u64::from(chunk.tet_count),
            cells + u64::from(chunk.cell_count),
        )
    });
    if tets != tet_count || cells != cell_count {
        return Err(ChunkError::Format("table counts mismatch".to_owned()));
    }
    Ok(ChunkTable {
        tet_count,
        node_count,
        cell_count,
        fields,
        surface_offset,
        surface_len,
        chunks,
    })
}

pub(crate) fn encode_tail(table_offset: u64, table_len: u64, table_raw_len: u64) -> Vec<u8> {
    let mut out = Writer::default();
    out.u64(table_offset);
    out.u64(table_len);
    out.u32(table_raw_len as u32);
    out.u32(FORMAT_VERSION);
    out.0.extend_from_slice(TAIL_MAGIC);
    out.0
}

/// 打开：一次读尾部窗口（块表通常在窗口内），一次 16 B 读校验文件头（小文件在窗口内则免），
/// 必要时再读一次块表；然后一次读入整个表面段。长度、头尾、块表与各块区间都在这里校验。返回块表与各块常驻表面。
pub(crate) fn read_index(
    source: &dyn ChunkSource,
) -> Result<(ChunkTable, Vec<ChunkSurface>), ChunkError> {
    let len = source.len();
    if len < HEAD_LEN + TAIL_LEN {
        return Err(ChunkError::Format("file too short".to_owned()));
    }
    let window = TAIL_WINDOW.min(len);
    let window_start = len - window;
    let tail_bytes = source.read_at(window_start, window as usize)?;
    // 文件头：小文件已在尾部窗口里，否则单独一次 16 B 读；防止文件被截断后又被别的内容覆盖开头。
    let head = if window_start == 0 {
        ChunkBytes::Owned(tail_bytes[..HEAD_LEN as usize].to_vec())
    } else {
        source.read_at(0, HEAD_LEN as usize)?
    };
    if &head[..8] != HEAD_MAGIC || head[8..12] != FORMAT_VERSION.to_le_bytes() {
        return Err(ChunkError::Format(
            "head magic or version mismatch".to_owned(),
        ));
    }
    let tail = &tail_bytes[tail_bytes.len() - TAIL_LEN as usize..];
    let mut reader = Reader::new(tail);
    let table_offset = reader.u64()?;
    let table_len = reader.u64()?;
    let table_raw_len = reader.u32()? as usize;
    let version = reader.u32()?;
    if reader.bytes(8)? != TAIL_MAGIC {
        return Err(ChunkError::Format("tail magic mismatch".to_owned()));
    }
    if version != FORMAT_VERSION {
        return Err(ChunkError::Format(format!("unsupported version {version}")));
    }
    if table_offset < HEAD_LEN
        || table_offset
            .checked_add(table_len)
            .is_none_or(|end| end != len - TAIL_LEN)
    {
        return Err(ChunkError::Format("table range invalid".to_owned()));
    }
    let compressed = if table_offset >= window_start {
        let start = (table_offset - window_start) as usize;
        ChunkBytes::Owned(tail_bytes[start..start + table_len as usize].to_vec())
    } else {
        source.read_at(table_offset, table_len as usize)?
    };
    let table = decode_table(&decompress(&compressed, table_raw_len)?)?;
    if table
        .surface_offset
        .checked_add(table.surface_len)
        .is_none_or(|end| end > table_offset)
    {
        return Err(ChunkError::Format(
            "surface section out of range".to_owned(),
        ));
    }
    for chunk in &table.chunks {
        let (start, end) = chunk.cells_range();
        if start < HEAD_LEN || end > table.surface_offset {
            return Err(ChunkError::Format("cells range out of section".to_owned()));
        }
        for (block, field) in chunk.field_blocks.iter().zip(&table.fields) {
            let tuples = match field.association {
                crate::FieldAssociation::Node => u64::from(chunk.node_count),
                crate::FieldAssociation::Cell => u64::from(chunk.cell_count),
            };
            let (values_start, values_end) = block.values_range();
            let (boundary_start, boundary_end) = block.boundary_range();
            if values_start < HEAD_LEN
                || boundary_start < HEAD_LEN
                || values_end > table.surface_offset
                || boundary_end > table.surface_offset
                || u64::from(block.values_raw_len) != tuples * u64::from(field.components) * 8
            {
                return Err(ChunkError::Format("field block invalid".to_owned()));
            }
        }
    }
    let section = source.read_at(table.surface_offset, table.surface_len as usize)?;
    let surfaces = table
        .chunks
        .iter()
        .map(|chunk| {
            let start = chunk.surface_offset as usize;
            decode_surface(&decompress(
                &section[start..start + chunk.surface_len as usize],
                chunk.surface_raw_len as usize,
            )?)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (chunk, surface) in table.chunks.iter().zip(&surfaces) {
        for (block, field) in chunk.field_blocks.iter().zip(&table.fields) {
            if u64::from(block.boundary_raw_len)
                != boundary_tuple_count(surface.boundary.len(), field.association) as u64
                    * u64::from(field.components)
                    * 8
            {
                return Err(ChunkError::Format(
                    "field boundary block invalid".to_owned(),
                ));
            }
        }
    }
    Ok((table, surfaces))
}
