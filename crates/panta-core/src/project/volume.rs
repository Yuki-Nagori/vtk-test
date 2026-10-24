//! 体网格驻留数据与只读命令：切面显示、质量统计、VTU 导出（task 021）。
//! 命令作用于共享 `Arc<TetMesh>` 快照，可在宿主后台线程执行（task 022）。

use super::{ProjectError, ProjectService};
use panta_mesh::chunked::{ChunkError, ChunkedVolume};
use panta_mesh::{
    FieldAssociation, FieldComponent, FieldSection, Progress, ResultField, SurfaceAssetChunks,
    SurfaceMesh, TetMesh, TetQuality, VolumeAssetChunks, VolumeStreamCounts, VolumeStreamError,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

impl ProjectService {
    pub(super) fn set_volume_mesh(&mut self, import_id: &str, volume: Option<TetMesh>) {
        match volume {
            Some(volume) => {
                self.volume_meshes
                    .insert(import_id.to_owned(), Arc::new(volume));
            }
            None => {
                self.volume_meshes.remove(import_id);
            }
        }
    }

    /// 驻留网格来自体网格资产时返回四面体数据；表面网格或未驻留返回 `None`。
    pub fn volume_for_import(&self, import_id: &str) -> Option<Arc<TetMesh>> {
        self.mesh_cache.get(import_id)?;
        self.volume_meshes.get(import_id).cloned()
    }
}

/// 只读命令按需流式读取的完整网格资产（task 025）：表面显示 LOD 或单元未驻留的体网格。
/// 路径已通过工程包含关系校验；`digest` 为激活时的资产摘要，读取前再次核对。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamedAsset {
    pub path: PathBuf,
    pub digest: String,
    pub kind: StreamedKind,
    /// 激活时打开的派生分块文件（task 028）：切面与质量按需读块；`None` 时逐次流式扫描 v3 资产。
    pub chunks: Option<SharedChunks>,
}

/// 共享的分块体网格句柄；相等性按同一打开实例判断。
#[derive(Clone)]
pub struct SharedChunks(pub Arc<ChunkedVolume>);

impl std::fmt::Debug for SharedChunks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedChunks")
            .field("tets", &self.0.tet_count())
            .field("reader", &self.0.reader_kind())
            .finish_non_exhaustive()
    }
}

