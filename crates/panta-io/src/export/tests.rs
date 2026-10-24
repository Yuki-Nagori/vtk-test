use super::*;
use crate::import::formats::{parse_obj, parse_ply, parse_vtk_legacy, parse_vtp, parse_vtu};
use panta_mesh::Progress;

fn sample() -> SurfaceMesh {
    SurfaceMesh {
        triangles: vec![
            [[0.0, 0.0, 0.0], [1.5, 0.0, 0.0], [0.0, 2.25, -3.0]],
            [[1.5, 0.0, 0.0], [0.1, 0.2, 0.3], [0.0, 2.25, -3.0]],
            [[-0.0, 0.0, 0.0], [0.0, 2.25, -3.0], [1e-300, 7.0, 1.0e10]],
        ],
    }
}

fn written(format: ExportFormat) -> Result<Vec<u8>, ExportError> {
    let mut bytes = Vec::new();
    write_surface_mesh(&sample(), format, &mut bytes)?;
    Ok(bytes)
}

fn assert_exact(mesh: &SurfaceMesh) {
    let expected = sample();
    assert_eq!(mesh.triangles.len(), expected.triangles.len());
    for (actual, expected) in mesh.triangles.iter().zip(&expected.triangles) {
        for (a, e) in actual.iter().flatten().zip(expected.iter().flatten()) {
            assert_eq!(*a, *e + 0.0);
        }
    }
}

#[test]
fn lossless_formats_round_trip_through_the_import_parsers() -> Result<(), Box<dyn std::error::Error>>
{
    assert_exact(&parse_obj(&written(ExportFormat::Obj)?)?);
    assert_exact(&parse_ply(&written(ExportFormat::Ply)?)?);
    assert_exact(&parse_vtk_legacy(&written(ExportFormat::VtkLegacy)?)?);
    assert_exact(&parse_vtp(&written(ExportFormat::Vtp)?)?);
    // VTU 三角形单元按表面单元导入，结果与 VTP 一致。
    assert_exact(&parse_vtu(&written(ExportFormat::Vtu)?)?);
    Ok(())
}

#[test]
fn binary_stl_round_trips_at_f32_precision() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = written(ExportFormat::Stl)?;
    assert_eq!(bytes.len(), 84 + 50 * 3);
    let mesh = panta_mesh::parse_stl(&bytes)?;
    assert_eq!(mesh.triangles.len(), 3);
    assert_eq!(mesh.triangles[0][2], [0.0, 2.25, -3.0]);
    let normal: [f32; 3] = std::array::from_fn(|axis| {
        let start = 84 + axis * 4;
        f32::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
        ])
    });
    let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((length - 1.0).abs() < 1e-6);
    Ok(())
}

#[test]
fn indexed_formats_share_bitwise_identical_points() -> Result<(), ExportError> {
    let indexed = IndexedMesh::new(&sample(), &Progress::new())?;
    // -0.0 与 0.0 合并；共 5 个不同点。
    assert_eq!(indexed.points.len(), 5);
    assert_eq!(indexed.triangles[2][0], indexed.triangles[0][0]);
    Ok(())
}

#[test]
fn base64_matches_reference_vectors() {
    for (input, expected) in [
        (&b""[..], &b""[..]),
        (b"f", b"Zg=="),
        (b"fo", b"Zm8="),
        (b"foo", b"Zm9v"),
        (b"foobar", b"Zm9vYmFy"),
    ] {
        assert_eq!(base64(input), expected);
    }
}

#[test]
fn export_file_replaces_target_and_rejects_unknown_extensions()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::temp_dir().join(format!("panta-export-{}", std::process::id()));
    fs::create_dir_all(&directory)?;
    let target = directory.join("part.obj");
    fs::write(&target, b"old")?;
    assert_eq!(export_surface_mesh(&sample(), &target)?, ExportFormat::Obj);
    assert_exact(&parse_obj(&fs::read(&target)?)?);
    assert!(matches!(
        export_surface_mesh(&sample(), &directory.join("part.step")),
        Err(ExportError::UnsupportedFormat(_))
    ));
    assert!(matches!(
        export_surface_mesh(&sample(), &directory.join("missing/part.stl")),
        Err(ExportError::Io(_))
    ));
    let leftovers = fs::read_dir(&directory)?.count();
    assert_eq!(leftovers, 1);
    fs::remove_dir_all(&directory)?;
    Ok(())
}

