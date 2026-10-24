//! 原生混合单元（task 030）的 IR 与拆分回归测试。

use crate::{
    CellType, MeshIndex, TetMesh, VolumeError, cell_offsets, cell_volumes, mirror_cell,
    validate_tet_mesh, volume_mesh_from_cells,
};

fn tet_volume(nodes: &[[f64; 3]], tet: [MeshIndex; 4]) -> f64 {
    let [a, b, c, d] = tet.map(|node| nodes[node as usize]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
    (u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
        + u[2] * (v[0] * w[1] - v[1] * w[0]))
        / 6.0
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 单位立方体六面体 + 顶面金字塔 + +x 侧三棱柱 + 金字塔侧面上的四面体，全部共形。
pub(crate) fn mixed_cells() -> (Vec<[f64; 3]>, Vec<CellType>, Vec<u32>) {
    let nodes = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.5, 0.5, 1.5],
        [2.0, 0.0, 0.5],
        [2.0, 1.0, 0.5],
        [1.5, 0.5, 1.5],
    ];
    let types = vec![
        CellType::Hexahedron,
        CellType::Pyramid,
        CellType::Wedge,
        CellType::Tetra,
    ];
    let connectivity = vec![
        0, 1, 2, 3, 4, 5, 6, 7, // hex
        4, 5, 6, 7, 8, // pyramid
        1, 9, 5, 2, 10, 6, // wedge（底 1-9-5，顶 2-10-6）
        5, 6, 8, 11, // tet
    ];
    (nodes, types, connectivity)
}

fn volumes_tail(mesh: &TetMesh) -> f64 {
    cell_volumes(mesh)[3]
}

pub(crate) fn mixed_mesh() -> TetMesh {
    let (nodes, types, connectivity) = mixed_cells();
    let offsets = cell_offsets(&types, connectivity.len()).unwrap_or_default();
    volume_mesh_from_cells(nodes, types, offsets, connectivity).unwrap_or_else(|error| {
        panic!("mixed fixture must build: {error:?}");
    })
}

#[test]
fn mixed_cells_keep_original_ids_types_and_volumes() -> TestResult {
    let mesh = mixed_mesh();
    let report = validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert!((report.volume_mm3 - (1.0 + 1.0 / 6.0 + 0.5 + volumes_tail(&mesh))).abs() < 1e-12);
    assert_eq!(mesh.cell_count(), 4);
    assert_eq!(mesh.cell_type_counts(), [1, 1, 1, 1]);
    // 六面体 5–6、金字塔 2、棱柱 3、四面体 1。
    let per_cell: Vec<usize> = (0..4)
        .map(|cell| {
            (0..mesh.tets.len())
                .filter(|&t| mesh.tet_cell(t) == cell)
                .count()
        })
        .collect();
    assert!(matches!(per_cell[0], 5 | 6));
    assert_eq!(&per_cell[1..], &[2, 3, 1]);
    let volumes = cell_volumes(&mesh);
    assert!((volumes[0] - 1.0).abs() < 1e-12);
    assert!((volumes[1] - 1.0 / 6.0).abs() < 1e-12);
    assert!((volumes[2] - 0.5).abs() < 1e-12);
    let total: f64 = mesh
        .tets
        .iter()
        .map(|tet| tet_volume(&mesh.nodes, tet.nodes))
        .sum();
    assert!((volumes.iter().sum::<f64>() - total).abs() < 1e-12);
    assert!(
        mesh.tets
            .iter()
            .all(|tet| tet_volume(&mesh.nodes, tet.nodes) > 0.0)
    );
    // 共形拆分后边界：六面体 4 个外露四边形 + 金字塔 3 个三角形 + 棱柱 2 三角 + 2 四边形 + 四面体 3 面。
    assert_eq!(mesh.boundary.len(), 4 * 2 + 3 + 2 + 2 * 2 + 3);
    Ok(())
}

