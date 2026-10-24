//! 结果场 sidecar（task 029）：节点 / 单元上的标量与向量场，与网格共用 v3 分块容器。
//!
//! 结果场单独成文件、不写进网格资产：网格资产和摘要保持不变，同一网格以后可以挂多个
//! 时间步的结果文件。绑定关系由表头中的网格资产摘要、节点数与单元数保证，读取端核对。
//!
//! 文件布局沿用容器头部与目录（类型字节为 `RESULT_KIND`），流依次为：
//! - `ResultTable`：字节流，小端，`u32 表版本`、`[u8; 32] 网格摘要`、`u64 节点数`、
//!   `u64 单元数`、`u32 字段数`，随后每个字段为 `u16 名称长度 + 名称`、`u8 关联`、
//!   `u8 分量数`、`u16 单位长度 + 单位`、`u64 缺失数`、`u8 有无范围`、`f64 最小`、`f64 最大`；
//! - `ResultValues`：f64 数值，按字段顺序排列，每个字段的块单独切分、不跨字段，
//!   以便按需只读取一个字段。

use super::stream::{open_layout, visit_chunks};
use super::{
    ChunkEntry, Layout, MeshAssetError, RESULT_KIND, Stream, assemble, chunk_jobs, encode_chunks,
};
use std::fs::File;
use std::ops::Range;
use std::path::Path;

/// 表头版本；新增字段属性时递增，读取端拒绝未知版本。
/// 结果场表版本，同时写入工程清单 `results.format_version`。
pub const RESULT_ASSET_FORMAT_VERSION: u32 = 1;
const RESULT_TABLE_VERSION: u32 = RESULT_ASSET_FORMAT_VERSION;
const MAX_FIELDS: usize = 4096;
/// 允许 1–9 分量（标量、向量、张量）；首期导入只产生 1 与 3。
const MAX_COMPONENTS: u8 = 9;
const MAX_NAME_BYTES: usize = 255;
const MAX_UNITS_BYTES: usize = 64;

/// 场值所在位置：节点（插值）或单元（分片常数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldAssociation {
    Node,
    Cell,
}

impl FieldAssociation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Cell => "cell",
        }
    }

    fn byte(self) -> u8 {
        match self {
            Self::Node => 0,
            Self::Cell => 1,
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Node),
            1 => Some(Self::Cell),
            _ => None,
        }
    }
}

/// 一个字段的元数据与统计；只读表头即可得到，不需要读取数值。
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSummary {
    pub name: String,
    pub association: FieldAssociation,
    pub components: u8,
    /// 来源声明的单位；空串表示来源未声明（VTK 格式本身不带单位）。
    pub units: String,
    /// 含任一非有限分量（NaN / ±inf）的元组数；缺失值不按 0 参与统计。
    pub missing: u64,
    /// 有限元组的显示标量范围：标量取值本身，多分量取模；全部缺失时为 `None`。
    pub range: Option<[f64; 2]>,
}

/// 一个字段的完整数值：`values.len() == 元组数 × components`，按元组交错存放。
#[derive(Debug, Clone, PartialEq)]
pub struct ResultField {
    pub name: String,
    pub association: FieldAssociation,
    pub components: u8,
    pub units: String,
    pub values: Vec<f64>,
}

impl ResultField {
    /// 按数值重新计算统计；非有限值原样保留在 `values` 中，只计入缺失数。
    pub fn summary(&self) -> FieldSummary {
        let components = usize::from(self.components.max(1));
        let mut missing = 0_u64;
        let mut range: Option<[f64; 2]> = None;
        for tuple in self.values.chunks(components) {
            if tuple.iter().any(|value| !value.is_finite()) {
                missing += 1;
                continue;
            }
            let scalar = if components == 1 {
                tuple[0]
            } else {
                tuple.iter().map(|value| value * value).sum::<f64>().sqrt()
            };
            if !scalar.is_finite() {
                missing += 1;
                continue;
            }
            range = Some(match range {
                Some([low, high]) => [low.min(scalar), high.max(scalar)],
                None => [scalar, scalar],
            });
        }
        FieldSummary {
            name: self.name.clone(),
            association: self.association,
            components: self.components,
            units: self.units.clone(),
            missing,
            range,
        }
    }
}

