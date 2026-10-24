use super::*;
use crate::import::{ImportFormat, read_source};
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn bytes(name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mesh/cgns")
        .join(name);
    let snapshot = read_source(&path)?;
    assert_eq!(snapshot.format, ImportFormat::Cgns);
    Ok(snapshot.bytes)
}

fn field<'a>(grid: &'a RawGrid, name: &str) -> Option<&'a panta_mesh::ResultField> {
    grid.fields.iter().find(|field| field.name == name)
}

fn group_triangles(mesh: &panta_mesh::TetMesh) -> Vec<usize> {
    let mut counts = vec![0; mesh.boundary_group_count as usize];
    for triangle in &mesh.boundary {
        counts[triangle.group as usize] += 1;
    }
    counts
}

/// 两个 2×2×2 块：blockHex 为 8 个 HEXA_8，blockMixed 为 16 个 PENTA_6（MIXED + 偏移）。
#[test]
fn mixed_zones_become_regions_with_bc_groups_and_fields() -> TestResult {
    let mut grid = parse_cgns_grid(&bytes("mixed.cgns")?)?;
    assert_eq!(grid.points.len(), 54);
    let mut expected = vec![CellType::Hexahedron; 8];
    expected.extend([CellType::Wedge; 16]);
    assert_eq!(grid.types, expected);
    assert_eq!(grid.region_names, ["blockHex", "blockMixed"]);
    assert!(grid.skipped_fields.is_empty(), "{:?}", grid.skipped_fields);
    let pressure = field(&grid, "Pressure").ok_or("Pressure missing")?;
    assert_eq!(
        (pressure.association, pressure.components),
        (FieldAssociation::Node, 1)
    );
    // Pressure = x + 10y；blockMixed 的首节点位于 x = 2。
    assert_eq!(pressure.values[27], 2.0);
    let velocity = field(&grid, "Velocity").ok_or("Velocity missing")?;
    assert_eq!(velocity.components, 3);
    assert_eq!(velocity.values.len(), 54 * 3);
    assert!(field(&grid, "VelocityX").is_none());
    let index = field(&grid, "CellIndex").ok_or("CellIndex missing")?;
    assert_eq!(index.association, FieldAssociation::Cell);
    assert_eq!((index.values[7], index.values[8]), (7.0, 0.0));
    let mesh = grid.take_mesh()?;
    let report = panta_mesh::validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert!((report.volume_mm3 - 16.0).abs() < 1.0e-9);
    assert_eq!(mesh.region_count, 2);
    assert_eq!(
        grid.boundary_group_names,
        ["blockHex/bottomWall", "blockMixed/bottomWall", "unassigned"]
    );
    assert_eq!(group_triangles(&mesh), [8, 8, 80]);
    Ok(())
}

/// CGNS 3.3 及更早的 MIXED 没有 ElementStartOffset，按类型节点数推进，结果应完全一致。
#[test]
fn mixed_without_offsets_matches_offset_layout() -> TestResult {
    let current = parse_cgns_grid(&bytes("mixed.cgns")?)?;
    let legacy = parse_cgns_grid(&bytes("mixed_v3.cgns")?)?;
    assert_eq!(current, legacy);
    Ok(())
}

#[test]
fn tetra_pyramid_zone_uses_vertex_bc_and_section_groups() -> TestResult {
    let mut grid = parse_cgns_grid(&bytes("tetpyr.cgns")?)?;
    assert_eq!(
        grid.types,
        [
            CellType::Pyramid,
            CellType::Pyramid,
            CellType::Pyramid,
            CellType::Tetra
        ]
    );
    // 单 Zone 不建立区域表。
    assert!(grid.region_names.is_empty() && grid.cell_regions.is_empty());
    assert!(grid.skipped_fields.is_empty(), "{:?}", grid.skipped_fields);
    let rank = field(&grid, "Rank").ok_or("Rank missing")?;
    assert_eq!(rank.values, [10.0, 20.0, 30.0, 40.0]);
    // 只有 X 分量的数组保持标量。
    assert_eq!(
        field(&grid, "MomentumX").map(|field| field.components),
        Some(1)
    );
    let mesh = grid.take_mesh()?;
    let report = panta_mesh::validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert!((report.volume_mm3 - (1.0 + 1.0 / 6.0)).abs() < 1.0e-9);
    assert_eq!(
        grid.boundary_group_names,
        ["top", "cubeBottom", "unassigned"]
    );
    assert_eq!(group_triangles(&mesh), [2, 2, 12]);
    Ok(())
}

