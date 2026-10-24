use super::*;
use crate::import::{ImportError, ImportFormat, read_source};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(case: &str, file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mesh/openfoam")
        .join(case)
        .join(file)
}

fn snapshot(path: &Path) -> Result<Vec<u8>, ImportError> {
    let snapshot = read_source(path)?;
    assert_eq!(snapshot.format, ImportFormat::OpenFoam);
    Ok(snapshot.bytes)
}

fn group_triangles(mesh: &panta_mesh::TetMesh, group: u32) -> usize {
    mesh.boundary
        .iter()
        .filter(|triangle| triangle.group == group)
        .count()
}

fn field<'a>(grid: &'a RawGrid, name: &str) -> Option<&'a panta_mesh::ResultField> {
    grid.fields.iter().find(|field| field.name == name)
}

/// 记录 checkMesh：nPoints 36、nCells 12（全六面体）、nFaces 52、nInternalFaces 20。
#[test]
fn ascii_cavity_rebuilds_hexahedra_patches_and_latest_fields() -> TestResult {
    let bytes = snapshot(&fixture("ascii", "ascii.foam"))?;
    let mut grid = parse_openfoam_grid(&bytes)?;
    assert_eq!(grid.points.len(), 36);
    assert_eq!(grid.types, vec![CellType::Hexahedron; 12]);
    let pressure = field(&grid, "p").ok_or("p missing")?;
    assert_eq!(
        (pressure.components, pressure.units.as_str()),
        (1, "m^2 s^-2")
    );
    assert_eq!(pressure.values.len(), 12);
    let velocity = field(&grid, "U").ok_or("U missing")?;
    assert_eq!(
        (velocity.components, velocity.units.as_str()),
        (3, "m s^-1")
    );
    // 最新时间 0.01 的速度非零（0 时刻 internalField 为 uniform (0 0 0)）。
    assert!(velocity.values.iter().any(|value| *value != 0.0));
    // phi 是 surfaceScalarField，不属于体场，不收入快照。
    assert!(field(&grid, "phi").is_none());
    assert!(grid.skipped_fields.is_empty(), "{:?}", grid.skipped_fields);
    let mesh = grid.take_mesh()?;
    let report = panta_mesh::validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert!((report.volume_mm3 - 1.0e-4).abs() < 1.0e-12);
    assert_eq!(
        grid.boundary_group_names,
        ["movingWall", "fixedWalls", "frontAndBack"]
    );
    assert_eq!(mesh.region_count, 1);
    assert_eq!(
        [
            group_triangles(&mesh, 0),
            group_triangles(&mesh, 1),
            group_triangles(&mesh, 2)
        ],
        [12, 28, 24]
    );
    Ok(())
}

#[test]
fn control_dict_selects_the_same_case_snapshot() -> TestResult {
    let marker = snapshot(&fixture("ascii", "ascii.foam"))?;
    let control = snapshot(&fixture("ascii", "system/controlDict"))?;
    assert_eq!(marker, control);
    Ok(())
}

/// foamFormatConvert 由 ASCII 算例转换而来，因此与 ASCII 解析结果逐值一致（ASCII 8 位有效数字）。
#[test]
fn binary_case_matches_ascii_case() -> TestResult {
    let ascii = parse_openfoam_grid(&snapshot(&fixture("ascii", "ascii.foam"))?)?;
    let binary = parse_openfoam_grid(&snapshot(&fixture("binary", "binary.foam"))?)?;
    assert_eq!(ascii.points, binary.points);
    assert_eq!(ascii.types, binary.types);
    assert_eq!(ascii.connectivity, binary.connectivity);
    assert_eq!(ascii.boundary, binary.boundary);
    for name in ["p", "U"] {
        let (left, right) = (
            field(&ascii, name).ok_or(name)?,
            field(&binary, name).ok_or(name)?,
        );
        assert_eq!(left.values.len(), right.values.len());
        for (a, b) in left.values.iter().zip(&right.values) {
            assert!(
                (a - b).abs() <= 1.0e-7 * a.abs().max(1.0),
                "{name}: {a} vs {b}"
            );
        }
    }
    Ok(())
}