/// 绑定到一份网格资产的一组结果场。
#[derive(Debug, Clone, PartialEq)]
pub struct ResultSet {
    /// 绑定网格资产的摘要（`sha256:` + 64 位十六进制）。
    pub mesh_digest: String,
    pub node_count: u64,
    pub cell_count: u64,
    pub fields: Vec<ResultField>,
}

impl ResultSet {
    fn tuples(&self, association: FieldAssociation) -> u64 {
        match association {
            FieldAssociation::Node => self.node_count,
            FieldAssociation::Cell => self.cell_count,
        }
    }
}

/// 编码结果场 sidecar，返回完整文件字节与文件摘要。
/// 字段为空、名称重复或非法、分量数越界、数值个数与网格计数不符时返回 `InvalidMesh`。
pub fn encode_result_asset(set: &ResultSet) -> Result<(Vec<u8>, String), MeshAssetError> {
    let mesh_digest = parse_digest(&set.mesh_digest).ok_or(MeshAssetError::InvalidMesh)?;
    if set.node_count == 0
        || set.cell_count == 0
        || set.fields.is_empty()
        || set.fields.len() > MAX_FIELDS
    {
        return Err(MeshAssetError::InvalidMesh);
    }
    let mut seen = std::collections::HashSet::new();
    let mut summaries = Vec::with_capacity(set.fields.len());
    for field in &set.fields {
        let expected = set
            .tuples(field.association)
            .checked_mul(u64::from(field.components))
            .ok_or(MeshAssetError::SizeOverflow)?;
        if !valid_field_header(&field.name, field.components, &field.units)
            || field.values.len() as u64 != expected
            || !seen.insert((field.name.as_str(), field.association))
        {
            return Err(MeshAssetError::InvalidMesh);
        }
        summaries.push(field.summary());
    }
    let table = encode_table(&mesh_digest, set.node_count, set.cell_count, &summaries);
    let mut jobs = chunk_jobs(Stream::ResultTable, table.len());
    // 各字段在一条拼接下标空间中占据连续区间；分别切块，保证块不跨字段。
    let mut starts = Vec::with_capacity(set.fields.len());
    let mut start = 0_usize;
    for field in &set.fields {
        starts.push(start);
        jobs.extend(
            chunk_jobs(Stream::ResultValues, field.values.len())
                .into_iter()
                .map(|(stream, range)| (stream, range.start + start..range.end + start)),
        );
        start = start
            .checked_add(field.values.len())
            .ok_or(MeshAssetError::SizeOverflow)?;
    }
    let chunks = encode_chunks(&jobs, |(stream, range), raw| match stream {
        Stream::ResultTable => raw.extend_from_slice(&table[range]),
        Stream::ResultValues => {
            let index = starts.partition_point(|&start| start <= range.start) - 1;
            let local = range.start - starts[index]..range.end - starts[index];
            for value in &set.fields[index].values[local] {
                raw.extend_from_slice(&value.to_le_bytes());
            }
        }
        _ => {}
    })?;
    assemble(RESULT_KIND, chunks)
}

fn valid_field_header(name: &str, components: u8, units: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && !name.chars().any(char::is_control)
        && units.len() <= MAX_UNITS_BYTES
        && !units.chars().any(char::is_control)
        && (1..=MAX_COMPONENTS).contains(&components)
}

fn parse_digest(digest: &str) -> Option<[u8; 32]> {
    let hex = digest.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(bytes)
}

fn format_digest(bytes: &[u8; 32]) -> String {
    let mut text = String::with_capacity(71);
    text.push_str("sha256:");
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn encode_table(
    mesh_digest: &[u8; 32],
    node_count: u64,
    cell_count: u64,
    summaries: &[FieldSummary],
) -> Vec<u8> {
    let mut table = Vec::new();
    table.extend_from_slice(&RESULT_TABLE_VERSION.to_le_bytes());
    table.extend_from_slice(mesh_digest);
    table.extend_from_slice(&node_count.to_le_bytes());
    table.extend_from_slice(&cell_count.to_le_bytes());
    table.extend_from_slice(&(summaries.len() as u32).to_le_bytes());
    for summary in summaries {
        // 名称与单位长度已由 `valid_field_header` 限定在 u16 内。
        table.extend_from_slice(&(summary.name.len() as u16).to_le_bytes());
        table.extend_from_slice(summary.name.as_bytes());
        table.push(summary.association.byte());
        table.push(summary.components);
        table.extend_from_slice(&(summary.units.len() as u16).to_le_bytes());
        table.extend_from_slice(summary.units.as_bytes());
        table.extend_from_slice(&summary.missing.to_le_bytes());
        let (flag, [low, high]) = match summary.range {
            Some(range) => (1, range),
            None => (0, [0.0, 0.0]),
        };
        table.push(flag);
        table.extend_from_slice(&low.to_le_bytes());
        table.extend_from_slice(&high.to_le_bytes());
    }
    table
}

struct TableCursor<'a> {
    bytes: &'a [u8],
}

