//! 体网格驻留数据与只读命令：切面显示、质量统计、VTU 导出（task 021）。
//! 命令作用于共享 `Arc<TetMesh>` 快照，可在宿主后台线程执行（task 022）。

use super::{ProjectError, ProjectService};
use panta_mesh::{Progress, SurfaceMesh, TetMesh, TetQuality};
use std::path::Path;
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

/// 与工程无关的体网格 VTU 导出（Netgen 生成结果复用同一写出路径），带进度与取消。
pub fn export_tet_mesh_vtu(
    volume: &TetMesh,
    path: &Path,
    progress: &Progress,
) -> Result<(), ProjectError> {
    panta_import::export::export_tet_mesh_vtu(volume, path, progress)
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
    let length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
    if !length.is_finite() || length == 0.0 || !fraction.is_finite() {
        return Err(ProjectError::CommandInvalid(
            "section normal or position is invalid".to_owned(),
        ));
    }
    let normal = normal.map(|value| value / length);
    let project =
        |point: &[f64; 3]| point[0] * normal[0] + point[1] * normal[1] + point[2] * normal[2];
    let (low, high) =
        volume
            .nodes
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), point| {
                let offset = project(point);
                (low.min(offset), high.max(offset))
            });
    let offset = low + (high - low) * fraction.clamp(0.0, 1.0);
    let origin = normal.map(|value| value * offset);
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
        let written = panta_import::formats::parse_vtu_grid(&std::fs::read(&out)?)?;
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
}
