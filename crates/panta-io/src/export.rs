//! 表面网格导出：与导入格式对称的写出端（STL / VTU / VTP / 旧版 VTK / OBJ / PLY）。
//!
//! 坐标按 `SurfaceMesh` 原样写出（工程资产为毫米），不做单位换算。带索引的格式
//! 按位相同的顶点去重（`-0.0` 视同 `0.0`），不做容差焊接；STL 二进制只能存 f32，
//! 会损失精度。写出先进临时文件，成功后原子替换目标，失败不留下半成品。

use panta_mesh::{PROGRESS_STRIDE, Progress, SurfaceMesh};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// 二进制 STL（f32 坐标）。
    Stl,
    /// VTK XML UnstructuredGrid，三角形单元（类型 5）。
    Vtu,
    /// VTK XML PolyData。
    Vtp,
    /// 旧版 `.vtk` 4.2 BINARY POLYDATA。
    VtkLegacy,
    Obj,
    /// binary_little_endian PLY。
    Ply,
}

impl ExportFormat {
    pub fn id(self) -> &'static str {
        match self {
            Self::Stl => "stl",
            Self::Vtu => "vtu",
            Self::Vtp => "vtp",
            Self::VtkLegacy => "vtk",
            Self::Obj => "obj",
            Self::Ply => "ply",
        }
    }
}

#[derive(Debug)]
pub enum ExportError {
    /// 扩展名不是可导出的网格格式。
    UnsupportedFormat(String),
    /// 网格超出目标格式的索引或计数上限。
    TooLarge(&'static str),
    /// 宿主经 [`Progress::cancel`] 取消；临时文件已删除，目标未被改动。
    Cancelled,
    Io(io::Error),
    /// 流式导出时源资产读取或校验失败（task 025）。
    Source(panta_mesh::MeshAssetError),
}

impl Display for ExportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFormat(path) => write!(formatter, "unsupported export format: {path}"),
            Self::TooLarge(limit) => write!(formatter, "mesh exceeds format limit: {limit}"),
            Self::Cancelled => write!(formatter, "export cancelled"),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Source(error) => write!(formatter, "source asset: {error}"),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<io::Error> for ExportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// 由扩展名决定导出格式（与导入一致，不做内容猜测）。
pub fn detect_export_format(path: &Path) -> Result<ExportFormat, ExportError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match extension.to_ascii_lowercase().as_str() {
        "stl" => Ok(ExportFormat::Stl),
        "vtu" => Ok(ExportFormat::Vtu),
        "vtp" => Ok(ExportFormat::Vtp),
        "vtk" => Ok(ExportFormat::VtkLegacy),
        "obj" => Ok(ExportFormat::Obj),
        "ply" => Ok(ExportFormat::Ply),
        _ => Err(ExportError::UnsupportedFormat(path.display().to_string())),
    }
}

/// 写出到 `path`：格式由扩展名决定；先写同目录临时文件再 rename。
pub fn export_surface_mesh(mesh: &SurfaceMesh, path: &Path) -> Result<ExportFormat, ExportError> {
    export_surface_mesh_with_progress(mesh, path, &Progress::new())
}

