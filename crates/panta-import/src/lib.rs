//! 格式识别和来源快照；格式专属算法由对应领域后端执行。
//! STEP / IGES 的几何读取由后续 OCCT adapter 接入。

pub mod export;
pub mod formats;

use panta_mesh::{StlError, SurfaceMesh, SurfaceSummary, parse_stl, surface_mesh_expanded_bytes};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Stl,
    /// VTK XML UnstructuredGrid；体单元只保留外表面。
    Vtu,
    /// VTK XML PolyData。
    Vtp,
    /// 旧版 `.vtk`（POLYDATA / UNSTRUCTURED_GRID）。
    VtkLegacy,
    Obj,
    Ply,
    Step,
    Iges,
}

impl ImportFormat {
    /// 写入 ImportRecord.format 的稳定标识；同时是默认显示名要剥离的扩展名。
    pub fn id(self) -> &'static str {
        match self {
            Self::Stl => "stl",
            Self::Vtu => "vtu",
            Self::Vtp => "vtp",
            Self::VtkLegacy => "vtk",
            Self::Obj => "obj",
            Self::Ply => "ply",
            Self::Step => "step",
            Self::Iges => "iges",
        }
    }

    /// 由 Rust 表面网格解析器支持、可走网格导入会话的格式。
    pub fn is_surface_mesh(self) -> bool {
        !matches!(self, Self::Step | Self::Iges)
    }
}

#[derive(Debug)]
pub enum ImportError {
    Missing(String),
    UnsupportedFormat(String),
    Read(String),
    Parse(StlError),
    /// STL 以外网格格式的解析错误。
    Format(formats::FormatError),
    SourceChanged(String),
    UnsupportedMeshType(String),
    UnsupportedUnits(String),
    CoordinateOverflow,
}

impl Display for ImportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ImportError {}

pub const IMPORT_RECORD_VERSION: u32 = 4;
pub const STL_IMPORT_PARSER_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportRecord {
    pub record_version: u32,
    pub parser_version: u32,
    pub id: String,
    pub display_name: String,
    pub source_name: String,
    pub source_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
    pub source_size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_modified_unix_ms: Option<u64>,
    pub mesh_asset_id: String,
    pub format: String,
    pub mesh_type: String,
    pub units: String,
    pub show_import_log: bool,
    pub triangle_count: u64,
    pub dimensions: [f64; 3],
}

/// 返回与来源名称分离的默认资产名；只移除该记录所声明格式的扩展名。
pub fn default_display_name(source_name: &str, format: &str) -> String {
    let extension = format.strip_prefix('.').unwrap_or(format);
    source_name
        .rsplit_once('.')
        .filter(|(_, suffix)| suffix.eq_ignore_ascii_case(extension))
        .map_or_else(|| source_name.to_owned(), |(stem, _)| stem.to_owned())
}

#[derive(Debug, Clone, PartialEq)]
pub struct StlImportPreview {
    pub source_name: String,
    pub triangle_count: u64,
    pub dimensions: [f64; 3],
    /// 未压缩 Mesh 资产的估算字节数；不包含来源缓冲、显示 DTO 或 GPU 内存。
    pub expanded_mesh_bytes: u64,
    /// 仅提醒资源成本；不限制导入，也不依赖来源文件的压缩率或字节数。
    pub large_mesh_warning: bool,
}

impl StlImportPreview {
    fn from_summary(source_name: String, summary: SurfaceSummary) -> Self {
        const LARGE_MESH_WARNING_BYTES: u64 = 2 * 1024 * 1024 * 1024;
        let expanded_mesh_bytes = surface_mesh_expanded_bytes(summary.triangle_count);
        Self {
            source_name,
            triangle_count: summary.triangle_count,
            dimensions: summary.dimensions,
            expanded_mesh_bytes,
            large_mesh_warning: expanded_mesh_bytes > LARGE_MESH_WARNING_BYTES,
        }
    }
}

/// 可用网格类型的稳定 ID 和英文源文案；Qt 仅负责翻译和展示。
#[derive(Debug, Clone, Copy)]
pub struct MeshTypeDefinition {
    pub id: &'static str,
    pub source_text: &'static str,
}

pub const DEFAULT_MESH_TYPE: &str = "dual-domain";
pub const MESH_TYPES: &[MeshTypeDefinition] = &[
    MeshTypeDefinition {
        id: "midplane",
        source_text: "中面",
    },
    MeshTypeDefinition {
        id: "dual-domain",
        source_text: "双层面",
    },
    MeshTypeDefinition {
        id: "solid-3d",
        source_text: "实体 3D",
    },
];