impl PartialEq for SharedChunks {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedChunks {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamedKind {
    /// 驻留网格为显示 LOD，值为源资产面数。
    Surface { triangles: u64 },
    /// 体网格单元未驻留，驻留网格为精确外表面。
    Volume(VolumeStreamCounts),
}

impl ProjectService {
    /// 驻留网格需要流式访问完整资产时返回其描述；完整驻留、未驻留或资产索引无效返回 `None`。
    pub fn streamed_asset_for_import(&self, import_id: &str) -> Option<StreamedAsset> {
        self.mesh_cache.get(import_id)?;
        let kind = match (
            self.streamed_volumes.get(import_id),
            self.mesh_lod_sources.get(import_id),
        ) {
            (Some(counts), _) => StreamedKind::Volume(*counts),
            (None, Some(&triangles)) => StreamedKind::Surface { triangles },
            (None, None) => return None,
        };
        let request = self.activation_request(import_id).ok()?;
        let path =
            super::service::resolve_asset_path(&request.project_root, &request.relative_path)
                .ok()?;
        let chunks = match kind {
            StreamedKind::Volume(_) => self
                .chunked_volumes
                .get(import_id)
                .cloned()
                .map(SharedChunks),
            StreamedKind::Surface { .. } => None,
        };
        Some(StreamedAsset {
            path,
            digest: request.expected_digest,
            kind,
            chunks,
        })
    }
}

fn open_volume(asset: &StreamedAsset) -> Result<VolumeAssetChunks, ProjectError> {
    let chunks = VolumeAssetChunks::open(&asset.path).map_err(|error| {
        ProjectError::StreamedAssetFailed(format!("{}: {error}", asset.path.display()))
    })?;
    if chunks.digest() != asset.digest {
        return Err(ProjectError::StreamedAssetFailed(format!(
            "{}: digest changed",
            asset.path.display()
        )));
    }
    Ok(chunks)
}

fn stream_error(asset: &StreamedAsset, error: VolumeStreamError) -> ProjectError {
    match error {
        VolumeStreamError::Asset(error) => {
            ProjectError::StreamedAssetFailed(format!("{}: {error}", asset.path.display()))
        }
        VolumeStreamError::Invalid(detail) => ProjectError::CommandInvalid(detail),
    }
}

/// 流式导出表面资产（显示 LOD 状态）的完整网格；STL 与驻留导出一致，带索引格式不焊接。
pub fn export_streamed_surface(
    asset: &StreamedAsset,
    target: &Path,
    progress: &Progress,
) -> Result<panta_io::export::ExportFormat, ProjectError> {
    let mut chunks = SurfaceAssetChunks::open(&asset.path).map_err(|error| {
        ProjectError::ExportSourceUnavailable(format!("{}: {error}", asset.path.display()))
    })?;
    if chunks.digest() != asset.digest {
        return Err(ProjectError::ExportSourceUnavailable(format!(
            "{}: digest changed",
            asset.path.display()
        )));
    }
    panta_io::export::export_surface_asset_with_progress(&mut chunks, target, progress)
        .map_err(super::service::export_error)
}

/// 流式导出单元未驻留的体网格为 `.vtu`（与驻留导出逐字节一致）。
pub fn export_streamed_volume_vtu(
    asset: &StreamedAsset,
    target: &Path,
    progress: &Progress,
) -> Result<(), ProjectError> {
    let mut chunks = open_volume(asset)
        .map_err(|error| ProjectError::ExportSourceUnavailable(error.detail()))?;
    panta_io::export::export_volume_asset_vtu(&mut chunks, target, progress)
        .map_err(super::service::export_error)
}

fn chunk_error(error: ChunkError) -> ProjectError {
    match error {
        ChunkError::Invalid(detail) => ProjectError::CommandInvalid(detail),
        other => ProjectError::StreamedAssetFailed(format!("derived chunk file: {other}")),
    }
}

/// 流式体网格质量统计（结果与驻留统计一致）；取消返回 `Ok(None)`。
/// 有派生分块文件时按块并行统计并缓存每块结果（task 028），否则顺序扫描 v3 资产。
pub fn streamed_volume_quality(
    asset: &StreamedAsset,
    progress: &Progress,
) -> Result<Option<TetQuality>, ProjectError> {
    if let Some(SharedChunks(volume)) = &asset.chunks {
        return volume
            .quality(&|| progress.is_cancelled())
            .map_err(chunk_error);
    }
    let mut chunks = open_volume(asset)?;
    panta_mesh::tet_quality_streaming(&mut chunks, progress)
        .map_err(|error| stream_error(asset, error))
}

/// 流式任意平面切面（与 [`section_plane`] 的三角形集合一致，顺序可能不同）。
/// 有派生分块文件时只读命中块（task 028），否则每次调用重新扫描单元流。
pub fn streamed_section_plane(
    asset: &StreamedAsset,
    normal: [f64; 3],
    fraction: f64,
    progress: &Progress,
) -> Result<Option<SurfaceMesh>, ProjectError> {
    validate_plane(normal, fraction)?;
    if let Some(SharedChunks(volume)) = &asset.chunks {
        let plane = volume
            .plane_through_nodes(normal, fraction)
            .map_err(chunk_error)?;
        return volume
            .fine_section(&plane, &|| progress.is_cancelled())
            .map_err(chunk_error);
    }
    let mut chunks = open_volume(asset)?;
    panta_mesh::section_plane_streaming(&mut chunks, normal, fraction, progress)
        .map_err(|error| stream_error(asset, error))
}

/// 单元未驻留体网格的场着色切面（task 029），几何与 [`streamed_section_plane`] 位级一致。
/// 只在派生分块文件带该字段时可用：逐次扫描 v3 资产的回退路径不支持场剖面
/// （每次都要再读整个结果场），返回 `CommandInvalid`，调用方退回无场切面。
pub fn streamed_section_plane_field(
    asset: &StreamedAsset,
    name: &str,
    association: FieldAssociation,
    component: FieldComponent,
    normal: [f64; 3],
    fraction: f64,
    progress: &Progress,
) -> Result<Option<FieldSection>, ProjectError> {
    validate_plane(normal, fraction)?;
    let Some(SharedChunks(volume)) = &asset.chunks else {
        return Err(ProjectError::CommandInvalid(
            "field sections need the derived chunk file".to_owned(),
        ));
    };
    let field = volume.field_index(name, association).ok_or_else(|| {
        ProjectError::CommandInvalid(format!(
            "result field {name} ({}) is not in the derived chunk file",
            association.as_str()
        ))
    })?;
    let plane = volume
        .plane_through_nodes(normal, fraction)
        .map_err(chunk_error)?;
    volume
        .fine_section_field(&plane, field, component, &|| progress.is_cancelled())
        .map_err(chunk_error)
}

/// 驻留体网格的场着色切面（task 029），几何与 [`section_plane`] 位级一致：
/// 节点场线性插值，单元场取所在单元值，缺失值为 NaN。`field` 由
/// [`ProjectService::read_result_field`] 读出（整字段顺序大块读取）。
pub fn section_plane_field(
    volume: &TetMesh,
    field: &ResultField,
    component: FieldComponent,
    normal: [f64; 3],
    fraction: f64,
    progress: &Progress,
) -> Result<Option<FieldSection>, ProjectError> {
    let normal = validate_plane(normal, fraction)?;
    let origin = panta_mesh::section_plane_origin(&volume.nodes, normal, fraction);
    panta_mesh::section_surface_field(volume, field, component, origin, normal, progress)
        .map_err(|error| ProjectError::CommandInvalid(error.to_string()))
}

/// 校验切面参数并返回单位法向。
fn validate_plane(normal: [f64; 3], fraction: f64) -> Result<[f64; 3], ProjectError> {
    let length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
    if !length.is_finite() || length == 0.0 || !fraction.is_finite() {
        return Err(ProjectError::CommandInvalid(
            "section normal or position is invalid".to_owned(),
        ));
    }
    Ok(normal.map(|value| value / length))
}

/// 与工程无关的体网格 VTU 导出（Netgen 生成结果复用同一写出路径），带进度与取消。
pub fn export_tet_mesh_vtu(
    volume: &TetMesh,
    path: &Path,
    progress: &Progress,
) -> Result<(), ProjectError> {
    panta_io::export::export_tet_mesh_vtu(volume, path, progress)
        .map_err(super::service::export_error)
}

/// 体网格质量统计；取消返回 `None`。
pub fn volume_quality(volume: &TetMesh, progress: &Progress) -> Option<TetQuality> {
    panta_mesh::tet_quality(volume, progress)
}

/// 任意单平面切面显示网格（task 024）。`normal` 为平面法向（内部单位化），
/// `fraction` 为体网格节点在法向上投影区间的 0..=1 位置；轴向切面即法向取坐标轴。
/// 只用于显示，不生成新的网格资产；取消返回 `Ok(None)`。
pub fn section_plane(
    volume: &TetMesh,
    normal: [f64; 3],
    fraction: f64,
    progress: &Progress,
) -> Result<Option<SurfaceMesh>, ProjectError> {
    let normal = validate_plane(normal, fraction)?;
    let origin = panta_mesh::section_plane_origin(&volume.nodes, normal, fraction);
    panta_mesh::section_surface(volume, origin, normal, progress)
        .map_err(|error| ProjectError::CommandInvalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{export_tet_mesh_vtu, section_plane, volume_quality};
    use crate::project::{ProjectError, ProjectService, temp_directory};
    use panta_mesh::Progress;
    use std::path::PathBuf;
    use std::time::Duration;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mesh/formats")
            .join(name)
    }

    #[test]
    fn solid_vtu_imports_as_volume_and_survives_reopen() -> Result<(), Box<dyn std::error::Error>> {
        let fixture_dir = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture_dir.root, "Volume")?;
        let record = service.import_stl(
            &fixture("tet_ball_ascii.vtu"),
            "solid-3d",
            "centimeters",
            false,
        )?;
        let volume = service
            .volume_for_import(&record.id)
            .ok_or("volume missing")?;
        assert_eq!(volume.boundary.len(), 64);
        let progress = Progress::new();
        let quality = volume_quality(&volume, &progress).ok_or("quality cancelled")?;
        assert_eq!(quality.tet_count, volume.tets.len());
        assert!(quality.min_dihedral_degrees > 0.0);
        let section =
            section_plane(&volume, [0.0, 0.0, 1.0], 0.5, &progress)?.ok_or("section cancelled")?;
        assert!(!section.triangles.is_empty());
        for normal in [[0.0, 0.0, 0.0], [f64::NAN, 0.0, 1.0]] {
            assert!(matches!(
                section_plane(&volume, normal, 0.5, &progress),
                Err(ProjectError::CommandInvalid(_))
            ));
        }
        assert!(matches!(
            section_plane(&volume, [0.0, 0.0, 1.0], f64::NAN, &progress),
            Err(ProjectError::CommandInvalid(_))
        ));
        let cancelled = Progress::new();
        cancelled.cancel();
        assert!(volume_quality(&volume, &cancelled).is_none());
        assert!(section_plane(&volume, [1.0, 0.0, 0.0], 0.5, &cancelled)?.is_none());
        let out = fixture_dir.root.join("out.vtu");
        assert!(matches!(
            export_tet_mesh_vtu(&volume, &out, &cancelled),
            Err(ProjectError::ExportCancelled)
        ));
        assert!(!out.exists());
        export_tet_mesh_vtu(&volume, &out, &progress)?;
        let written = panta_io::import::formats::parse_vtu_grid(&std::fs::read(&out)?)?;
        assert_eq!(written.tets.len(), volume.tets.len());
        // 厘米换算到毫米：VTK 夹具半径约 0.5，换算后外包尺寸应 > 5 mm。
        let span = written
            .points
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max)
            - written
                .points
                .iter()
                .map(|point| point[0])
                .fold(f64::INFINITY, f64::min);
        assert!(span > 5.0, "{span}");