/// polyDualMesh 生成的对偶网格（gzip）：checkMesh 记录 160 点、48 单元（12 六面体、36 多面体）。
#[test]
fn gzipped_dual_mesh_keeps_polyhedra() -> TestResult {
    let mut grid = parse_openfoam_grid(&snapshot(&fixture("poly", "poly.foam"))?)?;
    assert_eq!(grid.points.len(), 160);
    let count = |kind| grid.types.iter().filter(|&&cell| cell == kind).count();
    assert_eq!(
        (count(CellType::Hexahedron), count(CellType::Polyhedron)),
        (12, 36)
    );
    assert_eq!(field(&grid, "p").ok_or("p")?.values.len(), 48);
    let mesh = grid.take_mesh()?;
    let report = panta_mesh::validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert!(
        (report.volume_mm3 - 1.0e-4).abs() < 1.0e-12,
        "{}",
        report.volume_mm3
    );
    assert_eq!(mesh.cell_count(), 48);
    assert!(
        !grid
            .boundary_group_names
            .iter()
            .any(|name| name == "unassigned")
    );
    Ok(())
}

/// splitMeshRegions 的两区域算例：区域按名称排序（fluid、solid），默认区域忽略；
/// T 由 setFields 写入，solid 为 400 300 400 300，fluid 为 300 350 300 350。
#[test]
fn multi_region_case_maps_regions_groups_and_fields() -> TestResult {
    let mut grid = parse_openfoam_grid(&snapshot(&fixture("multi", "multi.foam"))?)?;
    assert_eq!(grid.region_names, ["fluid", "solid"]);
    assert_eq!(grid.cell_regions, [0, 0, 0, 0, 1, 1, 1, 1]);
    let temperature = field(&grid, "T").ok_or("T")?;
    assert_eq!(temperature.units, "K");
    assert_eq!(
        temperature.values,
        [300.0, 350.0, 300.0, 350.0, 400.0, 300.0, 400.0, 300.0]
    );
    let velocity = field(&grid, "U").ok_or("U")?;
    assert_eq!(velocity.values.len(), 24);
    assert!(field(&grid, "cellToRegion").is_some());
    let mesh = grid.take_mesh()?;
    let report = panta_mesh::validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert_eq!(mesh.region_count, 2);
    let solid_tets = mesh.tets.iter().filter(|tet| tet.region == 1).count();
    assert_eq!(solid_tets * 2, mesh.tets.len());
    assert!(
        grid.boundary_group_names
            .iter()
            .any(|name| name == "solid/solid_to_fluid")
    );
    assert!(
        grid.boundary_group_names
            .iter()
            .any(|name| name == "fluid/fluid_to_solid")
    );
    assert!(
        grid.boundary_group_names
            .iter()
            .all(|name| name.starts_with("fluid/") || name.starts_with("solid/"))
    );
    Ok(())
}

#[test]
fn surface_mode_triangulates_patch_faces() -> TestResult {
    let mesh = parse_openfoam_surface(&snapshot(&fixture("ascii", "ascii.foam"))?)?;
    // 32 个边界四边形。
    assert_eq!(mesh.triangles.len(), 64);
    Ok(())
}

/// 在临时目录复制 ASCII 算例，便于构造缺失 / 截断 / 不支持场等情形。
struct ScratchCase(PathBuf);