#[derive(Debug, Clone, Copy)]
pub struct ImportOptions {
    mesh_type: &'static str,
    units: LengthUnit,
    show_import_log: bool,
}

#[derive(Debug, Clone, Copy)]
enum LengthUnit {
    Millimeters,
    Centimeters,
    Inches,
}

impl LengthUnit {
    fn parse(value: &str) -> Result<Self, ImportError> {
        match value {
            "millimeters" => Ok(Self::Millimeters),
            "centimeters" => Ok(Self::Centimeters),
            "inches" => Ok(Self::Inches),
            other => Err(ImportError::UnsupportedUnits(other.to_owned())),
        }
    }

    fn factor_mm(self) -> f64 {
        match self {
            Self::Millimeters => 1.0,
            Self::Centimeters => 10.0,
            Self::Inches => 25.4,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Millimeters => "millimeters",
            Self::Centimeters => "centimeters",
            Self::Inches => "inches",
        }
    }
}

impl ImportOptions {
    pub fn new(mesh_type: &str, units: &str, show_import_log: bool) -> Result<Self, ImportError> {
        let mesh_type = MESH_TYPES
            .iter()
            .find(|entry| entry.id == mesh_type)
            .map(|entry| entry.id)
            .ok_or_else(|| ImportError::UnsupportedMeshType(mesh_type.to_owned()))?;
        Ok(Self {
            mesh_type,
            units: LengthUnit::parse(units)?,
            show_import_log,
        })
    }
}

pub struct SourceSnapshot {
    pub path: PathBuf,
    pub source_name: String,
    pub format: ImportFormat,
    pub bytes: Vec<u8>,
    pub modified_unix_ms: Option<u64>,
}

/// 为导入来源计算稳定的 SHA-256 快照标识。
pub fn source_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn detect_format(path: &Path) -> Result<ImportFormat, ImportError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match extension.to_ascii_lowercase().as_str() {
        "stl" => Ok(ImportFormat::Stl),
        "vtu" => Ok(ImportFormat::Vtu),
        "vtp" => Ok(ImportFormat::Vtp),
        "vtk" => Ok(ImportFormat::VtkLegacy),
        "obj" => Ok(ImportFormat::Obj),
        "ply" => Ok(ImportFormat::Ply),
        "step" | "stp" => Ok(ImportFormat::Step),
        "iges" | "igs" => Ok(ImportFormat::Iges),
        _ => Err(ImportError::UnsupportedFormat(path.display().to_string())),
    }
}

pub fn read_source(path: &Path) -> Result<SourceSnapshot, ImportError> {
    // 永不取消的同步入口：Cancelled 分支按构造不可达。
    match read_source_checked(path, || false) {
        Ok(snapshot) => Ok(snapshot),
        Err(CheckedReadError::Failed(error)) => Err(error),
        Err(CheckedReadError::Cancelled(_)) => {
            unreachable!("a read that never checks cancellation cannot be cancelled")
        }
    }
}

/// 异步激活（073）使用的可取消读取错误：取消与读失败分开表达，
/// 不混入共享的 [`ImportError`]。
#[derive(Debug)]
pub enum CheckedReadError {
    Cancelled(String),
    Failed(ImportError),
}

impl Display for CheckedReadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CheckedReadError {}

/// 每个读取块（256 KiB）后查询一次取消检查点。
const READ_CHUNK_BYTES: usize = 256 * 1024;

/// 与 [`read_source`] 相同的读取契约，但分块执行并在每个检查点查询
/// `is_cancelled`；取消与 I/O 失败分别返回，供异步激活流程在安全点停止。
pub fn read_source_checked(
    path: &Path,
    is_cancelled: impl Fn() -> bool,
) -> Result<SourceSnapshot, CheckedReadError> {
    if !path.is_absolute() || !path.is_file() {
        return Err(CheckedReadError::Failed(ImportError::Missing(
            path.display().to_string(),
        )));
    }
    let format = detect_format(path).map_err(CheckedReadError::Failed)?;
    let source_name = source_name(path).map_err(CheckedReadError::Failed)?;
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return Err(read_error(path, error)),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => return Err(read_error(path, error)),
    };
    let expected = metadata.len();
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok());
    let mut bytes = Vec::new();
    reserve_source_bytes(
        path,
        &mut bytes,
        usize::try_from(expected).unwrap_or(usize::MAX),
    )?;
    let mut chunk = vec![0u8; READ_CHUNK_BYTES];
    loop {
        if is_cancelled() {
            return Err(CheckedReadError::Cancelled(path.display().to_string()));
        }
        let read = match std::io::Read::read(&mut file, &mut chunk) {
            Ok(read) => read,
            Err(error) => return Err(read_error(path, error)),
        };
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(SourceSnapshot {
        path: path.to_path_buf(),
        source_name,
        format,
        bytes,
        modified_unix_ms,
    })
}