/// 同 [`export_surface_mesh`]，并报告进度：带索引格式的顶点去重占 0–40%，
/// 写出按估算字节数占 40–99%，原子替换后为 100%；写出过程中可协作取消。
pub fn export_surface_mesh_with_progress(
    mesh: &SurfaceMesh,
    path: &Path,
    progress: &Progress,
) -> Result<ExportFormat, ExportError> {
    let format = detect_export_format(path)?;
    let temporary = temporary_path(path);
    let result = (|| {
        let file = File::create(&temporary)?;
        let estimate = estimated_bytes(mesh, format);
        let mut writer = ProgressWriter {
            inner: BufWriter::with_capacity(1 << 20, file),
            progress,
            written: 0,
            estimate,
            start: 0.4,
        };
        write_with_progress(mesh, format, &mut writer, progress)?;
        if progress.is_cancelled() {
            return Err(ExportError::Cancelled);
        }
        let file = writer
            .inner
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        progress.set_fraction(1.0);
        Ok(format)
    })();
    if result.is_err() {
        let _ignored = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(format!(".{}.partial", std::process::id()));
    path.with_file_name(name)
}

/// 按格式写到任意 writer；供测试与后续流式导出复用。
pub fn write_surface_mesh(
    mesh: &SurfaceMesh,
    format: ExportFormat,
    writer: &mut impl Write,
) -> Result<(), ExportError> {
    write_with_progress(mesh, format, writer, &Progress::new())
}

fn write_with_progress(
    mesh: &SurfaceMesh,
    format: ExportFormat,
    writer: &mut impl Write,
    progress: &Progress,
) -> Result<(), ExportError> {
    let indexed = || IndexedMesh::new(mesh, progress);
    match format {
        ExportFormat::Stl => write_stl(mesh, writer),
        ExportFormat::Obj => write_obj(&indexed()?, writer),
        ExportFormat::Ply => write_ply(&indexed()?, writer),
        ExportFormat::VtkLegacy => write_vtk_legacy(&indexed()?, writer),
        ExportFormat::Vtp => write_vtk_xml(&indexed()?, false, writer),
        ExportFormat::Vtu => write_vtk_xml(&indexed()?, true, writer),
    }
    .map_err(|error| match error {
        ExportError::Io(io) if io.kind() == io::ErrorKind::Interrupted => ExportError::Cancelled,
        other => other,
    })
}

/// 写出字节数的粗略估算，仅用于进度比例；假设焊接后顶点约为三角形数一半。
fn estimated_bytes(mesh: &SurfaceMesh, format: ExportFormat) -> u64 {
    let triangles = mesh.triangles.len() as u64;
    let points = triangles / 2 + 3;
    match format {
        ExportFormat::Stl => 84 + 50 * triangles,
        ExportFormat::Obj => points * 60 + triangles * 24,
        ExportFormat::Ply | ExportFormat::VtkLegacy => points * 24 + triangles * 13,
        ExportFormat::Vtp => (points * 24 + triangles * 32) * 4 / 3,
        ExportFormat::Vtu => (points * 24 + triangles * 33) * 4 / 3,
    }
    .max(1)
}

/// 统计写出字节并映射到 `start`–99% 进度；取消时以 Interrupted 中止写出。
pub(crate) struct ProgressWriter<'a, W: Write> {
    inner: W,
    progress: &'a Progress,
    written: u64,
    estimate: u64,
    start: f64,
}

impl<W: Write> Write for ProgressWriter<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let before = self.written >> 20;
        let count = self.inner.write(buffer)?;
        self.written += count as u64;
        if self.written >> 20 != before {
            if self.progress.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "export cancelled",
                ));
            }
            self.progress.report(
                self.start,
                0.99,
                self.written.min(self.estimate),
                self.estimate,
            );
        }
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

