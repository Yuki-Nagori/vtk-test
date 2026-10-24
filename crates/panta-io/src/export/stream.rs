//! 从 v3 资产按块流式导出（task 025）：显示 LOD 或体网格未驻留时仍能导出完整网格，
//! 内存只保留一个批次的块。
//!
//! 表面网格：STL 与内存路径逐字节一致；带索引的格式不做全局顶点去重（去重需要与规模成正比的内存），
//! 按“未焊接”写出，每面三个独立点，坐标原样。体网格 `.vtu` 与内存路径的 ASCII 输出逐字节一致。

use super::{
    ExportError, ExportFormat, base64, detect_export_format, facet_normal, write_atomically,
};
use panta_mesh::{Progress, SurfaceAssetChunks, Triangle, VolumeAssetChunks};
use std::io::{self, Write};
use std::path::Path;

/// 流式导出表面资产的全部三角面；格式由扩展名决定，原子替换目标，可取消。
pub fn export_surface_asset_with_progress(
    chunks: &mut SurfaceAssetChunks,
    path: &Path,
    progress: &Progress,
) -> Result<ExportFormat, ExportError> {
    let format = detect_export_format(path)?;
    let triangles = chunks.triangle_count();
    let points = triangles
        .checked_mul(3)
        .ok_or(ExportError::TooLarge("point count"))?;
    match format {
        ExportFormat::Stl if u32::try_from(triangles).is_err() => {
            return Err(ExportError::TooLarge("binary STL triangle count"));
        }
        ExportFormat::Ply | ExportFormat::VtkLegacy if i32::try_from(points).is_err() => {
            return Err(ExportError::TooLarge("int32 point index"));
        }
        ExportFormat::VtkLegacy if i32::try_from(triangles.saturating_mul(4)).is_err() => {
            return Err(ExportError::TooLarge("legacy VTK POLYGONS size"));
        }
        _ => {}
    }
    let estimate = match format {
        ExportFormat::Stl => 84 + 50 * triangles,
        ExportFormat::Obj => points * 60 + triangles * 30,
        ExportFormat::Ply => points * 24 + triangles * 13,
        ExportFormat::VtkLegacy => points * 24 + triangles * 16,
        ExportFormat::Vtp => (points * 24 + triangles * 32) * 4 / 3,
        ExportFormat::Vtu => (points * 24 + triangles * 33) * 4 / 3,
    };
    write_atomically(path, progress, estimate, |writer| {
        write_surface_stream(chunks, format, triangles, writer)
    })?;
    Ok(format)
}

/// 逐批访问全部三角面；`write` 的首个错误中止遍历并返回，取消返回 `Cancelled`。
fn for_each_triangle(
    chunks: &mut SurfaceAssetChunks,
    mut write: impl FnMut(&Triangle) -> io::Result<()>,
) -> Result<(), ExportError> {
    let mut failure = None;
    let finished = chunks
        .visit_batches_until(None, |_, batch| {
            for triangle in batch.iter().flatten() {
                if let Err(error) = write(triangle) {
                    failure = Some(error);
                    return false;
                }
            }
            true
        })
        .map_err(ExportError::Source)?;
    if let Some(error) = failure {
        return Err(ExportError::Io(error));
    }
    if !finished {
        return Err(ExportError::Cancelled);
    }
    Ok(())
}