fn source_name(path: &Path) -> Result<String, ImportError> {
    path.file_name()
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string_lossy().into_owned())
        .ok_or_else(|| ImportError::UnsupportedFormat(path.display().to_string()))
}

fn read_error(path: &Path, error: std::io::Error) -> CheckedReadError {
    CheckedReadError::Failed(ImportError::Read(format!("{}: {error}", path.display())))
}

fn reserve_source_bytes(
    path: &Path,
    bytes: &mut Vec<u8>,
    capacity: usize,
) -> Result<(), CheckedReadError> {
    match bytes.try_reserve_exact(capacity) {
        Ok(()) => Ok(()),
        Err(error) => Err(CheckedReadError::Failed(ImportError::Read(format!(
            "{}: allocation failed: {error}",
            path.display()
        )))),
    }
}

/// 唯一的表面网格解析入口；STEP / IGES 等几何格式尚无 native backend，不得走此函数。
pub fn parse_mesh_snapshot(snapshot: &SourceSnapshot) -> Result<SurfaceMesh, ImportError> {
    let bytes = &snapshot.bytes;
    match snapshot.format {
        ImportFormat::Stl => parse_stl(bytes).map_err(ImportError::Parse),
        ImportFormat::Vtu => formats::parse_vtu(bytes).map_err(ImportError::Format),
        ImportFormat::Vtp => formats::parse_vtp(bytes).map_err(ImportError::Format),
        ImportFormat::VtkLegacy => formats::parse_vtk_legacy(bytes).map_err(ImportError::Format),
        ImportFormat::Obj => formats::parse_obj(bytes).map_err(ImportError::Format),
        ImportFormat::Ply => formats::parse_ply(bytes).map_err(ImportError::Format),
        ImportFormat::Step | ImportFormat::Iges => Err(ImportError::UnsupportedFormat(
            snapshot.path.display().to_string(),
        )),
    }
}

/// 预览与提交共享同一字节快照；提交前重新读取源文件并逐字节比较。
#[derive(Debug, Default)]
pub struct StlImportSession {
    preview: Option<PreparedStl>,
}

#[derive(Debug)]
pub struct PreparedStl {
    pub path: PathBuf,
    pub source_name: String,
    pub format: ImportFormat,
    pub bytes: Vec<u8>,
    pub mesh: SurfaceMesh,
    pub modified_unix_ms: Option<u64>,
}

impl PreparedStl {
    pub fn scale_mesh_mm(&mut self, options: ImportOptions) -> Result<(), ImportError> {
        scale_mesh_mm(&mut self.mesh, options.units)
    }

    pub fn record(&self, import_number: usize, options: ImportOptions) -> ImportRecord {
        let summary = self.summary();
        ImportRecord {
            record_version: IMPORT_RECORD_VERSION,
            parser_version: STL_IMPORT_PARSER_VERSION,
            id: format!("import-{import_number}"),
            display_name: default_display_name(&self.source_name, self.format.id()),
            source_name: self.source_name.clone(),
            source_path: self.path.to_string_lossy().into_owned(),
            source_digest: Some(source_digest(&self.bytes)),
            source_size_bytes: u64::try_from(self.bytes.len()).unwrap_or(u64::MAX),
            source_modified_unix_ms: self.modified_unix_ms,
            mesh_asset_id: format!("mesh-import-{import_number}"),
            format: self.format.id().to_owned(),
            mesh_type: options.mesh_type.to_owned(),
            units: options.units.label().to_owned(),
            show_import_log: options.show_import_log,
            triangle_count: summary.triangle_count,
            dimensions: summary.dimensions,
        }
    }

    pub fn summary(&self) -> SurfaceSummary {
        self.mesh.summary()
    }
}