struct IndexedMesh {
    points: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

impl IndexedMesh {
    fn new(mesh: &SurfaceMesh, progress: &Progress) -> Result<Self, ExportError> {
        let mut lookup: HashMap<[u64; 3], u32> = HashMap::with_capacity(mesh.triangles.len() / 2);
        let mut points = Vec::with_capacity(mesh.triangles.len() / 2);
        let mut triangles = Vec::with_capacity(mesh.triangles.len());
        let total = mesh.triangles.len() as u64;
        for (position, triangle) in mesh.triangles.iter().enumerate() {
            if position.is_multiple_of(PROGRESS_STRIDE) {
                if progress.is_cancelled() {
                    return Err(ExportError::Cancelled);
                }
                progress.report(0.0, 0.4, position as u64, total);
            }
            let mut indices = [0_u32; 3];
            for (slot, point) in indices.iter_mut().zip(triangle) {
                let key = point.map(|value| (value + 0.0).to_bits());
                *slot = match lookup.get(&key) {
                    Some(index) => *index,
                    None => {
                        // 旧版 VTK / PLY 用 int32 索引；统一以 i32 上限约束。
                        let index = u32::try_from(points.len())
                            .ok()
                            .filter(|index| i32::try_from(*index).is_ok())
                            .ok_or(ExportError::TooLarge("int32 point index"))?;
                        lookup.insert(key, index);
                        points.push(*point);
                        index
                    }
                };
            }
            triangles.push(indices);
        }
        Ok(Self { points, triangles })
    }
}

fn write_stl(mesh: &SurfaceMesh, writer: &mut impl Write) -> Result<(), ExportError> {
    let count = u32::try_from(mesh.triangles.len())
        .map_err(|_| ExportError::TooLarge("binary STL triangle count"))?;
    let mut header = [0_u8; 80];
    let label = b"panta binary STL export (mm)";
    header[..label.len()].copy_from_slice(label);
    writer.write_all(&header)?;
    writer.write_all(&count.to_le_bytes())?;
    for triangle in &mesh.triangles {
        let normal = facet_normal(triangle);
        for value in normal {
            writer.write_all(&value.to_le_bytes())?;
        }
        for point in triangle {
            for value in point {
                #[expect(clippy::cast_possible_truncation, reason = "二进制 STL 只存 f32")]
                let narrow = *value as f32;
                writer.write_all(&narrow.to_le_bytes())?;
            }
        }
        writer.write_all(&[0, 0])?;
    }
    Ok(())
}

pub(crate) fn facet_normal(triangle: &[[f64; 3]; 3]) -> [f32; 3] {
    let [a, b, c] = triangle;
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if !length.is_normal() {
        return [0.0; 3];
    }
    #[expect(clippy::cast_possible_truncation, reason = "二进制 STL 法向为 f32")]
    n.map(|value| (value / length) as f32)
}

fn write_obj(mesh: &IndexedMesh, writer: &mut impl Write) -> Result<(), ExportError> {
    writeln!(writer, "# panta OBJ export (mm)")?;
    for [x, y, z] in &mesh.points {
        writeln!(writer, "v {x:?} {y:?} {z:?}")?;
    }
    for [a, b, c] in &mesh.triangles {
        writeln!(
            writer,
            "f {} {} {}",
            u64::from(*a) + 1,
            u64::from(*b) + 1,
            u64::from(*c) + 1
        )?;
    }
    Ok(())
}

fn write_ply(mesh: &IndexedMesh, writer: &mut impl Write) -> Result<(), ExportError> {
    write!(
        writer,
        "ply\nformat binary_little_endian 1.0\ncomment panta export (mm)\n\
         element vertex {}\nproperty double x\nproperty double y\nproperty double z\n\
         element face {}\nproperty list uchar int vertex_indices\nend_header\n",
        mesh.points.len(),
        mesh.triangles.len()
    )?;
    for point in &mesh.points {
        for value in point {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    for triangle in &mesh.triangles {
        writer.write_all(&[3])?;
        for index in triangle {
            writer.write_all(&index.to_le_bytes())?;
        }
    }
    Ok(())
}

fn write_vtk_legacy(mesh: &IndexedMesh, writer: &mut impl Write) -> Result<(), ExportError> {
    let size = mesh
        .triangles
        .len()
        .checked_mul(4)
        .filter(|size| i32::try_from(*size).is_ok())
        .ok_or(ExportError::TooLarge("legacy VTK POLYGONS size"))?;
    write!(
        writer,
        "# vtk DataFile Version 4.2\npanta export (mm)\nBINARY\nDATASET POLYDATA\nPOINTS {} double\n",
        mesh.points.len()
    )?;
    for point in &mesh.points {
        for value in point {
            writer.write_all(&value.to_be_bytes())?;
        }
    }
    write!(writer, "\nPOLYGONS {} {size}\n", mesh.triangles.len())?;
    for triangle in &mesh.triangles {
        writer.write_all(&3_i32.to_be_bytes())?;
        for index in triangle {
            writer.write_all(&index.to_be_bytes())?;
        }
    }
    writeln!(writer)?;
    Ok(())
}

/// inline binary（base64，UInt64 头，未压缩）；VTK reader 与本仓库解析器均支持。
fn write_vtk_xml(
    mesh: &IndexedMesh,
    unstructured: bool,
    writer: &mut impl Write,
) -> Result<(), ExportError> {
    let (kind, cells) = if unstructured {
        ("UnstructuredGrid", "Cells")
    } else {
        ("PolyData", "Polys")
    };
    let piece_counts = if unstructured {
        format!(
            "NumberOfPoints=\"{}\" NumberOfCells=\"{}\"",
            mesh.points.len(),
            mesh.triangles.len()
        )
    } else {
        format!(
            "NumberOfPoints=\"{}\" NumberOfPolys=\"{}\"",
            mesh.points.len(),
            mesh.triangles.len()
        )
    };
    write!(
        writer,
        "<?xml version=\"1.0\"?>\n<VTKFile type=\"{kind}\" version=\"1.0\" \
         byte_order=\"LittleEndian\" header_type=\"UInt64\">\n<{kind}>\n<Piece {piece_counts}>\n\
         <Points>\n<DataArray type=\"Float64\" NumberOfComponents=\"3\" format=\"binary\">\n"
    )?;
    write_base64_block(writer, mesh.points.len() * 24, |out| {
        for point in &mesh.points {
            for value in point {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
    })?;
    write!(
        writer,
        "\n</DataArray>\n</Points>\n<{cells}>\n\
         <DataArray type=\"Int64\" Name=\"connectivity\" format=\"binary\">\n"
    )?;
    write_base64_block(writer, mesh.triangles.len() * 24, |out| {
        for triangle in &mesh.triangles {
            for index in triangle {
                out.extend_from_slice(&i64::from(*index).to_le_bytes());
            }
        }
    })?;
    write!(
        writer,
        "\n</DataArray>\n<DataArray type=\"Int64\" Name=\"offsets\" format=\"binary\">\n"
    )?;
    write_base64_block(writer, mesh.triangles.len() * 8, |out| {
        for cell in 1..=mesh.triangles.len() as u64 {
            out.extend_from_slice(&(cell * 3).to_le_bytes());
        }
    })?;
    writeln!(writer, "\n</DataArray>")?;
    if unstructured {
        writeln!(
            writer,
            "<DataArray type=\"UInt8\" Name=\"types\" format=\"binary\">"
        )?;
        write_base64_block(writer, mesh.triangles.len(), |out| {
            out.resize(out.len() + mesh.triangles.len(), 5);
        })?;
        writeln!(writer, "\n</DataArray>")?;
    }
    write!(writer, "</{cells}>\n</Piece>\n</{kind}>\n</VTKFile>\n")?;
    Ok(())
}

/// VTK 未压缩 inline binary：UInt64 字节长度头与数据作为同一 base64 流编码。
fn write_base64_block(
    writer: &mut impl Write,
    byte_len: usize,
    fill: impl FnOnce(&mut Vec<u8>),
) -> Result<(), ExportError> {
    let mut data = Vec::with_capacity(byte_len + 8);
    data.extend_from_slice(&(byte_len as u64).to_le_bytes());
    fill(&mut data);
    debug_assert_eq!(data.len(), byte_len + 8);
    // 分段长度为 3 的倍数，逐段编码的拼接与整体编码一致。
    for chunk in data.chunks(3 * 1024 * 1024) {
        writer.write_all(&base64(chunk))?;
    }
    Ok(())
}

pub(crate) fn base64(bytes: &[u8]) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextets = [n >> 18, n >> 12, n >> 6, n].map(|value| TABLE[(value & 63) as usize]);
        out.extend_from_slice(&sextets[..chunk.len() + 1]);
        out.resize(out.len() + 3 - chunk.len(), b'=');
    }
    out
}

#[cfg(test)]
mod tests;

mod stream;
#[cfg(test)]
pub(crate) use stream::Base64Probe;
pub use stream::{export_surface_asset_with_progress, export_volume_asset_vtu};

/// 体网格导出 VTU（ASCII，原始节点与线性四面体单元，坐标为毫米），原子替换目标（task 021）。
/// 按估算字节报告 0–99% 进度，原子替换后到 100%。
/// 取消时删除临时文件并返回 [`ExportError::Cancelled`]，目标文件不变。
pub fn export_tet_mesh_vtu(
    mesh: &panta_mesh::TetMesh,
    path: &Path,
    progress: &Progress,
) -> Result<(), ExportError> {
    // ASCII：每节点约 3×20 字节，每单元连接/偏移/类型约 40 字节。
    let estimate = mesh.nodes.len() as u64 * 60 + mesh.tets.len() as u64 * 40 + 512;
    write_atomically(path, progress, estimate, |writer| {
        write_tet_mesh_vtu(mesh, writer)
    })
}

/// 先写同目录 `.partial` 临时文件再原子替换目标；按写出字节相对 `estimate` 报告 0–99% 进度，
/// 取消（写出中断）映射为 [`ExportError::Cancelled`]，失败时删除临时文件。
pub(crate) fn write_atomically(
    path: &Path,
    progress: &Progress,
    estimate: u64,
    write: impl FnOnce(&mut ProgressWriter<'_, BufWriter<File>>) -> Result<(), ExportError>,
) -> Result<(), ExportError> {
    let temporary = temporary_path(path);
    let result = (|| {
        let mut writer = ProgressWriter {
            inner: BufWriter::with_capacity(1 << 20, File::create(&temporary)?),
            progress,
            written: 0,
            estimate: estimate.max(1),
            start: 0.0,
        };
        write(&mut writer).map_err(|error| match error {
            ExportError::Io(io) if io.kind() == io::ErrorKind::Interrupted => {
                ExportError::Cancelled
            }
            other => other,
        })?;
        if progress.is_cancelled() {
            return Err(ExportError::Cancelled);
        }
        let file = writer
            .inner
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        progress.set_fraction(1.0);
        Ok(())
    })();
    if result.is_err() {
        let _ignored = fs::remove_file(&temporary);
    }
    result
}

pub fn write_tet_mesh_vtu(
    mesh: &panta_mesh::TetMesh,
    writer: &mut impl Write,
) -> Result<(), ExportError> {
    writeln!(writer, "<?xml version=\"1.0\"?>")?;
    writeln!(
        writer,
        "<VTKFile type=\"UnstructuredGrid\" version=\"1.0\" byte_order=\"LittleEndian\">"
    )?;
    writeln!(writer, "<UnstructuredGrid>")?;
    writeln!(
        writer,
        "<Piece NumberOfPoints=\"{}\" NumberOfCells=\"{}\">",
        mesh.nodes.len(),
        mesh.tets.len()
    )?;
    writeln!(
        writer,
        "<Points><DataArray type=\"Float64\" NumberOfComponents=\"3\" format=\"ascii\">"
    )?;
    for [x, y, z] in &mesh.nodes {
        writeln!(writer, "{x:?} {y:?} {z:?}")?;
    }
    writeln!(writer, "</DataArray></Points>")?;
    writeln!(
        writer,
        "<Cells><DataArray type=\"Int64\" Name=\"connectivity\" format=\"ascii\">"
    )?;
    for tet in &mesh.tets {
        let [a, b, c, d] = tet.nodes;
        writeln!(writer, "{a} {b} {c} {d}")?;
    }
    writeln!(
        writer,
        "</DataArray><DataArray type=\"Int64\" Name=\"offsets\" format=\"ascii\">"
    )?;
    for index in 1..=mesh.tets.len() {
        writeln!(writer, "{}", index * 4)?;
    }
    writeln!(
        writer,
        "</DataArray><DataArray type=\"UInt8\" Name=\"types\" format=\"ascii\">"
    )?;
    for _ in &mesh.tets {
        writeln!(writer, "10")?;
    }
    writeln!(writer, "</DataArray></Cells>")?;
    writeln!(writer, "</Piece></UnstructuredGrid></VTKFile>")?;
    Ok(())
}