#[test]
fn inverted_cells_are_mirrored_but_degenerate_or_polyhedron_cells_are_rejected() -> TestResult {
    let (nodes, types, mut connectivity) = mixed_cells();
    let offsets = cell_offsets(&types, connectivity.len()).map_err(|c| format!("cell {c}"))?;
    for (cell, kind) in types.iter().enumerate() {
        let range = offsets[cell] as usize..offsets[cell + 1] as usize;
        mirror_cell(*kind, &mut connectivity[range]);
    }
    let mesh = volume_mesh_from_cells(
        nodes.clone(),
        types.clone(),
        offsets.clone(),
        connectivity.clone(),
    )?;
    assert_eq!(cell_volumes(&mesh), cell_volumes(&mixed_mesh()));

    let mut repeated = connectivity.clone();
    repeated[1] = repeated[0];
    assert!(matches!(
        volume_mesh_from_cells(nodes.clone(), types.clone(), offsets.clone(), repeated),
        Err(VolumeError(message)) if message.contains("cell 0")
    ));
    assert!(CellType::from_vtk(crate::VTK_POLYHEDRON).is_none());
    assert!(volume_mesh_from_cells(nodes, types, vec![0, 8, 13], connectivity).is_err());
    Ok(())
}

#[test]
fn tampered_layouts_fail_validation() {
    let mesh = mixed_mesh();
    let mut wrong_region = mesh.clone();
    wrong_region.tets[0].region = 1;
    wrong_region.region_count = 2;
    assert!(!validate_tet_mesh(&wrong_region).is_ok());
    let mut wrong_tets = mesh;
    wrong_tets.tets.swap(0, 1);
    assert!(!validate_tet_mesh(&wrong_tets).is_ok());
}

#[test]
fn quality_is_reported_per_original_cell_type_resident_and_streamed() -> TestResult {
    let mesh = mixed_mesh();
    let resident = crate::tet_quality(&mesh, &crate::Progress::new()).ok_or("cancelled")?;
    // 二面角/纵横比只统计原生四面体，拆分出的四面体不计入。
    assert_eq!(resident.tet_count, 1);
    let counts: Vec<usize> = resident.by_type.iter().map(|kind| kind.count).collect();
    assert_eq!(counts, vec![1, 1, 1, 1]);
    let hex = resident.by_type[CellType::Hexahedron.ordinal()];
    assert!((hex.min_scaled_jacobian - 1.0).abs() < 1e-12);
    for summary in &resident.by_type {
        assert!(summary.min_scaled_jacobian > 0.0 && summary.min_scaled_jacobian <= 1.0);
    }
    let directory =
        std::env::temp_dir().join(format!("panta-mixed-quality-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("mixed.pmesh");
    let (bytes, _) = crate::encode_mesh_asset(&crate::MeshAsset::Volume(mesh))?;
    std::fs::write(&path, bytes)?;
    let mut chunks = crate::VolumeAssetChunks::open(&path)?;
    let streamed = crate::tet_quality_streaming(&mut chunks, &crate::Progress::new())?;
    std::fs::remove_dir_all(&directory)?;
    assert_eq!(streamed, Some(resident));
    Ok(())
}

#[test]
fn cell_field_sections_and_diagnostics_use_original_cell_ids() -> TestResult {
    let mesh = mixed_mesh();
    let field = crate::ResultField {
        name: "id".to_owned(),
        association: crate::FieldAssociation::Cell,
        components: 1,
        units: String::new(),
        values: vec![10.0, 20.0, 30.0, 40.0],
    };
    // z = 0.5 只切过六面体（单元 0）与三棱柱（单元 2）。
    let section = crate::section_surface_field(
        &mesh,
        &field,
        crate::FieldComponent::Component(0),
        [0.0, 0.0, 0.5],
        [0.0, 0.0, 1.0],
        &crate::Progress::new(),
    )?
    .ok_or("cancelled")?;
    let mut seen: Vec<u64> = section.values.iter().map(|value| value[0] as u64).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, vec![10, 30]);
    let mut short = field.clone();
    short.values.push(50.0);
    assert!(
        crate::section_surface_field(
            &mesh,
            &short,
            crate::FieldComponent::Component(0),
            [0.0, 0.0, 0.5],
            [0.0, 0.0, 1.0],
            &crate::Progress::new(),
        )
        .is_err()
    );
    // 锥顶压进六面体：问题按原始单元（金字塔，单元 1）报告。
    let mut inverted = mesh;
    inverted.nodes[8][2] = 0.5;
    let report = validate_tet_mesh(&inverted);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.starts_with("cell 1 (pyramid) sub-tetrahedron")),
        "{:?}",
        report.issues
    );
    Ok(())
}