impl ScratchCase {
    fn new(tag: &str) -> Result<Self, std::io::Error> {
        let root = std::env::temp_dir().join(format!("panta-foam-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let source = fixture("ascii", "");
        for file in [
            "ascii.foam",
            "constant/polyMesh/boundary",
            "constant/polyMesh/faces",
            "constant/polyMesh/neighbour",
            "constant/polyMesh/owner",
            "constant/polyMesh/points",
            "0.01/p",
        ] {
            let target = root.join(file);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(source.join(file), target)?;
        }
        Ok(Self(root))
    }

    fn marker(&self) -> PathBuf {
        self.0.join("ascii.foam")
    }
}

impl Drop for ScratchCase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn tensor_fields_are_skipped_with_reason() -> TestResult {
    let case = ScratchCase::new("tensor")?;
    std::fs::write(
        case.0.join("0.01/R"),
        "FoamFile { format ascii; class volSymmTensorField; object R; }\n\
         dimensions [0 2 -2 0 0 0 0];\ninternalField uniform (0 0 0 0 0 0);\nboundaryField {}\n",
    )?;
    let grid = parse_openfoam_grid(&snapshot(&case.marker())?)?;
    assert_eq!(
        grid.skipped_fields,
        ["R (cell): OpenFOAM volSymmTensorField is not supported"]
    );
    Ok(())
}

#[test]
fn malformed_cases_are_rejected_whole() -> TestResult {
    let case = ScratchCase::new("bad")?;
    let owner = std::fs::read_to_string(case.0.join("constant/polyMesh/owner"))?;
    std::fs::write(
        case.0.join("constant/polyMesh/owner"),
        owner.replace("52\n(", "53\n("),
    )?;
    assert!(parse_openfoam_grid(&snapshot(&case.marker())?).is_err());
    std::fs::remove_file(case.0.join("constant/polyMesh/owner"))?;
    assert!(matches!(
        read_source(&case.marker()),
        Err(ImportError::Format(_))
    ));
    std::fs::remove_dir_all(case.0.join("constant"))?;
    std::fs::create_dir_all(case.0.join("processor0"))?;
    let Err(ImportError::Format(error)) = read_source(&case.marker()) else {
        return Err("decomposed case must be rejected".into());
    };
    assert!(error.0.contains("reconstructPar"), "{error}");
    Ok(())
}

#[test]
fn snapshot_decoding_rejects_trailing_or_truncated_bytes() {
    let mut bytes = bundle::MAGIC.to_vec();
    bytes.extend(0u32.to_le_bytes());
    assert!(bundle::decode(&bytes).is_ok());
    bytes.push(0);
    assert!(bundle::decode(&bytes).is_err());
    assert!(bundle::decode(&bytes[..bundle::MAGIC.len() + 2]).is_err());
}

/// 面邻接质量与 OpenFOAM v1912 checkMesh 报告对照（相对误差 1e-6）：
/// `skew` 为扰动后的 4×3×2 blockMesh，`skewdual` 为其 polyDualMesh，`poly` 为未扰动对偶网格。
/// checkMesh 不报告正交质量，这里只检查其落在 (0, 1]（对偶网格边界单元约 0.15，与逐单元
/// 正交质量同一量级）。
#[test]
fn face_quality_matches_check_mesh() -> TestResult {
    let close =
        |actual: f64, expected: f64| (actual - expected).abs() <= 1e-6 * expected.abs().max(1.0);
    // (算例, 内部面, 最大非正交角, 平均非正交角, 最大偏斜度)
    let cases = [
        ("skew", 46, 32.663972, 21.620442, 1.4240861),
        ("skewdual", 133, 41.626226, 21.297343, 2.4449241),
        ("poly", 104, 0.0, 0.0, 1.0000001),
    ];
    for (case, internal, max_angle, mean_angle, max_skew) in cases {
        let grid = parse_openfoam_grid(&snapshot(&fixture(case, &format!("{case}.foam")))?)?;
        let quality = grid.face_quality.ok_or("face quality")?;
        let summary = format!(
            "{case}: internal {} max {} mean {} skew {}",
            quality.internal_faces,
            quality.max_non_orthogonality_degrees(),
            quality.mean_non_orthogonality_degrees(),
            quality.max_skewness
        );
        assert_eq!(quality.internal_faces, internal, "{summary}");
        assert!(
            close(quality.max_non_orthogonality_degrees(), max_angle)
                || max_angle == 0.0 && quality.max_non_orthogonality_degrees() < 1e-4,
            "{summary}"
        );
        assert!(
            close(quality.mean_non_orthogonality_degrees(), mean_angle)
                || mean_angle == 0.0 && quality.mean_non_orthogonality_degrees() < 1e-4,
            "{summary}"
        );
        assert!(close(quality.max_skewness, max_skew), "{summary}");
        assert_eq!(
            (quality.severe_non_orthogonal_faces, quality.high_skew_faces),
            (0, 0),
            "{summary}"
        );
        assert!(
            quality.min_orthogonal_quality > 0.0 && quality.min_orthogonal_quality <= 1.0,
            "{summary} orthogonal {}",
            quality.min_orthogonal_quality
        );
    }
    Ok(())
}