        // 表面类型仍按表面导入，无体网格数据。
        let surface = service.import_stl(
            &fixture("tet_ball_ascii.vtu"),
            "dual-domain",
            "millimeters",
            false,
        )?;
        assert!(service.volume_for_import(&surface.id).is_none());
        // 体素块含非四面体单元，实体 3D 整体拒绝。
        assert!(matches!(
            service.import_stl(
                &fixture("voxel_block_binary.vtu"),
                "solid-3d",
                "millimeters",
                false
            ),
            Err(ProjectError::ImportVolumeUnsupported(_))
        ));

        let mut reopened = ProjectService::new();
        reopened.open(&project.path)?;
        reopened.begin_asset_activation(&record.id)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(outcome) = reopened.drain_asset_activations().into_iter().next() {
                assert_eq!(outcome.kind, crate::activation::OutcomeKind::Succeeded);
                assert_eq!(outcome.mesh.ok_or("no mesh")?.triangles.len(), 64);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "activation timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        let reopened_volume = reopened
            .volume_for_import(&record.id)
            .ok_or("volume lost")?;
        assert_eq!(reopened_volume.tets.len(), volume.tets.len());
        Ok(())
    }

    #[test]
    fn oblique_section_matches_axis_section_and_keeps_the_negative_side()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture_dir = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        service.create(&fixture_dir.root, "Section")?;
        let record = service.import_stl(
            &fixture("tet_ball_ascii.vtu"),
            "solid-3d",
            "millimeters",
            false,
        )?;
        let volume = service
            .volume_for_import(&record.id)
            .ok_or("volume missing")?;
        let progress = Progress::new();