fn write_surface_stream(
    chunks: &mut SurfaceAssetChunks,
    format: ExportFormat,
    triangles: u64,
    writer: &mut impl Write,
) -> Result<(), ExportError> {
    let points = triangles * 3;
    match format {
        ExportFormat::Stl => {
            let mut header = [0_u8; 80];
            let label = b"panta binary STL export (mm)";
            header[..label.len()].copy_from_slice(label);
            writer.write_all(&header)?;
            writer.write_all(&(triangles as u32).to_le_bytes())?;
            for_each_triangle(chunks, |triangle| {
                for value in facet_normal(triangle) {
                    writer.write_all(&value.to_le_bytes())?;
                }
                for value in triangle.iter().flatten() {
                    #[expect(clippy::cast_possible_truncation, reason = "二进制 STL 只存 f32")]
                    let narrow = *value as f32;
                    writer.write_all(&narrow.to_le_bytes())?;
                }
                writer.write_all(&[0, 0])
            })
        }
        ExportFormat::Obj => {
            writeln!(writer, "# panta OBJ export (mm, unwelded)")?;
            for_each_triangle(chunks, |triangle| {
                for [x, y, z] in triangle {
                    writeln!(writer, "v {x:?} {y:?} {z:?}")?;
                }
                Ok(())
            })?;
            for cell in 0..triangles {
                let first = cell * 3 + 1;
                writeln!(writer, "f {} {} {}", first, first + 1, first + 2)?;
            }
            Ok(())
        }
        ExportFormat::Ply => {
            write!(
                writer,
                "ply\nformat binary_little_endian 1.0\ncomment panta export (mm, unwelded)\n\
                 element vertex {points}\nproperty double x\nproperty double y\nproperty double z\n\
                 element face {triangles}\nproperty list uchar int vertex_indices\nend_header\n"
            )?;
            for_each_triangle(chunks, |triangle| {
                for value in triangle.iter().flatten() {
                    writer.write_all(&value.to_le_bytes())?;
                }
                Ok(())
            })?;
            for cell in 0..triangles as u32 {
                writer.write_all(&[3])?;
                for corner in 0..3 {
                    writer.write_all(&(cell * 3 + corner).to_le_bytes())?;
                }
            }
            Ok(())
        }
        ExportFormat::VtkLegacy => {
            write!(
                writer,
                "# vtk DataFile Version 4.2\npanta export (mm, unwelded)\nBINARY\nDATASET POLYDATA\n\
                 POINTS {points} double\n"
            )?;
            for_each_triangle(chunks, |triangle| {
                for value in triangle.iter().flatten() {
                    writer.write_all(&value.to_be_bytes())?;
                }
                Ok(())
            })?;
            write!(writer, "\nPOLYGONS {triangles} {}\n", triangles * 4)?;
            for cell in 0..triangles as u32 {
                writer.write_all(&3_i32.to_be_bytes())?;
                for corner in 0..3 {
                    writer.write_all(&(cell * 3 + corner).to_be_bytes())?;
                }
            }
            writeln!(writer)?;
            Ok(())
        }
        ExportFormat::Vtp | ExportFormat::Vtu => {
            let unstructured = format == ExportFormat::Vtu;
            let (kind, cells, count_name) = if unstructured {
                ("UnstructuredGrid", "Cells", "NumberOfCells")
            } else {
                ("PolyData", "Polys", "NumberOfPolys")
            };
            write!(
                writer,
                "<?xml version=\"1.0\"?>\n<VTKFile type=\"{kind}\" version=\"1.0\" \
                 byte_order=\"LittleEndian\" header_type=\"UInt64\">\n<{kind}>\n\
                 <Piece NumberOfPoints=\"{points}\" {count_name}=\"{triangles}\">\n\
                 <Points>\n<DataArray type=\"Float64\" NumberOfComponents=\"3\" format=\"binary\">\n"
            )?;
            {
                let mut block = Base64Block::new(&mut *writer, points * 24)?;
                for_each_triangle(chunks, |triangle| {
                    for value in triangle.iter().flatten() {
                        block.push(&value.to_le_bytes())?;
                    }
                    Ok(())
                })?;
                block.finish()?;
            }
            write!(
                writer,
                "\n</DataArray>\n</Points>\n<{cells}>\n\
                 <DataArray type=\"Int64\" Name=\"connectivity\" format=\"binary\">\n"
            )?;
            let mut block = Base64Block::new(&mut *writer, points * 8)?;
            for index in 0..points {
                block.push(&(index as i64).to_le_bytes())?;
            }
            block.finish()?;
            write!(
                writer,
                "\n</DataArray>\n<DataArray type=\"Int64\" Name=\"offsets\" format=\"binary\">\n"
            )?;
            let mut block = Base64Block::new(&mut *writer, triangles * 8)?;
            for cell in 1..=triangles {
                block.push(&(cell * 3).to_le_bytes())?;
            }
            block.finish()?;
            writeln!(writer, "\n</DataArray>")?;
            if unstructured {
                writeln!(
                    writer,
                    "<DataArray type=\"UInt8\" Name=\"types\" format=\"binary\">"
                )?;
                let mut block = Base64Block::new(&mut *writer, triangles)?;
                for _ in 0..triangles {
                    block.push(&[5])?;
                }
                block.finish()?;
                writeln!(writer, "\n</DataArray>")?;
            }
            write!(writer, "</{cells}>\n</Piece>\n</{kind}>\n</VTKFile>\n")?;
            Ok(())
        }
    }
}