#[test]
fn progress_export_completes_and_cancel_leaves_no_target() -> Result<(), Box<dyn std::error::Error>>
{
    let directory =
        std::env::temp_dir().join(format!("panta-export-progress-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let mesh = SurfaceMesh {
        triangles: vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
    };
    let done = Progress::new();
    let target = directory.join("ok.vtp");
    export_surface_mesh_with_progress(&mesh, &target, &done)?;
    assert_eq!(done.fraction(), 1.0);
    assert!(target.is_file());

    let cancelled = Progress::new();
    cancelled.cancel();
    let target = directory.join("cancelled.stl");
    assert!(matches!(
        export_surface_mesh_with_progress(&mesh, &target, &cancelled),
        Err(ExportError::Cancelled)
    ));
    assert!(!target.exists());
    let leftovers = std::fs::read_dir(&directory)?.count();
    assert_eq!(leftovers, 1, "只剩成功导出的文件，没有 .partial");
    std::fs::remove_dir_all(&directory)?;
    Ok(())
}

#[test]
fn tet_volume_round_trips_through_vtu_and_rejects_other_cells()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::import::formats::{parse_vtk_legacy_grid, parse_vtu_grid};
    let nodes = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ];
    let mesh = panta_mesh::tet_mesh_from_cells(nodes, vec![[0, 1, 2, 3], [1, 2, 3, 4]])?;
    let mut bytes = Vec::new();
    crate::export::write_tet_mesh_vtu(&mesh, &mut bytes)?;
    let grid = parse_vtu_grid(&bytes)?;
    assert_eq!(grid.points, mesh.nodes);
    assert_eq!(
        grid.connectivity,
        mesh.tets
            .iter()
            .flat_map(|tet| tet.nodes)
            .collect::<Vec<_>>()
    );
    let legacy = "# vtk DataFile Version 4.2\nt\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 4 double\n0 0 0 1 0 0 0 1 0 0 0 1\nCELLS 1 5\n4 0 1 2 3\nCELL_TYPES 1\n10\n";
    assert_eq!(
        parse_vtk_legacy_grid(legacy.as_bytes())?.connectivity,
        vec![0, 1, 2, 3]
    );
    let quadratic = legacy.replace("CELL_TYPES 1\n10", "CELL_TYPES 1\n12");
    assert!(parse_vtk_legacy_grid(quadratic.as_bytes()).is_err());
    let directory = std::env::temp_dir().join(format!("panta-tet-export-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let done = Progress::new();
    let target = directory.join("ok.vtu");
    export_tet_mesh_vtu(&mesh, &target, &done)?;
    assert_eq!(done.fraction(), 1.0);
    assert_eq!(parse_vtu_grid(&std::fs::read(&target)?)?.cell_count(), 2);
    let cancelled = Progress::new();
    cancelled.cancel();
    let target = directory.join("cancelled.vtu");
    assert!(matches!(
        export_tet_mesh_vtu(&mesh, &target, &cancelled),
        Err(ExportError::Cancelled)
    ));
    assert_eq!(
        std::fs::read_dir(&directory)?.count(),
        1,
        "取消不留目标与 .partial"
    );
    std::fs::remove_dir_all(&directory)?;
    let polydata = "# vtk DataFile Version 4.2\nt\nASCII\nDATASET POLYDATA\nPOINTS 3 double\n0 0 0 1 0 0 0 1 0\nPOLYGONS 1 4\n3 0 1 2\n";
    assert!(parse_vtk_legacy_grid(polydata.as_bytes()).is_err());
    Ok(())
}

/// 多面体导出随 task 031 PR 3 提供；此前拒绝，且不留临时文件。
#[test]
fn polyhedron_meshes_are_rejected_by_vtu_export_until_supported()
-> Result<(), Box<dyn std::error::Error>> {
    let nodes: Vec<[f64; 3]> = (0..8_u32)
        .map(|corner| {
            let (x, y) = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)][corner as usize % 4];
            [x, y, f64::from(corner / 4)]
        })
        .collect();
    let mut connectivity = vec![u32::MAX, 6];
    for face in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        connectivity.push(4);
        connectivity.extend(face);
    }
    let types = vec![panta_mesh::CellType::Polyhedron];
    let offsets =
        panta_mesh::cell_offsets(&types, &connectivity).map_err(|cell| format!("{cell}"))?;
    let mesh = panta_mesh::volume_mesh_from_cells(nodes, types, offsets, connectivity)?;
    let directory = std::env::temp_dir().join(format!("panta-poly-export-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let result = export_tet_mesh_vtu(&mesh, &directory.join("poly.vtu"), &Progress::new());
    assert!(
        matches!(&result, Err(ExportError::UnsupportedFormat(reason)) if reason.contains("polyhedron"))
    );
    assert_eq!(std::fs::read_dir(&directory)?.count(), 0);
    std::fs::remove_dir_all(&directory)?;
    Ok(())
}

mod streamed {
    use super::{assert_exact, sample};
    use crate::export::{
        Base64Probe, ExportError, export_surface_asset_with_progress, export_surface_mesh,
        export_tet_mesh_vtu, export_volume_asset_vtu,
    };
    use crate::import::formats::{parse_obj, parse_ply, parse_vtk_legacy, parse_vtp, parse_vtu};
    use panta_mesh::{
        MeshAsset, Progress, SurfaceAssetChunks, SyntheticTetBlock, TetMesh, VolumeAssetChunks,
        encode_mesh_asset, write_volume_asset_streaming,
    };
    use std::path::{Path, PathBuf};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> std::io::Result<Dir> {
        let path =
            std::env::temp_dir().join(format!("panta-stream-export-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Dir(path))
    }

    fn surface_asset(dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = dir.join("surface.pmesh");
        let (bytes, _) = encode_mesh_asset(&MeshAsset::Surface(sample()))?;
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    #[test]
    fn streamed_stl_is_byte_identical_and_indexed_formats_round_trip_unwelded() -> TestResult {
        let dir = dir("surface")?;
        let asset = surface_asset(&dir.0)?;
        let resident = dir.0.join("resident.stl");
        export_surface_mesh(&sample(), &resident)?;
        let streamed = dir.0.join("streamed.stl");
        let mut chunks = SurfaceAssetChunks::open(&asset)?;
        export_surface_asset_with_progress(&mut chunks, &streamed, &Progress::new())?;
        assert_eq!(std::fs::read(&streamed)?, std::fs::read(&resident)?);

        type Parser =
            fn(&[u8]) -> Result<panta_mesh::SurfaceMesh, crate::import::formats::FormatError>;
        let parsers: [(&str, Parser); 5] = [
            ("obj", parse_obj),
            ("ply", parse_ply),
            ("vtk", parse_vtk_legacy),
            ("vtp", parse_vtp),
            ("vtu", parse_vtu),
        ];
        for (extension, parse) in parsers {
            let path = dir.0.join(format!("streamed.{extension}"));
            let progress = Progress::new();
            export_surface_asset_with_progress(&mut chunks, &path, &progress)?;
            assert_eq!(progress.fraction(), 1.0);
            assert_exact(&parse(&std::fs::read(&path)?)?);
        }

        let cancelled = Progress::new();
        cancelled.cancel();
        let target = dir.0.join("cancelled.vtp");
        assert!(matches!(
            export_surface_asset_with_progress(&mut chunks, &target, &cancelled),
            Err(ExportError::Cancelled)
        ));
        assert!(!target.exists());
        let leftovers = std::fs::read_dir(&dir.0)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".partial"))
            .count();
        assert_eq!(leftovers, 0);
        Ok(())
    }

    #[test]
    fn streamed_base64_segments_concatenate_to_the_whole_encoding() -> std::io::Result<()> {
        let data: Vec<u8> = (0..2_500_000_u32)
            .map(|value| (value * 7 % 251) as u8)
            .collect();
        assert_eq!(Base64Probe::streamed(&data)?, Base64Probe::whole(&data));
        assert_eq!(Base64Probe::streamed(&[1, 2])?, Base64Probe::whole(&[1, 2]));
        Ok(())
    }

    #[test]
    fn streamed_volume_vtu_is_byte_identical_to_resident_export() -> TestResult {
        let dir = dir("volume")?;
        let block = SyntheticTetBlock::with_cells_per_axis(3);
        let asset = dir.0.join("volume.pmesh");
        write_volume_asset_streaming(std::fs::File::create(&asset)?, block.counts(), &block, None)?
            .ok_or("cancelled")?;
        let mesh = TetMesh {
            nodes: (0..block.node_count()).map(|i| block.node(i)).collect(),
            tets: (0..block.tet_count()).map(|i| block.tet(i)).collect(),
            boundary: (0..block.boundary_count())
                .map(|i| block.boundary_face(i))
                .collect(),
            region_count: 1,
            boundary_group_count: 6,
            cells: None,
        };
        let resident = dir.0.join("resident.vtu");
        export_tet_mesh_vtu(&mesh, &resident, &Progress::new())?;
        let streamed = dir.0.join("streamed.vtu");
        let mut chunks = VolumeAssetChunks::open(&asset)?;
        export_volume_asset_vtu(&mut chunks, &streamed, &Progress::new())?;
        assert_eq!(std::fs::read(&streamed)?, std::fs::read(&resident)?);
        Ok(())
    }
}

/// 原生混合单元（task 030）：体素 + 金字塔 + 三棱柱 + 四面体经旧版 VTK 导入、VTU 导出、
/// 流式资产导出往返，单元类型、节点与单元场逐原始单元对应。
mod mixed_cells {
    use crate::export::{export_volume_asset_vtu, write_tet_mesh_vtu};
    use crate::import::formats::{parse_vtk_legacy_grid, parse_vtu_grid};
    use panta_mesh::{CellType, MeshAsset, Progress, VolumeAssetChunks, encode_mesh_asset};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const LEGACY: &str = "# vtk DataFile Version 4.2\nmixed\nASCII\nDATASET UNSTRUCTURED_GRID\n\
POINTS 12 double\n0 0 0 1 0 0 1 1 0 0 1 0 0 0 1 1 0 1 1 1 1 0 1 1 0.5 0.5 1.5 2 0 0.5 2 1 0.5 1.5 0.5 1.5\n\
CELLS 4 27\n8 0 1 3 2 4 5 7 6\n5 4 5 6 7 8\n6 1 9 5 2 10 6\n4 5 6 8 11\n\
CELL_TYPES 4\n11\n14\n13\n10\n\
CELL_DATA 4\nSCALARS id double 1\nLOOKUP_TABLE default\n0 1 2 3\n";

    #[test]
    fn legacy_mixed_cells_round_trip_through_vtu_and_streamed_asset() -> TestResult {
        let mut grid = parse_vtk_legacy_grid(LEGACY.as_bytes())?;
        assert_eq!(
            grid.types,
            vec![
                CellType::Hexahedron,
                CellType::Pyramid,
                CellType::Wedge,
                CellType::Tetra
            ]
        );
        // 体素按 VTK_VOXEL → VTK_HEXAHEDRON 的局部顺序改写。
        assert_eq!(&grid.connectivity[..8], &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(grid.cell_count(), 4);
        let field = grid
            .fields
            .iter()
            .find(|field| field.name == "id")
            .ok_or("cell field kept")?;
        assert_eq!(field.values, vec![0.0, 1.0, 2.0, 3.0]);
        let mesh = grid.take_mesh()?;
        assert_eq!(mesh.cell_count(), 4);
        assert_eq!(mesh.cell_type_counts(), [1, 1, 1, 1, 0]);
        assert!(panta_mesh::validate_tet_mesh(&mesh).is_ok());
        let layout = mesh.cells.as_ref().ok_or("mixed layout")?;

        let mut bytes = Vec::new();
        write_tet_mesh_vtu(&mesh, &mut bytes)?;
        let reread = parse_vtu_grid(&bytes)?;
        assert_eq!(reread.types, layout.types);
        assert_eq!(reread.connectivity, layout.connectivity);
        assert_eq!(reread.points, mesh.nodes);

        let directory =
            std::env::temp_dir().join(format!("panta-mixed-export-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let asset = directory.join("mixed.pmesh");
        let (encoded, _) = encode_mesh_asset(&MeshAsset::Volume(mesh.clone()))?;
        std::fs::write(&asset, encoded)?;
        let mut chunks = VolumeAssetChunks::open(&asset)?;
        assert!(chunks.is_mixed());
        let streamed = directory.join("streamed.vtu");
        export_volume_asset_vtu(&mut chunks, &streamed, &Progress::new())?;
        let streamed_bytes = std::fs::read(&streamed)?;
        std::fs::remove_dir_all(&directory)?;
        assert_eq!(streamed_bytes, bytes);
        Ok(())
    }

    #[test]
    fn polyhedra_and_unknown_cells_are_rejected_with_clear_errors() {
        let polyhedron = LEGACY.replace(
            "CELL_TYPES 4\n11\n14\n13\n10",
            "CELL_TYPES 4\n11\n14\n13\n42",
        );
        let error = parse_vtk_legacy_grid(polyhedron.as_bytes())
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("42"), "{error}");
        let quadratic = LEGACY.replace(
            "CELL_TYPES 4\n11\n14\n13\n10",
            "CELL_TYPES 4\n11\n14\n13\n24",
        );
        assert!(parse_vtk_legacy_grid(quadratic.as_bytes()).is_err());
        // 节点数与类型不符（四面体写成金字塔）。
        let mismatched = LEGACY.replace(
            "CELL_TYPES 4\n11\n14\n13\n10",
            "CELL_TYPES 4\n11\n14\n13\n14",
        );
        assert!(parse_vtk_legacy_grid(mismatched.as_bytes()).is_err());
    }
}