/// NGON_n + NFACE_n：两个立方体多面体，其中两个面以反向存储、由负号引用。
#[test]
fn ngon_nface_cells_become_closed_polyhedra() -> TestResult {
    let mut grid = parse_cgns_grid(&bytes("ngon.cgns")?)?;
    assert_eq!(grid.types, [CellType::Polyhedron; 2]);
    let mesh = grid.take_mesh()?;
    let report = panta_mesh::validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert!((report.volume_mm3 - 2.0).abs() < 1.0e-9);
    assert_eq!(grid.boundary_group_names, ["inlet", "unassigned"]);
    assert_eq!(group_triangles(&mesh), [2, 18]);
    Ok(())
}

#[test]
fn surface_mode_outputs_exterior_triangles() -> TestResult {
    for (name, triangles) in [("mixed.cgns", 96), ("tetpyr.cgns", 16), ("ngon.cgns", 20)] {
        let mesh = parse_cgns_surface(&bytes(name)?)?;
        assert_eq!(mesh.triangles.len(), triangles, "{name}");
    }
    Ok(())
}

#[test]
fn adf_and_non_hdf5_files_are_rejected() -> TestResult {
    let error = parse_cgns_grid(&bytes("adf.cgns")?)
        .err()
        .ok_or("ADF accepted")?;
    assert!(error.0.contains("ADF"), "{error}");
    let error = parse_cgns_surface(b"not a cgns file")
        .err()
        .ok_or("text accepted")?;
    assert!(error.0.contains("HDF5"), "{error}");
    Ok(())
}

#[test]
fn legacy_mixed_and_ngon_layouts_split_by_count() -> TestResult {
    let mixed = Section {
        name: "m".to_owned(),
        code: MIXED,
        first: 1,
        count: 2,
        connectivity: vec![10, 1, 2, 3, 4, 7, 1, 2, 3, 4],
        offsets: None,
    };
    let elements = split_elements(&mixed)?;
    assert_eq!(elements, [(10, &[1, 2, 3, 4][..]), (7, &[1, 2, 3, 4][..])]);
    let ngon = Section {
        name: "n".to_owned(),
        code: NGON_N,
        first: 1,
        count: 2,
        connectivity: vec![3, 1, 2, 3, 4, 1, 2, 3, 4],
        offsets: None,
    };
    let elements = split_elements(&ngon)?;
    assert_eq!(
        elements,
        [(NGON_N, &[1, 2, 3][..]), (NGON_N, &[1, 2, 3, 4][..])]
    );
    let truncated = Section {
        connectivity: vec![10, 1, 2, 3],
        ..mixed
    };
    assert!(split_elements(&truncated).is_err());
    Ok(())
}

#[test]
fn quadratic_cells_are_rejected() -> TestResult {
    let section = Section {
        name: "q".to_owned(),
        code: 11,
        first: 1,
        count: 1,
        connectivity: (1..=10).collect(),
        offsets: None,
    };
    assert_eq!(split_elements(&section)?.len(), 1);
    assert_eq!(linear_cell(11), None);
    assert_eq!(element_dimension(11), 3);
    Ok(())
}

#[test]
fn xyz_arrays_combine_into_vectors() {
    let combined = combine_vectors(vec![
        ("VelocityX".to_owned(), vec![1.0, 2.0]),
        ("VelocityY".to_owned(), vec![3.0, 4.0]),
        ("VelocityZ".to_owned(), vec![5.0, 6.0]),
        ("Density".to_owned(), vec![7.0, 8.0]),
        ("X".to_owned(), vec![0.0, 0.0]),
    ]);
    assert_eq!(
        combined,
        [
            ("Velocity".to_owned(), 3, vec![1.0, 3.0, 5.0, 2.0, 4.0, 6.0]),
            ("Density".to_owned(), 1, vec![7.0, 8.0]),
            ("X".to_owned(), 1, vec![0.0, 0.0]),
        ]
    );
}
