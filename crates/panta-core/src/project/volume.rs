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

/// 轴向单平面切面显示网格；`axis` 为 0/1/2，`fraction` 为沿轴包围盒的 0..=1 位置。
/// 只用于显示，不生成新的网格资产；取消返回 `Ok(None)`。
pub fn section_along_axis(
    volume: &TetMesh,
    axis: usize,
    fraction: f64,
    progress: &Progress,
) -> Result<Option<SurfaceMesh>, ProjectError> {
    if axis > 2 || !fraction.is_finite() {
        return Err(ProjectError::CommandInvalid(
            "section axis or position is invalid".to_owned(),
        ));
    }
    let (low, high) = volume
        .nodes
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), point| {
            (low.min(point[axis]), high.max(point[axis]))
        });
    let mut origin = [0.0; 3];
    origin[axis] = low + (high - low) * fraction.clamp(0.0, 1.0);
    let mut normal = [0.0; 3];
    normal[axis] = 1.0;
    panta_mesh::section_surface(volume, origin, normal, progress)
        .map_err(|error| ProjectError::CommandInvalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{export_tet_mesh_vtu, section_along_axis, volume_quality};
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
        let section = section_along_axis(&volume, 2, 0.5, &progress)?.ok_or("section cancelled")?;
        assert!(!section.triangles.is_empty());
        assert!(matches!(
            section_along_axis(&volume, 3, 0.5, &progress),
            Err(ProjectError::CommandInvalid(_))
        ));
        let cancelled = Progress::new();
        cancelled.cancel();
        assert!(volume_quality(&volume, &cancelled).is_none());
        assert!(section_along_axis(&volume, 0, 0.5, &cancelled)?.is_none());
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
}
