//! 网格 DTO 布局转换及显示缓冲扁平化；领域校验转交 panta-mesh。
use crate::bridge;

/// Netgen / native IR DTO → 领域 TetMesh；布局或分配失败返回说明（task 021 子项 5）。
pub(super) fn tet_mesh_from_dto(data: bridge::TetMeshData) -> Result<panta_mesh::TetMesh, String> {
    if !data.nodes.len().is_multiple_of(3)
        || !data.tets.len().is_multiple_of(4)
        || !data.boundary.len().is_multiple_of(3)
        || data.tet_regions.len() != data.tets.len() / 4
        || data.boundary_groups.len() != data.boundary.len() / 3
    {
        return Err("invalid mesh DTO layout".to_owned());
    }
    let mut nodes = Vec::new();
    let mut tets = Vec::new();
    let mut boundary = Vec::new();
    nodes
        .try_reserve_exact(data.nodes.len() / 3)
        .map_err(|error| format!("cannot allocate mesh nodes: {error}"))?;
    tets.try_reserve_exact(data.tets.len() / 4)
        .map_err(|error| format!("cannot allocate tetrahedra: {error}"))?;
    boundary
        .try_reserve_exact(data.boundary.len() / 3)
        .map_err(|error| format!("cannot allocate boundary triangles: {error}"))?;
    nodes.extend(
        data.nodes
            .as_chunks::<3>()
            .0
            .iter()
            .map(|point| [point[0], point[1], point[2]]),
    );
    tets.extend(
        data.tets
            .as_chunks::<4>()
            .0
            .iter()
            .zip(data.tet_regions)
            .map(|(nodes, region)| panta_mesh::Tetrahedron {
                nodes: [nodes[0], nodes[1], nodes[2], nodes[3]],
                region,
            }),
    );
    boundary.extend(
        data.boundary
            .as_chunks::<3>()
            .0
            .iter()
            .zip(data.boundary_groups)
            .map(|(nodes, group)| panta_mesh::SurfaceTriangle {
                nodes: [nodes[0], nodes[1], nodes[2]],
                group,
            }),
    );
    Ok(panta_mesh::TetMesh {
        nodes,
        tets,
        boundary,
        region_count: data.region_count,
        boundary_group_count: data.boundary_group_count,
    })
}

pub(super) fn mesh_validate_tet(data: bridge::TetMeshData) -> bridge::TetMeshValidation {
    match tet_mesh_from_dto(data) {
        Ok(mesh) => {
            let report = panta_mesh::validate_tet_mesh(&mesh);
            bridge::TetMeshValidation {
                issues: report.issues,
                volume_mm3: report.volume_mm3,
            }
        }
        Err(issue) => bridge::TetMeshValidation {
            issues: vec![issue],
            volume_mm3: 0.0,
        },
    }
}

/// Netgen 体网格走与导入体网格相同的质量统计（校验失败写入诊断）。
pub(super) fn mesh_tet_quality(data: bridge::TetMeshData) -> bridge::VolumeInfoResult {
    let mut result = bridge::VolumeInfoResult::default();
    let mesh = match checked_tet_mesh(data) {
        Ok(mesh) => mesh,
        Err(error) => {
            result.error = error;
            return result;
        }
    };
    let quality = panta_mesh::tet_quality(&mesh, &|_| {});
    result.available = true;
    result.node_count = mesh.nodes.len() as u64;
    result.tet_count = quality.tet_count as u64;
    result.min_dihedral_degrees = quality.min_dihedral_degrees;
    result.max_aspect_ratio = quality.max_aspect_ratio;
    result.mean_aspect_ratio = quality.mean_aspect_ratio;
    result.poor_count = quality.poor_count as u64;
    result
}

/// Netgen 体网格走与导入体网格相同的 VTU 导出（原始节点与四面体单元）。
pub(super) fn mesh_tet_export_vtu(
    data: bridge::TetMeshData,
    path: &str,
) -> bridge::ProjectDiagnostic {
    let mesh = match checked_tet_mesh(data) {
        Ok(mesh) => mesh,
        Err(error) => return error,
    };
    match panta_core::project::export_tet_mesh_vtu(&mesh, std::path::Path::new(path)) {
        Ok(()) => bridge::ProjectDiagnostic::default(),
        Err(error) => error.into(),
    }
}

fn checked_tet_mesh(
    data: bridge::TetMeshData,
) -> Result<panta_mesh::TetMesh, bridge::ProjectDiagnostic> {
    let invalid = |detail: String| {
        crate::project_response::diagnostic("project.volume_unavailable", "input", detail)
    };
    let mesh = tet_mesh_from_dto(data).map_err(invalid)?;
    let report = panta_mesh::validate_tet_mesh(&mesh);
    if !report.issues.is_empty() {
        return Err(invalid(report.issues.join("; ")));
    }
    Ok(mesh)
}

/// SurfaceMesh → 扁平坐标缓冲；分配失败的上下文由调用方配上稳定诊断码。
pub(super) fn mesh_coordinates(mesh: &panta_mesh::SurfaceMesh) -> Result<Vec<f64>, String> {
    let mut coordinates = Vec::new();
    let values = mesh
        .triangles
        .len()
        .checked_mul(9)
        .ok_or("mesh snapshot coordinate count overflow")?;
    coordinates
        .try_reserve_exact(values)
        .map_err(|error| format!("mesh snapshot allocation failed: {error}"))?;
    for triangle in &mesh.triangles {
        for point in triangle {
            coordinates.extend_from_slice(point);
        }
    }
    Ok(coordinates)
}

#[cfg(test)]
mod tests {
    use crate::bridge;
    #[test]
    fn tet_mesh_bridge_checks_layout_and_domain_data() {
        let mesh_data = || bridge::TetMeshData {
            nodes: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            tets: vec![0, 1, 2, 3],
            tet_regions: vec![0],
            boundary: vec![0, 1, 2],
            boundary_groups: vec![0],
            region_count: 1,
            boundary_group_count: 1,
        };

        let quality = crate::mesh::mesh_tet_quality(mesh_data());
        assert!(quality.available);
        assert_eq!(quality.tet_count, 1);
        assert_eq!(quality.node_count, 4);
        let mut broken = mesh_data();
        broken.tets = vec![0, 1, 2, 9];
        assert_eq!(
            crate::mesh::mesh_tet_quality(broken).error.code,
            "project.volume_unavailable"
        );
        let out = std::env::temp_dir().join(format!("panta-netgen-{}.vtu", std::process::id()));
        let exported = crate::mesh::mesh_tet_export_vtu(mesh_data(), &out.display().to_string());
        assert!(exported.code.is_empty());
        let written = std::fs::read_to_string(&out);
        let _ = std::fs::remove_file(&out);
        assert!(written.is_ok_and(|text| text.contains("NumberOfCells=\"1\"")));

        let valid = crate::mesh_validate_tet(mesh_data());
        assert!(valid.issues.is_empty());
        assert_eq!(valid.volume_mm3, 1.0 / 6.0);

        let mut invalid_layout = mesh_data();
        invalid_layout.nodes.pop();
        let layout = crate::mesh_validate_tet(invalid_layout);
        assert_eq!(layout.issues, ["invalid mesh DTO layout"]);
        assert_eq!(layout.volume_mm3, 0.0);

        let mut invalid_mesh = mesh_data();
        invalid_mesh.tets[3] = 9;
        let domain = crate::mesh_validate_tet(invalid_mesh);
        assert!(!domain.issues.is_empty());
        assert_eq!(domain.volume_mm3, 0.0);
    }
}