impl StlImportSession {
    pub fn clear(&mut self) {
        self.preview = None;
    }

    /// 当前预检解析出的来源坐标网格，供只读诊断复用，避免重复读取与解析。
    pub fn preview_mesh(&self) -> Option<&SurfaceMesh> {
        self.preview.as_ref().map(|preview| &preview.mesh)
    }

    pub fn has_preview_for(&self, path: &Path) -> bool {
        self.preview
            .as_ref()
            .is_some_and(|preview| preview.path == path)
    }

    pub fn preview(&mut self, path: &Path) -> Result<StlImportPreview, ImportError> {
        match self.preview_checked(path, || false) {
            Ok(preview) => Ok(preview),
            Err(CheckedReadError::Failed(error)) => Err(error),
            Err(CheckedReadError::Cancelled(_)) => unreachable!("uncancellable preview"),
        }
    }

    /// 后台预检使用分块读取检查取消；解析不能中断，解析后的快照发布前再次检查。
    /// 取消与失败都清空本次预览，不改变源文件。
    pub fn preview_checked(
        &mut self,
        path: &Path,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<StlImportPreview, CheckedReadError> {
        self.preview = None;
        let input = read_source_checked(path, &is_cancelled)?;
        let mesh = parse_mesh_snapshot(&input).map_err(CheckedReadError::Failed)?;
        if is_cancelled() {
            return Err(CheckedReadError::Cancelled(path.display().to_string()));
        }
        let summary = mesh.summary();
        let source_name = input.source_name.clone();
        self.preview = Some(PreparedStl {
            path: input.path,
            source_name: input.source_name,
            format: input.format,
            bytes: input.bytes,
            mesh,
            modified_unix_ms: input.modified_unix_ms,
        });
        Ok(StlImportPreview::from_summary(source_name, summary))
    }

    pub fn prepare(&mut self, path: &Path) -> Result<PreparedStl, ImportError> {
        match self.prepare_checked(path, || false) {
            Ok(prepared) => Ok(prepared),
            Err(CheckedReadError::Failed(error)) => Err(error),
            Err(CheckedReadError::Cancelled(_)) => {
                unreachable!("an import preparation that never checks cancellation cannot cancel")
            }
        }
    }

    /// 后台确认导入复核源快照时可在分块读取边界取消。
    /// 源快照一致后复用预检网格，避免重复解析；取消或变化不消耗原预览。
    pub fn prepare_checked(
        &mut self,
        path: &Path,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<PreparedStl, CheckedReadError> {
        let input = read_source_checked(path, &is_cancelled)?;
        if !input.format.is_surface_mesh() {
            return Err(CheckedReadError::Failed(ImportError::UnsupportedFormat(
                path.display().to_string(),
            )));
        }
        if let Some(cached) = self.preview.as_ref().filter(|cached| cached.path == path) {
            if cached.bytes != input.bytes {
                return Err(CheckedReadError::Failed(ImportError::SourceChanged(
                    path.display().to_string(),
                )));
            }
            if is_cancelled() {
                return Err(CheckedReadError::Cancelled(path.display().to_string()));
            }
            if let Some(mut confirmed) = self.preview.take() {
                confirmed.modified_unix_ms = input.modified_unix_ms;
                return Ok(confirmed);
            }
        }
        let mesh = parse_mesh_snapshot(&input).map_err(CheckedReadError::Failed)?;
        if is_cancelled() {
            return Err(CheckedReadError::Cancelled(path.display().to_string()));
        }
        Ok(PreparedStl {
            path: input.path,
            source_name: input.source_name,
            format: input.format,
            bytes: input.bytes,
            mesh,
            modified_unix_ms: input.modified_unix_ms,
        })
    }
}

fn scale_mesh_mm(mesh: &mut SurfaceMesh, units: LengthUnit) -> Result<(), ImportError> {
    let factor = units.factor_mm();
    for triangle in &mut mesh.triangles {
        for point in triangle {
            for coordinate in point {
                *coordinate *= factor;
                if !coordinate.is_finite() {
                    return Err(ImportError::CoordinateOverflow);
                }
            }
        }
    }
    if mesh
        .summary()
        .dimensions
        .iter()
        .any(|size| !size.is_finite())
    {
        return Err(ImportError::CoordinateOverflow);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("panta-import-{}-{id}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root)?;
            Ok(Self { root })
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn recognizes_current_and_future_import_formats() {
        assert_eq!(
            detect_format(Path::new("a.STL")).ok(),
            Some(ImportFormat::Stl)
        );
        assert_eq!(
            detect_format(Path::new("a.stp")).ok(),
            Some(ImportFormat::Step)
        );
        assert_eq!(
            detect_format(Path::new("a.IGS")).ok(),
            Some(ImportFormat::Iges)
        );
        assert!(detect_format(Path::new("a.txt")).is_err());
    }

    #[test]
    fn display_name_strips_only_the_declared_format_extension() {
        assert_eq!(default_display_name("part.stl", "stl"), "part");
        assert_eq!(default_display_name("Case.STL", "stl"), "Case");
        assert_eq!(default_display_name("零件.模型.stl", "stl"), "零件.模型");
        assert_eq!(default_display_name("part", "stl"), "part");
        assert_eq!(default_display_name("part.stl.bak", "stl"), "part.stl.bak");
        assert_eq!(default_display_name("part.step", "step"), "part");
        assert_eq!(default_display_name("part.stl", "step"), "part.stl");
    }

    #[test]
    fn preview_reports_expanded_asset_cost_without_rejecting_large_counts() {
        let preview = |triangle_count| {
            StlImportPreview::from_summary(
                "model.stl".to_owned(),
                SurfaceSummary {
                    triangle_count,
                    dimensions: [1.0; 3],
                },
            )
        };
        assert_eq!(preview(1).expanded_mesh_bytes, 136);
        let below = preview(29_826_160);
        assert_eq!(below.expanded_mesh_bytes, 2_147_483_584);
        assert!(!below.large_mesh_warning);
        let above = preview(29_826_161);
        assert_eq!(above.expanded_mesh_bytes, 2_147_483_656);
        assert!(above.large_mesh_warning);
        let overflowing = preview(u64::MAX);
        assert_eq!(overflowing.expanded_mesh_bytes, u64::MAX);
        assert!(overflowing.large_mesh_warning);
        assert_eq!(overflowing.triangle_count, u64::MAX);
    }

    #[test]
    fn preview_cancellation_before_read_and_after_parse_does_not_publish_a_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let path = fixture.root.join("preview.stl");
        fs::write(&path, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
        let mut session = StlImportSession::default();
        assert!(matches!(
            session.preview_checked(&path, || true),
            Err(CheckedReadError::Cancelled(_))
        ));
        assert!(session.preview.is_none());
        let checks = AtomicUsize::new(0);
        // 小文件经过两次读检查（数据、EOF），第三次位于解析后发布前。
        assert!(matches!(
            session.preview_checked(&path, || checks.fetch_add(1, Ordering::Relaxed) >= 2),
            Err(CheckedReadError::Cancelled(_))
        ));
        assert!(session.preview.is_none());
        assert_eq!(session.preview(&path)?.triangle_count, 1);
        Ok(())
    }

    #[test]
    fn import_options_records_and_unit_scaling_are_consistent()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let source = fixture.root.join("part.stl");
        fs::write(
            &source,
            b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 2 0\nendsolid\n",
        )?;

        let millimeters = ImportOptions::new("midplane", "millimeters", false)?;
        let centimeters = ImportOptions::new("dual-domain", "centimeters", true)?;
        let inches = ImportOptions::new("solid-3d", "inches", false)?;
        assert!(matches!(
            ImportOptions::new("surface", "millimeters", false),
            Err(ImportError::UnsupportedMeshType(kind)) if kind == "surface"
        ));
        assert!(matches!(
            ImportOptions::new("solid-3d", "metres", false),
            Err(ImportError::UnsupportedUnits(units)) if units == "metres"
        ));
        assert_eq!(
            ImportError::CoordinateOverflow.to_string(),
            "CoordinateOverflow"
        );

        let mut session = StlImportSession::default();
        let preview = session.preview(&source)?;
        assert_eq!(preview.source_name, "part.stl");
        assert_eq!(preview.triangle_count, 1);
        assert_eq!(preview.dimensions, [1.0, 2.0, 0.0]);
        assert_eq!(preview.expanded_mesh_bytes, 136);
        assert!(!preview.large_mesh_warning);

        let prepared = session.prepare(&source)?;
        assert_eq!(prepared.summary().dimensions, preview.dimensions);
        let record = prepared.record(7, centimeters);
        assert_eq!(record.record_version, IMPORT_RECORD_VERSION);
        assert_eq!(record.parser_version, STL_IMPORT_PARSER_VERSION);
        assert_eq!(record.id, "import-7");
        assert_eq!(record.display_name, "part");
        assert_eq!(
            record.source_digest,
            Some(format!("sha256:{:x}", Sha256::digest(fs::read(&source)?)))
        );
        assert_eq!(record.mesh_asset_id, "mesh-import-7");
        assert_eq!(record.source_path, source.to_string_lossy());
        assert_eq!(record.source_size_bytes, fs::metadata(&source)?.len());
        assert_eq!(record.mesh_type, "dual-domain");
        assert_eq!(record.units, "centimeters");
        assert!(record.show_import_log);
        assert_eq!(record.triangle_count, 1);
        assert_eq!(record.dimensions, [1.0, 2.0, 0.0]);

        let mut millimeter_mesh = prepared;
        millimeter_mesh.scale_mesh_mm(millimeters)?;
        assert_eq!(millimeter_mesh.summary().dimensions, [1.0, 2.0, 0.0]);
        let mut inch_mesh = session.prepare(&source)?;
        inch_mesh.scale_mesh_mm(inches)?;
        assert_eq!(inch_mesh.summary().dimensions, [25.4, 50.8, 0.0]);

        session.preview(&source)?;
        session.clear();
        let cleared = session.prepare(&source)?;
        assert_eq!(cleared.summary().dimensions, [1.0, 2.0, 0.0]);
        Ok(())
    }

    #[test]
    fn source_reads_and_previews_reject_invalid_or_changed_inputs()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let source = fixture.root.join("part.stl");
        let original = b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n";
        fs::write(&source, original)?;

        assert!(matches!(
            detect_format(Path::new("extensionless")),
            Err(ImportError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            read_source(Path::new("relative.stl")),
            Err(ImportError::Missing(_))
        ));
        assert!(matches!(
            read_source(&fixture.root.join("missing.stl")),
            Err(ImportError::Missing(_))
        ));
        let unsupported = fixture.root.join("part.txt");
        fs::write(&unsupported, b"text")?;
        assert!(matches!(
            read_source(&unsupported),
            Err(ImportError::UnsupportedFormat(_))
        ));

        let non_stl = SourceSnapshot {
            path: fixture.root.join("part.step"),
            source_name: "part.step".to_owned(),
            format: ImportFormat::Step,
            bytes: original.to_vec(),
            modified_unix_ms: None,
        };
        assert!(matches!(
            parse_mesh_snapshot(&non_stl),
            Err(ImportError::UnsupportedFormat(_))
        ));
        let non_stl_path = fixture.root.join("part.step");
        fs::write(&non_stl_path, original)?;
        assert!(matches!(
            StlImportSession::default().prepare(&non_stl_path),
            Err(ImportError::UnsupportedFormat(_))
        ));

        let mut session = StlImportSession::default();
        session.preview(&source)?;
        fs::write(&source, b"vertex 0 0 0\nvertex 3 0 0\nvertex 0 4 0\n")?;
        assert!(matches!(
            session.prepare(&source),
            Err(ImportError::SourceChanged(_))
        ));
        session.clear();
        let updated = session.prepare(&source)?;
        assert_eq!(updated.summary().dimensions, [3.0, 4.0, 0.0]);

        fs::write(&source, b"invalid stl")?;
        assert!(matches!(
            session.preview(&source),
            Err(ImportError::Parse(_))
        ));
        fs::write(&source, original)?;
        let reread = session.prepare(&source)?;
        assert_eq!(reread.summary().dimensions, [1.0, 1.0, 0.0]);
        Ok(())
    }

    #[test]
    fn load_asset_scales_units_and_reports_coordinate_overflow()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let source = fixture.root.join("part.stl");
        fs::write(&source, b"vertex 0 0 0\nvertex 2 0 0\nvertex 0 3 0\n")?;
        let centimeters = ImportOptions::new("dual-domain", "centimeters", false)?;
        let mut mesh = StlImportSession::default().prepare(&source)?;
        mesh.scale_mesh_mm(centimeters)?;
        assert_eq!(mesh.summary().dimensions, [20.0, 30.0, 0.0]);
        assert!(matches!(
            ImportOptions::new("dual-domain", "yards", false),
            Err(ImportError::UnsupportedUnits(units)) if units == "yards"
        ));

        fs::write(
            &source,
            format!(
                "vertex 0 0 0\nvertex {} 0 0\nvertex 0 1 0\n",
                f64::MAX / 2.0
            ),
        )?;
        let mut scaled_mesh = StlImportSession::default().prepare(&source)?;
        let inches = ImportOptions::new("dual-domain", "inches", false)?;
        assert!(matches!(
            scaled_mesh.scale_mesh_mm(inches),
            Err(ImportError::CoordinateOverflow)
        ));

        // 有限坐标本身仍可能让包围盒的 max - min 溢出。
        let mut wide_mesh = SurfaceMesh {
            triangles: vec![[[-f64::MAX, 0.0, 0.0], [f64::MAX, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        };
        assert!(matches!(
            scale_mesh_mm(&mut wide_mesh, LengthUnit::Millimeters),
            Err(ImportError::CoordinateOverflow)
        ));
        Ok(())
    }

    #[test]
    fn checked_read_supports_cancellation_checkpoints() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let source = fixture.root.join("part.stl");
        let payload = b"vertex 0 0 0\nvertex 2 0 0\nvertex 0 3 0\n";
        fs::write(&source, payload)?;
        let snapshot = read_source_checked(&source, || false)?;
        assert_eq!(snapshot.bytes.len(), payload.len());

        assert!(matches!(
            read_source_checked(&source, || true),
            Err(CheckedReadError::Cancelled(_))
        ));
        let large_source = fixture.root.join("large.stl");
        fs::write(&large_source, vec![b' '; READ_CHUNK_BYTES + 1])?;
        let checkpoints = AtomicUsize::new(0);
        assert!(matches!(
            read_source_checked(&large_source, || {
                checkpoints.fetch_add(1, Ordering::Relaxed) == 1
            }),
            Err(CheckedReadError::Cancelled(path)) if path == large_source.display().to_string()
        ));
        // 第二次检查发生在首个 256 KiB 数据块读取后。
        assert_eq!(checkpoints.load(Ordering::Relaxed), 2);
        // 不存在的文件以 Failed(Missing) 表达，不冒充取消。
        assert!(matches!(
            read_source_checked(&fixture.root.join("absent.stl"), || false),
            Err(CheckedReadError::Failed(ImportError::Missing(_)))
        ));
        // 未登记扩展名在读取前即拒绝（与 read_source 同一契约）；
        // .step 能通过格式识别，拒绝发生在解析层，不属读取边界。
        let unknown = fixture.root.join("part.3mf");
        fs::write(&unknown, b"anything")?;
        assert!(matches!(
            read_source_checked(&unknown, || false),
            Err(CheckedReadError::Failed(ImportError::UnsupportedFormat(_)))
        ));

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;

            let non_utf8_name = std::ffi::OsString::from_vec(b"part-\xff.stl".to_vec());
            let non_utf8 = fixture.root.join(non_utf8_name);
            match fs::write(&non_utf8, payload) {
                Ok(()) => {
                    let source = read_source_checked(&non_utf8, || false)?;
                    assert_eq!(source.source_name, "part-\u{fffd}.stl");
                    fs::remove_file(non_utf8)?;
                }
                // Some filesystems reject non-UTF-8 names before they can be read.
                Err(error)
                    if error.kind() == std::io::ErrorKind::PermissionDenied
                        || error.kind() == std::io::ErrorKind::InvalidInput
                        || (cfg!(target_os = "macos") && error.raw_os_error() == Some(92)) =>
                {
                    assert_eq!(source_name(&non_utf8)?, "part-\u{fffd}.stl");
                }
                Err(error) => return Err(error.into()),
            }
        }

        assert!(matches!(
            read_error(&source, std::io::Error::other("fixture error")),
            CheckedReadError::Failed(ImportError::Read(message))
                if message.contains("part.stl") && message.contains("fixture error")
        ));
        let mut unreserved = Vec::new();
        assert!(matches!(
            reserve_source_bytes(&source, &mut unreserved, usize::MAX),
            Err(CheckedReadError::Failed(ImportError::Read(message)))
                if message.contains("part.stl") && message.contains("allocation failed")
        ));

        let cancelled = CheckedReadError::Cancelled(source.display().to_string());
        assert!(cancelled.to_string().contains("Cancelled"));
        let failed = CheckedReadError::Failed(ImportError::Missing(source.display().to_string()));
        assert!(failed.to_string().contains("Missing"));
        Ok(())
    }
}
