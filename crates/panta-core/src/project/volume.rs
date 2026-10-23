//! 体网格驻留数据与只读命令：切面显示、质量统计、VTU 导出（task 021）。

use super::{ProjectError, ProjectService};
use panta_mesh::{SurfaceMesh, TetMesh, TetQuality};
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

    fn require_volume(&self, import_id: &str) -> Result<Arc<TetMesh>, ProjectError> {
        self.volume_for_import(import_id)
            .ok_or_else(|| ProjectError::VolumeUnavailable(import_id.to_owned()))
    }

    /// 单平面切面显示网格；`axis` 为 0/1/2，`fraction` 为沿轴包围盒的 0..=1 位置。
    /// 只用于显示，不生成新的网格资产。
    pub fn volume_section(
        &self,
        import_id: &str,
        axis: usize,
        fraction: f64,
    ) -> Result<SurfaceMesh, ProjectError> {
        let volume = self.require_volume(import_id)?;
        section_along_axis(&volume, axis, fraction)
    }

    pub fn volume_quality(&self, import_id: &str) -> Result<TetQuality, ProjectError> {
        let volume = self.require_volume(import_id)?;
        Ok(panta_mesh::tet_quality(&volume, &|_| {}))
    }

    /// 体网格导出 VTU（原始节点与四面体单元）；表面格式仍走现有导出（外表面）。
    pub fn export_volume_vtu(&self, import_id: &str, path: &Path) -> Result<(), ProjectError> {
        let volume = self.require_volume(import_id)?;
        panta_import::export::export_tet_mesh_vtu(&volume, path)
            .map_err(|error| ProjectError::ExportWriteFailed(error.to_string()))
    }
}

pub fn section_along_axis(
    volume: &TetMesh,
    axis: usize,
    fraction: f64,
) -> Result<SurfaceMesh, ProjectError> {
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
    panta_mesh::section_surface(volume, origin, normal)
        .map_err(|error| ProjectError::CommandInvalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::project::{ProjectError, ProjectService, temp_directory};
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
        let quality = service.volume_quality(&record.id)?;
        assert_eq!(quality.tet_count, volume.tets.len());
        assert!(quality.min_dihedral_degrees > 0.0);
        let section = service.volume_section(&record.id, 2, 0.5)?;
        assert!(!section.triangles.is_empty());
        let out = fixture_dir.root.join("out.vtu");
        service.export_volume_vtu(&record.id, &out)?;
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
        assert!(matches!(
            service.volume_quality(&surface.id),
            Err(ProjectError::VolumeUnavailable(_))
        ));
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