impl<'a> TableCursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], MeshAssetError> {
        if self.bytes.len() < len {
            return Err(MeshAssetError::InvalidLength);
        }
        let (head, tail) = self.bytes.split_at(len);
        self.bytes = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], MeshAssetError> {
        let mut value = [0; N];
        value.copy_from_slice(self.take(N)?);
        Ok(value)
    }

    fn text(&mut self) -> Result<String, MeshAssetError> {
        let len = usize::from(u16::from_le_bytes(self.array()?));
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| MeshAssetError::InvalidMesh)
    }
}

struct Table {
    mesh_digest: String,
    node_count: u64,
    cell_count: u64,
    summaries: Vec<FieldSummary>,
}

fn decode_table(bytes: &[u8]) -> Result<Table, MeshAssetError> {
    let mut cursor = TableCursor { bytes };
    if u32::from_le_bytes(cursor.array()?) != RESULT_TABLE_VERSION {
        return Err(MeshAssetError::UnsupportedLayout);
    }
    let mesh_digest = format_digest(&cursor.array()?);
    let node_count = u64::from_le_bytes(cursor.array()?);
    let cell_count = u64::from_le_bytes(cursor.array()?);
    let field_count = u32::from_le_bytes(cursor.array()?) as usize;
    if node_count == 0 || cell_count == 0 || field_count == 0 || field_count > MAX_FIELDS {
        return Err(MeshAssetError::InvalidMesh);
    }
    let mut summaries = Vec::with_capacity(field_count);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..field_count {
        let name = cursor.text()?;
        let association = FieldAssociation::from_byte(cursor.array::<1>()?[0])
            .ok_or(MeshAssetError::InvalidMesh)?;
        let components = cursor.array::<1>()?[0];
        let units = cursor.text()?;
        let missing = u64::from_le_bytes(cursor.array()?);
        let flag = cursor.array::<1>()?[0];
        let low = f64::from_le_bytes(cursor.array()?);
        let high = f64::from_le_bytes(cursor.array()?);
        let range = match flag {
            0 => None,
            1 if low.is_finite() && high.is_finite() && low <= high => Some([low, high]),
            _ => return Err(MeshAssetError::InvalidMesh),
        };
        if !valid_field_header(&name, components, &units)
            || !seen.insert((name.clone(), association))
        {
            return Err(MeshAssetError::InvalidMesh);
        }
        summaries.push(FieldSummary {
            name,
            association,
            components,
            units,
            missing,
            range,
        });
    }
    if !cursor.bytes.is_empty() {
        return Err(MeshAssetError::InvalidLength);
    }
    Ok(Table {
        mesh_digest,
        node_count,
        cell_count,
        summaries,
    })
}

/// 已打开的结果场 sidecar：头部、目录与字段表已校验，数值按字段按需读取。
pub struct ResultAssetReader {
    file: File,
    layout: Layout,
    table: Table,
    /// 每个字段在目录中的数值块。
    field_chunks: Vec<Vec<ChunkEntry>>,
}