/// VTK 未压缩 inline binary 的流式编码：UInt64 长度头与数据作为同一 base64 流。
/// 缓冲按 3 字节倍数分段编码，拼接结果与整体编码一致。
struct Base64Block<W: Write> {
    writer: W,
    pending: Vec<u8>,
}

const BASE64_SEGMENT: usize = 3 * 256 * 1024;

impl<W: Write> Base64Block<W> {
    fn new(writer: W, byte_len: u64) -> io::Result<Self> {
        let mut block = Self {
            writer,
            pending: Vec::with_capacity(BASE64_SEGMENT + 64),
        };
        block.push(&byte_len.to_le_bytes())?;
        Ok(block)
    }

    fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() >= BASE64_SEGMENT {
            let rest = self.pending.split_off(BASE64_SEGMENT);
            self.writer.write_all(&base64(&self.pending))?;
            self.pending = rest;
            self.pending.reserve(BASE64_SEGMENT + 64);
        }
        Ok(())
    }

    fn finish(mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            self.writer.write_all(&base64(&self.pending))?;
        }
        Ok(())
    }
}

/// 流式导出体网格资产为 `.vtu`（ASCII 原生单元，与 [`super::write_tet_mesh_vtu`] 输出一致）。
/// 单元类型逐块收集为每单元 1 B，用于随后写 offsets / types，不驻留连接表。多面体（task 031）
/// 只写源节点，另记每个多面体 8 B（唯一节点数、面流长度），并再扫描一遍单元写 `faces`。
pub fn export_volume_asset_vtu(
    chunks: &mut VolumeAssetChunks,
    path: &Path,
    progress: &Progress,
) -> Result<(), ExportError> {
    let counts = chunks.counts();
    let source_nodes = counts.source_nodes();
    let estimate = counts.nodes as u64 * 60 + counts.cells as u64 * 60 + 512;
    write_atomically(path, progress, estimate, |writer| {
        writeln!(writer, "<?xml version=\"1.0\"?>")?;
        writeln!(
            writer,
            "<VTKFile type=\"UnstructuredGrid\" version=\"1.0\" byte_order=\"LittleEndian\">"
        )?;
        writeln!(writer, "<UnstructuredGrid>")?;
        writeln!(
            writer,
            "<Piece NumberOfPoints=\"{}\" NumberOfCells=\"{}\">",
            source_nodes, counts.cells
        )?;
        writeln!(
            writer,
            "<Points><DataArray type=\"Float64\" NumberOfComponents=\"3\" format=\"ascii\">"
        )?;
        let mut failure = None;
        let mut written = 0_usize;
        let finished = chunks
            .visit_nodes(None, |batch| {
                // 辅助中心节点位于末尾，不导出。
                for [x, y, z] in batch.iter().flatten().take(source_nodes - written) {
                    written += 1;
                    if let Err(error) = writeln!(writer, "{x:?} {y:?} {z:?}") {
                        failure = Some(error);
                        return false;
                    }
                }
                true
            })
            .map_err(ExportError::Source)?;
        if let Some(error) = failure.take() {
            return Err(ExportError::Io(error));
        }
        if !finished {
            return Err(ExportError::Cancelled);
        }
        writeln!(writer, "</DataArray></Points>")?;
        writeln!(
            writer,
            "<Cells><DataArray type=\"Int64\" Name=\"connectivity\" format=\"ascii\">"
        )?;
        let mut kinds: Vec<u8> = Vec::new();
        let mut polyhedra: Vec<(u32, u32)> = Vec::new();
        polyhedra.try_reserve_exact(counts.polyhedra).map_err(|_| {
            ExportError::Io(io::Error::other("polyhedron buffer allocation failed"))
        })?;
        kinds
            .try_reserve_exact(counts.cells)
            .map_err(|_| ExportError::Io(io::Error::other("cell type buffer allocation failed")))?;
        let finished = chunks
            .visit_cells(None, |batch| {
                for (kind, _, nodes) in batch.iter().flat_map(panta_mesh::CellBlock::iter) {
                    kinds.push(kind.vtk_code());
                    let written = if kind == panta_mesh::CellType::Polyhedron {
                        let unique = panta_mesh::cell_source_nodes(kind, nodes);
                        polyhedra.push((unique.len() as u32, nodes.len() as u32 - 1));
                        super::write_node_line(writer, &unique)
                    } else {
                        super::write_node_line(writer, nodes)
                    };
                    if let Err(error) = written {
                        failure = Some(error);
                        return false;
                    }
                }
                true
            })
            .map_err(ExportError::Source)?;
        if let Some(error) = failure {
            return Err(ExportError::Io(error));
        }
        if !finished {
            return Err(ExportError::Cancelled);
        }
        if kinds.len() != counts.cells || polyhedra.len() != counts.polyhedra {
            return Err(ExportError::Source(panta_mesh::MeshAssetError::InvalidMesh));
        }
        writeln!(
            writer,
            "</DataArray><DataArray type=\"Int64\" Name=\"offsets\" format=\"ascii\">"
        )?;
        let mut end = 0_u64;
        let mut poly = polyhedra.iter();
        for &code in &kinds {
            end += match panta_mesh::CellType::from_vtk(code) {
                Some(panta_mesh::CellType::Polyhedron) => {
                    poly.next().map_or(0, |&(unique, _)| u64::from(unique))
                }
                kind => kind
                    .and_then(panta_mesh::CellType::fixed_node_count)
                    .unwrap_or(0) as u64,
            };
            writeln!(writer, "{end}")?;
        }
        writeln!(
            writer,
            "</DataArray><DataArray type=\"UInt8\" Name=\"types\" format=\"ascii\">"
        )?;
        for code in &kinds {
            writeln!(writer, "{code}")?;
        }
        writeln!(writer, "</DataArray>")?;
        if !polyhedra.is_empty() {
            writeln!(
                writer,
                "<DataArray type=\"Int64\" Name=\"faces\" format=\"ascii\">"
            )?;
            let finished = chunks
                .visit_cells(None, |batch| {
                    for (kind, _, nodes) in batch.iter().flat_map(panta_mesh::CellBlock::iter) {
                        if kind != panta_mesh::CellType::Polyhedron {
                            continue;
                        }
                        if let Err(error) = super::write_node_line(writer, &nodes[1..]) {
                            failure = Some(error);
                            return false;
                        }
                    }
                    true
                })
                .map_err(ExportError::Source)?;
            if let Some(error) = failure {
                return Err(ExportError::Io(error));
            }
            if !finished {
                return Err(ExportError::Cancelled);
            }
            writeln!(
                writer,
                "</DataArray><DataArray type=\"Int64\" Name=\"faceoffsets\" format=\"ascii\">"
            )?;
            let mut end = 0_u64;
            let mut poly = polyhedra.iter();
            let polyhedron = panta_mesh::CellType::Polyhedron.vtk_code();
            for &code in &kinds {
                if code == polyhedron {
                    end += poly.next().map_or(0, |&(_, stream)| u64::from(stream));
                    writeln!(writer, "{end}")?;
                } else {
                    writeln!(writer, "-1")?;
                }
            }
            writeln!(writer, "</DataArray>")?;
        }
        writeln!(writer, "</Cells>")?;
        writeln!(writer, "</Piece></UnstructuredGrid></VTKFile>")?;
        Ok(())
    })
}

/// 测试探针：比较分段编码与整体编码。
#[cfg(test)]
pub(crate) struct Base64Probe;

#[cfg(test)]
impl Base64Probe {
    pub(crate) fn streamed(data: &[u8]) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut block = Base64Block::new(&mut out, data.len() as u64)?;
        for piece in data.chunks(1000) {
            block.push(piece)?;
        }
        block.finish()?;
        Ok(out)
    }

    pub(crate) fn whole(data: &[u8]) -> Vec<u8> {
        let mut bytes = (data.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(data);
        base64(&bytes)
    }
}