        // 轴向法向：与按 Z 包围盒直接构造的平面完全一致，且法向长度不影响结果。
        let (low, high) = volume
            .nodes
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), point| {
                (low.min(point[2]), high.max(point[2]))
            });
        let origin = [0.0, 0.0, low + (high - low) * 0.3];
        let expected = panta_mesh::section_surface(&volume, origin, [0.0, 0.0, 1.0], &progress)?
            .ok_or("section cancelled")?;
        let axis =
            section_plane(&volume, [0.0, 0.0, 1.0], 0.3, &progress)?.ok_or("section cancelled")?;
        let scaled =
            section_plane(&volume, [0.0, 0.0, 2.5], 0.3, &progress)?.ok_or("section cancelled")?;
        assert_eq!(axis, expected);
        assert_eq!(scaled, expected);

        // 斜法向：所有顶点位于平面的负侧（含平面），且切口顶点落在平面上。
        let normal = [1.0 / 2f64.sqrt(), 1.0 / 2f64.sqrt(), 0.0];
        let (low, high) =
            volume
                .nodes
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), point| {
                    let offset = point[0] * normal[0] + point[1] * normal[1];
                    (low.min(offset), high.max(offset))
                });
        let offset = low + (high - low) * 0.5;
        let oblique =
            section_plane(&volume, [1.0, 1.0, 0.0], 0.5, &progress)?.ok_or("section cancelled")?;
        assert!(!oblique.triangles.is_empty());
        let mut on_plane = 0usize;
        for vertex in oblique.triangles.iter().flatten() {
            let signed = vertex[0] * normal[0] + vertex[1] * normal[1] - offset;
            assert!(signed <= 1e-9, "{signed}");
            if signed.abs() <= 1e-9 {
                on_plane += 1;
            }
        }
        assert!(on_plane > 0);
        Ok(())
    }

    #[test]
    fn chunked_streamed_commands_match_resident_commands() -> Result<(), Box<dyn std::error::Error>>
    {
        use super::{
            SharedChunks, StreamedAsset, StreamedKind, export_streamed_volume_vtu,
            streamed_section_plane, streamed_volume_quality,
        };
        let fixture_dir = temp_directory::Fixture::new()?;
        let block = panta_mesh::SyntheticTetBlock::with_cells_per_axis(5);
        let path = fixture_dir.root.join("volume.pmesh");
        let (_, digest) = panta_mesh::write_volume_asset_streaming(
            std::fs::File::create(&path)?,
            block.counts(),
            &block,
            None,
        )?
        .ok_or("cancelled")?;
        let chunk_path = fixture_dir.root.join("volume.pchunk");
        panta_mesh::chunked::build_chunked_from_asset(
            &mut panta_mesh::VolumeAssetChunks::open(&path)?,
            panta_mesh::chunked::ChunkBuildOptions {
                max_chunk_cells: 100,
                simplify_ratio: 4,
            },
            std::fs::File::create(&chunk_path)?,
            &Progress::new(),
        )?
        .ok_or("cancelled")?;
        let chunks = std::sync::Arc::new(panta_mesh::chunked::ChunkedVolume::open_file(
            &chunk_path,
            64 << 10,
        )?);
        let len = std::fs::metadata(&path)?.len();
        let (decoded, _) = panta_mesh::MeshAssetReader::open(std::fs::File::open(&path)?, len)?
            .read(|| false)?
            .ok_or("cancelled")?;
        let panta_mesh::MeshAsset::Volume(volume) = decoded else {
            return Err("expected volume".into());
        };
        let asset = StreamedAsset {
            path: path.clone(),
            digest,
            kind: StreamedKind::Volume(block.counts()),
            chunks: Some(SharedChunks(std::sync::Arc::clone(&chunks))),
        };
        assert_eq!(asset.clone(), asset);
        let progress = Progress::new();
        let sorted = |mesh: panta_mesh::SurfaceMesh| {
            let mut keys: Vec<[u64; 9]> = mesh
                .triangles
                .iter()
                .map(|t| std::array::from_fn(|i| t[i / 3][i % 3].to_bits()))
                .collect();
            keys.sort_unstable();
            keys
        };
        for (normal, fraction) in [([1.0, 2.0, -0.5], 0.3), ([0.0, 0.0, 1.0], 0.5)] {
            let streamed =
                streamed_section_plane(&asset, normal, fraction, &progress)?.ok_or("cancelled")?;
            let resident =
                section_plane(&volume, normal, fraction, &progress)?.ok_or("cancelled")?;
            assert_eq!(sorted(streamed), sorted(resident));
        }
        let quality = streamed_volume_quality(&asset, &progress)?.ok_or("cancelled")?;
        let resident = volume_quality(&volume, &progress).ok_or("cancelled")?;
        assert_eq!(quality.tet_count, resident.tet_count);
        assert_eq!(quality.poor_count, resident.poor_count);
        assert_eq!(quality.min_dihedral_degrees, resident.min_dihedral_degrees);
        assert_eq!(quality.max_aspect_ratio, resident.max_aspect_ratio);
        assert!(matches!(
            streamed_section_plane(&asset, [0.0; 3], 0.3, &progress),
            Err(ProjectError::CommandInvalid(_))
        ));
        // 导出仍走 v3 顺序流式路径，与驻留导出逐字节一致。
        let streamed_out = fixture_dir.root.join("streamed.vtu");
        let resident_out = fixture_dir.root.join("resident.vtu");
        export_streamed_volume_vtu(&asset, &streamed_out, &progress)?;
        export_tet_mesh_vtu(&volume, &resident_out, &progress)?;
        assert_eq!(std::fs::read(&streamed_out)?, std::fs::read(&resident_out)?);
        let cancelled = Progress::new();
        cancelled.cancel();
        // 每块质量结果已缓存：再次统计不读块，取消标志不影响直接返回。
        assert_eq!(
            streamed_volume_quality(&asset, &cancelled)?.map(|value| value.tet_count),
            Some(resident.tet_count)
        );
        Ok(())
    }

    #[test]
    fn streamed_commands_match_resident_commands_and_check_the_digest()
    -> Result<(), Box<dyn std::error::Error>> {
        use super::{
            StreamedAsset, StreamedKind, export_streamed_volume_vtu, streamed_section_plane,
            streamed_volume_quality,
        };
        let fixture_dir = temp_directory::Fixture::new()?;
        let block = panta_mesh::SyntheticTetBlock::with_cells_per_axis(4);
        let path = fixture_dir.root.join("volume.pmesh");
        let (_, digest) = panta_mesh::write_volume_asset_streaming(
            std::fs::File::create(&path)?,
            block.counts(),
            &block,
            None,
        )?
        .ok_or("cancelled")?;
        let len = std::fs::metadata(&path)?.len();
        let (decoded, _) = panta_mesh::MeshAssetReader::open(std::fs::File::open(&path)?, len)?
            .read(|| false)?
            .ok_or("cancelled")?;
        let panta_mesh::MeshAsset::Volume(volume) = decoded else {
            return Err("expected volume".into());
        };
        let asset = StreamedAsset {
            path: path.clone(),
            digest,
            kind: StreamedKind::Volume(block.counts()),
            chunks: None,
        };
        let progress = Progress::new();
        let quality = streamed_volume_quality(&asset, &progress)?.ok_or("cancelled")?;
        let resident = volume_quality(&volume, &progress).ok_or("cancelled")?;
        assert_eq!(quality.poor_count, resident.poor_count);
        assert_eq!(quality.min_dihedral_degrees, resident.min_dihedral_degrees);
        let normal = [1.0, 2.0, -0.5];
        assert_eq!(
            streamed_section_plane(&asset, normal, 0.3, &progress)?,
            section_plane(&volume, normal, 0.3, &progress)?
        );
        assert!(matches!(
            streamed_section_plane(&asset, [0.0; 3], 0.3, &progress),
            Err(ProjectError::CommandInvalid(_))
        ));
        let streamed_out = fixture_dir.root.join("streamed.vtu");
        let resident_out = fixture_dir.root.join("resident.vtu");
        export_streamed_volume_vtu(&asset, &streamed_out, &progress)?;
        export_tet_mesh_vtu(&volume, &resident_out, &progress)?;
        assert_eq!(std::fs::read(&streamed_out)?, std::fs::read(&resident_out)?);

        let stale = StreamedAsset {
            digest: "sha256:stale".to_owned(),
            ..asset
        };
        assert!(matches!(
            streamed_volume_quality(&stale, &progress),
            Err(ProjectError::StreamedAssetFailed(_))
        ));
        assert!(matches!(
            export_streamed_volume_vtu(&stale, &streamed_out, &progress),
            Err(ProjectError::ExportSourceUnavailable(_))
        ));
        Ok(())
    }
}