impl ResultAssetReader {
    /// 读取头部、目录与字段表，并核对每个字段的数值块恰好覆盖 `元组数 × 分量数`。
    pub fn open(path: &Path) -> Result<Self, MeshAssetError> {
        let (mut file, layout) = open_layout(path, RESULT_KIND)?;
        let table_chunks: Vec<ChunkEntry> = layout
            .chunks
            .iter()
            .filter(|chunk| chunk.stream == Stream::ResultTable)
            .cloned()
            .collect();
        if table_chunks.is_empty() {
            return Err(MeshAssetError::UnsupportedLayout);
        }
        let mut table_bytes = Vec::new();
        visit_chunks(
            &mut file,
            &table_chunks,
            None,
            |_, raw| Ok(raw.to_vec()),
            |decoded| {
                decoded
                    .iter()
                    .for_each(|bytes| table_bytes.extend_from_slice(bytes));
                true
            },
        )?;
        let table = decode_table(&table_bytes)?;
        let mut values = layout
            .chunks
            .iter()
            .filter(|chunk| chunk.stream == Stream::ResultValues)
            .cloned()
            .peekable();
        let mut field_chunks = Vec::with_capacity(table.summaries.len());
        for summary in &table.summaries {
            let tuples = match summary.association {
                FieldAssociation::Node => table.node_count,
                FieldAssociation::Cell => table.cell_count,
            };
            let expected = tuples
                .checked_mul(u64::from(summary.components))
                .ok_or(MeshAssetError::InvalidLength)?;
            let mut covered = 0_u64;
            let mut chunks = Vec::new();
            while covered < expected {
                let chunk = values.next().ok_or(MeshAssetError::InvalidLength)?;
                covered += chunk.record_count as u64;
                chunks.push(chunk);
            }
            if covered != expected {
                return Err(MeshAssetError::InvalidLength);
            }
            field_chunks.push(chunks);
        }
        if values.peek().is_some() {
            return Err(MeshAssetError::InvalidLength);
        }
        Ok(Self {
            file,
            layout,
            table,
            field_chunks,
        })
    }

    /// 结果文件自身的摘要（头部记录的目录摘要）。
    pub fn digest(&self) -> &str {
        &self.layout.digest
    }

    /// 绑定网格资产的摘要；调用方须与当前网格资产摘要比对后再使用。
    pub fn mesh_digest(&self) -> &str {
        &self.table.mesh_digest
    }

    pub fn node_count(&self) -> u64 {
        self.table.node_count
    }

    pub fn cell_count(&self) -> u64 {
        self.table.cell_count
    }

    pub fn summaries(&self) -> &[FieldSummary] {
        &self.table.summaries
    }

    /// 读取一个字段的全部数值，并核对重新计算的统计与表头一致。
    pub fn read_field(&mut self, index: usize) -> Result<ResultField, MeshAssetError> {
        let summary = self
            .table
            .summaries
            .get(index)
            .cloned()
            .ok_or(MeshAssetError::InvalidMesh)?;
        let chunks = &self.field_chunks[index];
        let total: usize = chunks.iter().map(|chunk| chunk.record_count).sum();
        let mut values = Vec::new();
        values
            .try_reserve_exact(total)
            .map_err(|_| MeshAssetError::AllocationFailed)?;
        visit_chunks(
            &mut self.file,
            chunks,
            None,
            |_, raw| {
                Ok(raw
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|bytes| f64::from_le_bytes(*bytes))
                    .collect())
            },
            |decoded| {
                decoded
                    .iter()
                    .for_each(|batch| values.extend_from_slice(batch));
                true
            },
        )?;
        let field = ResultField {
            name: summary.name.clone(),
            association: summary.association,
            components: summary.components,
            units: summary.units.clone(),
            values,
        };
        if !same_summary(&field.summary(), &summary) {
            return Err(MeshAssetError::ChecksumMismatch);
        }
        Ok(field)
    }

    /// 读取全部字段。
    pub fn read_all(mut self) -> Result<ResultSet, MeshAssetError> {
        let fields = (0..self.table.summaries.len())
            .map(|index| self.read_field(index))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ResultSet {
            mesh_digest: self.table.mesh_digest,
            node_count: self.table.node_count,
            cell_count: self.table.cell_count,
            fields,
        })
    }

    /// 每个字段数值块在文件中的字节区间，供基准统计读量。
    pub fn field_byte_range(&self, index: usize) -> Option<Range<u64>> {
        let chunks = self.field_chunks.get(index)?;
        let first = chunks.first()?;
        let last = chunks.last()?;
        Some(first.offset..last.offset + last.stored_len as u64)
    }
}

/// 范围逐位比较：同一数值序列重新计算应得到完全相同的结果。
fn same_summary(left: &FieldSummary, right: &FieldSummary) -> bool {
    let bits = |range: Option<[f64; 2]>| range.map(|range| range.map(f64::to_bits));
    left.missing == right.missing && bits(left.range) == bits(right.range)
}

#[cfg(test)]
mod tests;
