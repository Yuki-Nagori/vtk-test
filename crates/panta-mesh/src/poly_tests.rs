//! 多面体单元（task 031）的拆分、质量与资产回归测试。

use crate::{
    CellBlock, CellType, MeshAsset, MeshAssetReader, MeshIndex, TetMesh, VolumeAssetChunks,
    VolumeSource, VolumeStreamCounts, cell_offsets, cell_volumes, decode_mesh_asset,
    encode_mesh_asset, validate_tet_mesh, volume_mesh_from_cells,
};
use std::ops::Range;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 单位立方体的 8 个角点（与六面体节点顺序相同：底面 0-3 逆时针，顶面 4-7）。
fn cube_nodes(origin: [f64; 3]) -> Vec<[f64; 3]> {
    (0..8)
        .map(|corner: usize| {
            let (x, y) = match corner % 4 {
                0 => (0.0, 0.0),
                1 => (1.0, 0.0),
                2 => (1.0, 1.0),
                _ => (0.0, 1.0),
            };
            let z = if corner >= 4 { 1.0 } else { 0.0 };
            [origin[0] + x, origin[1] + y, origin[2] + z]
        })
        .collect()
}

/// 六面体节点顺序 `c` 的 6 个外法向面。
fn cube_faces(c: [MeshIndex; 8]) -> Vec<Vec<MeshIndex>> {
    [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ]
    .iter()
    .map(|face| face.iter().map(|&slot| c[slot]).collect())
    .collect()
}

/// 规范面流，`apex` 槽位留作占位（构建时写回中心节点号）。
fn stream(faces: &[Vec<MeshIndex>]) -> Vec<MeshIndex> {
    let mut out = vec![MeshIndex::MAX, faces.len() as MeshIndex];
    for face in faces {
        out.push(face.len() as MeshIndex);
        out.extend_from_slice(face);
    }
    out
}

/// 正六棱柱（外接圆半径 1、高 1）：节点 0-5 底面、6-11 顶面，从 `first` 起编号。
fn hex_prism(first: MeshIndex, shift: f64) -> (Vec<[f64; 3]>, Vec<Vec<MeshIndex>>) {
    let mut nodes = Vec::new();
    for z in [0.0, 1.0] {
        for k in 0..6 {
            let angle = std::f64::consts::PI / 3.0 * k as f64;
            nodes.push([shift + angle.cos(), angle.sin(), z]);
        }
    }
    let mut faces = vec![
        (0..6).rev().map(|k| first + k).collect(),
        (6..12).map(|k| first + k).collect(),
    ];
    for k in 0..6 {
        let next = (k + 1) % 6;
        faces.push(vec![
            first + k,
            first + next,
            first + next + 6,
            first + k + 6,
        ]);
    }
    (nodes, faces)
}

fn build(
    nodes: Vec<[f64; 3]>,
    types: Vec<CellType>,
    connectivity: Vec<MeshIndex>,
) -> Result<TetMesh, crate::VolumeError> {
    let offsets = cell_offsets(&types, &connectivity)
        .map_err(|cell| crate::VolumeError(format!("offsets {cell}")))?;
    volume_mesh_from_cells(nodes, types, offsets, connectivity)
}

/// 六面体 [0,1]³ + 共形相邻多面体立方体 [1,2]×[0,1]² + 远处六棱柱多面体。
fn poly_mesh() -> TetMesh {
    let mut nodes = cube_nodes([0.0; 3]);
    nodes.extend([
        [2.0, 0.0, 0.0],
        [2.0, 1.0, 0.0],
        [2.0, 0.0, 1.0],
        [2.0, 1.0, 1.0],
    ]);
    let (prism_nodes, prism_faces) = hex_prism(12, 5.0);
    nodes.extend(prism_nodes);
    let mut connectivity: Vec<MeshIndex> = (0..8).collect();
    connectivity.extend(stream(&cube_faces([1, 8, 9, 2, 5, 10, 11, 6])));
    connectivity.extend(stream(&prism_faces));
    let types = vec![
        CellType::Hexahedron,
        CellType::Polyhedron,
        CellType::Polyhedron,
    ];
    build(nodes, types, connectivity).unwrap_or_else(|error| panic!("fixture: {error:?}"))
}

#[test]
fn cube_polyhedron_decomposes_around_its_centroid() -> TestResult {
    let faces = cube_faces([0, 1, 2, 3, 4, 5, 6, 7]);
    for inward in [false, true] {
        let faces: Vec<Vec<MeshIndex>> = faces
            .iter()
            .map(|face| {
                let mut face = face.clone();
                if inward {
                    face.reverse();
                }
                face
            })
            .collect();
        let mesh = build(
            cube_nodes([0.0; 3]),
            vec![CellType::Polyhedron],
            stream(&faces),
        )?;
        let report = validate_tet_mesh(&mesh);
        assert!(report.is_ok(), "{:?}", report.issues);
        assert_eq!(mesh.nodes.len(), 9);
        assert_eq!(mesh.source_node_count(), 8);
        assert_eq!(mesh.nodes[8], [0.5, 0.5, 0.5]);
        assert_eq!(mesh.tets.len(), 12);
        assert_eq!(mesh.boundary.len(), 12);
        assert_eq!(mesh.cell_type_counts(), [0, 0, 0, 0, 1]);
        assert!((cell_volumes(&mesh)[0] - 1.0).abs() < 1e-12);
        // 中心节点不出现在边界上。
        assert!(
            mesh.boundary
                .iter()
                .all(|face| face.nodes.iter().all(|&n| n < 8))
        );
        let quality = crate::tet_quality(&mesh, &crate::Progress::new()).ok_or("cancelled")?;
        let poly = quality.by_type[CellType::Polyhedron.ordinal()];
        assert_eq!(poly.count, 1);
        assert!((poly.min_scaled_jacobian - 1.0).abs() < 1e-12);
        assert_eq!(quality.poor_count, 0);
    }
    Ok(())
}

#[test]
fn polyhedra_are_conformal_with_neighbouring_hexahedra() -> TestResult {
    let mesh = poly_mesh();
    let report = validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    let volumes = cell_volumes(&mesh);
    let prism = 3.0 * 3.0_f64.sqrt() / 2.0;
    assert!((volumes[0] - 1.0).abs() < 1e-12);
    assert!((volumes[1] - 1.0).abs() < 1e-12);
    assert!((volumes[2] - prism).abs() < 1e-12);
    // 共享四边形两侧的对角线一致，因此不出现内部边界面：
    // 两个立方体 10 个外露四边形 + 六棱柱 2 个六边形（各 4 个三角形）+ 6 个四边形。
    assert_eq!(mesh.boundary.len(), 10 * 2 + 2 * 4 + 6 * 2);
    assert_eq!(mesh.source_node_count(), 24);
    assert_eq!(mesh.nodes.len(), 26);
    let quality = crate::tet_quality(&mesh, &crate::Progress::new()).ok_or("cancelled")?;
    let poly = quality.by_type[CellType::Polyhedron.ordinal()];
    assert_eq!(poly.count, 2);
    assert!(poly.min_scaled_jacobian > 0.9 && poly.min_scaled_jacobian <= 1.0 + 1e-12);
    Ok(())
}

#[test]
fn open_inconsistent_concave_or_tampered_polyhedra_are_rejected() {
    let nodes = cube_nodes([0.0; 3]);
    let faces = cube_faces([0, 1, 2, 3, 4, 5, 6, 7]);
    let poly =
        |faces: &[Vec<MeshIndex>]| build(nodes.clone(), vec![CellType::Polyhedron], stream(faces));
    // 开口：缺一个面。
    assert!(poly(&faces[..5]).is_err());
    // 面方向不一致。
    let mut flipped = faces.clone();
    flipped[2].reverse();
    assert!(poly(&flipped).is_err());
    // 重复节点与过少的面。
    let mut repeated = faces.clone();
    repeated[0][1] = repeated[0][0];
    assert!(poly(&repeated).is_err());
    assert!(poly(&faces[..3]).is_err());
    // 面流长度与声明不符。
    let mut truncated = stream(&faces);
    truncated.pop();
    let types = vec![CellType::Polyhedron];
    assert!(cell_offsets(&types, &truncated).is_err());
    // 凹多面体（C 形截面拉伸）：质心落在缺口中，拆分出非正体积四面体。
    let outline = [
        [0.0, 0.0],
        [3.0, 0.0],
        [3.0, 1.0],
        [1.0, 1.0],
        [1.0, 2.0],
        [3.0, 2.0],
        [3.0, 3.0],
        [0.0, 3.0],
    ];
    let mut c_nodes = Vec::new();
    for z in [0.0, 1.0] {
        c_nodes.extend(outline.iter().map(|&[x, y]| [x, y, z]));
    }
    let mut c_faces: Vec<Vec<MeshIndex>> = vec![(0..8).rev().collect(), (8..16).collect()];
    for k in 0..8 {
        let next = (k + 1) % 8;
        c_faces.push(vec![k, next, next + 8, k + 8]);
    }
    assert!(build(c_nodes, vec![CellType::Polyhedron], stream(&c_faces)).is_err());
    // 中心节点号被篡改的布局不能通过校验。
    let mesh = poly_mesh();
    let mut tampered = mesh.clone();
    if let Some(layout) = tampered.cells.as_mut() {
        let layout = std::sync::Arc::make_mut(layout);
        let first = layout.offsets[1] as usize;
        layout.connectivity[first] = 0;
    }
    assert!(!validate_tet_mesh(&tampered).is_ok());
}

/// 以驻留网格为数据源的流式写入端，验证两种写入路径逐字节一致。
struct MeshSource<'a>(&'a TetMesh);

impl VolumeSource for MeshSource<'_> {
    fn nodes(&self, range: Range<usize>, out: &mut Vec<[f64; 3]>) {
        out.extend_from_slice(&self.0.nodes[range]);
    }
    fn tets(&self, _: Range<usize>, _: &mut Vec<crate::Tetrahedron>) {}
    fn boundary(&self, range: Range<usize>, out: &mut Vec<crate::SurfaceTriangle>) {
        out.extend_from_slice(&self.0.boundary[range]);
    }
    fn cells(&self, range: Range<usize>, out: &mut CellBlock) {
        let Some(layout) = &self.0.cells else { return };
        for cell in range {
            let first = layout
                .tet_cells
                .partition_point(|&owner| (owner as usize) < cell);
            out.push(
                layout.types[cell],
                self.0.tets[first].region,
                layout.cell_nodes(cell),
            );
        }
    }
}

fn stream_counts(mesh: &TetMesh) -> VolumeStreamCounts {
    VolumeStreamCounts {
        nodes: mesh.nodes.len(),
        tets: mesh.tets.len(),
        cells: mesh.cell_count(),
        boundary: mesh.boundary.len(),
        region_count: mesh.region_count,
        boundary_group_count: mesh.boundary_group_count,
        polyhedra: mesh.cell_type_counts()[CellType::Polyhedron.ordinal()] as usize,
    }
}

/// 资产编码、完整读取、流式写入、按块遍历与流式质量统计全部一致。
fn assert_asset_round_trip(mesh: &TetMesh, name: &str) -> TestResult {
    let asset = MeshAsset::Volume(mesh.clone());
    let (bytes, digest) = encode_mesh_asset(&asset)?;
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into()?), 5);
    assert_eq!(decode_mesh_asset(&bytes)?, asset);
    let streamed = MeshAssetReader::open(bytes.as_slice(), bytes.len() as u64)?.read(|| false)?;
    assert_eq!(streamed, Some((asset.clone(), digest.clone())));

    let mut written = std::io::Cursor::new(Vec::new());
    let result = crate::write_volume_asset_streaming(
        &mut written,
        stream_counts(mesh),
        &MeshSource(mesh),
        None,
    )?;
    assert_eq!(result, Some((bytes.len() as u64, digest.clone())));
    assert_eq!(written.into_inner(), bytes);

    let directory = std::env::temp_dir().join(format!("panta-poly-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("poly.pmesh");
    std::fs::write(&path, &bytes)?;
    let mut chunks = VolumeAssetChunks::open(&path)?;
    assert_eq!(chunks.counts(), stream_counts(mesh));
    let mut seen = CellBlock::default();
    assert!(chunks.visit_cells(None, |batch| {
        for block in batch {
            for (kind, region, nodes) in block.iter() {
                seen.push(kind, region, nodes);
            }
        }
        true
    })?);
    let layout = mesh.cells.as_ref().ok_or("layout")?;
    assert_eq!(seen.types, layout.types);
    assert_eq!(seen.connectivity, layout.connectivity);
    let mut tets = Vec::new();
    assert!(chunks.visit_cell_tets(None, |batch| {
        tets.extend(batch.iter().flatten().copied());
        true
    })?);
    let expected: Vec<(u32, crate::Tetrahedron)> = mesh
        .tets
        .iter()
        .enumerate()
        .map(|(index, tet)| (mesh.tet_cell(index) as u32, *tet))
        .collect();
    assert_eq!(tets, expected);
    let resident = crate::tet_quality(mesh, &crate::Progress::new());
    let streamed = crate::tet_quality_streaming(&mut chunks, &crate::Progress::new())?;
    drop(chunks);
    std::fs::remove_dir_all(&directory)?;
    let (resident, streamed) = (resident.ok_or("cancelled")?, streamed.ok_or("cancelled")?);
    assert_eq!(resident.by_type, streamed.by_type);
    assert_eq!(resident.poor_count, streamed.poor_count);
    Ok(())
}

#[test]
fn polyhedron_assets_round_trip_resident_and_streamed() -> TestResult {
    assert_asset_round_trip(&poly_mesh(), "small")
}

#[test]
fn polyhedron_face_chunks_pair_with_cell_chunks_across_many_chunks() -> TestResult {
    // 26³ 个单元 > 16384：第一块单元全是六面体（面流块为 0 条记录），之后出现多面体。
    let n = 26_usize;
    let side = n + 1;
    let index = |i: usize, j: usize, k: usize| ((k * side + j) * side + i) as MeshIndex;
    let nodes: Vec<[f64; 3]> = (0..side.pow(3))
        .map(|v| {
            [
                (v % side) as f64,
                ((v / side) % side) as f64,
                (v / side / side) as f64,
            ]
        })
        .collect();
    let mut types = Vec::new();
    let mut connectivity = Vec::new();
    for cell in 0..n.pow(3) {
        let (i, j, k) = (cell % n, (cell / n) % n, cell / n / n);
        let corners: [MeshIndex; 8] = std::array::from_fn(|corner| {
            let (di, dj) = [(0, 0), (1, 0), (1, 1), (0, 1)][corner % 4];
            index(i + di, j + dj, k + corner / 4)
        });
        if cell >= 17_000 && cell % 3 == 0 {
            types.push(CellType::Polyhedron);
            connectivity.extend(stream(&cube_faces(corners)));
        } else {
            types.push(CellType::Hexahedron);
            connectivity.extend_from_slice(&corners);
        }
    }
    let mesh = build(nodes, types, connectivity)?;
    assert!(mesh.cell_type_counts()[CellType::Polyhedron.ordinal()] > 100);
    assert_asset_round_trip(&mesh, "many")
}

#[test]
fn v4_mixed_assets_stay_readable() -> TestResult {
    let bytes = include_bytes!("../tests/data/mixed-v4.pmesh");
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into()?), 4);
    let MeshAsset::Volume(mesh) = decode_mesh_asset(bytes)? else {
        return Err("volume expected".into());
    };
    assert_eq!(mesh, crate::mixed_tests::mixed_mesh());
    let directory = std::env::temp_dir().join(format!("panta-poly-v4-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("mixed.pmesh");
    std::fs::write(&path, bytes)?;
    let mut chunks = VolumeAssetChunks::open(&path)?;
    assert_eq!(chunks.counts().polyhedra, 0);
    let mut cells = 0;
    assert!(chunks.visit_cells(None, |batch| {
        cells += batch.iter().map(CellBlock::len).sum::<usize>();
        true
    })?);
    drop(chunks);
    std::fs::remove_dir_all(&directory)?;
    assert_eq!(cells, 4);
    Ok(())
}

#[test]
fn chunked_build_rejects_polyhedra_until_pchunk_supports_them() {
    let mesh = poly_mesh();
    let result = crate::chunked::build_chunked_volume(
        &mesh,
        crate::chunked::ChunkBuildOptions::default(),
        std::io::sink(),
        &crate::Progress::new(),
    );
    assert!(
        matches!(result, Err(crate::chunked::ChunkError::Invalid(message)) if message.contains("polyhedron"))
    );
}

#[test]
fn node_fields_cover_source_nodes_and_interpolate_through_centres() -> TestResult {
    let mesh = poly_mesh();
    // 线性场 x：中心节点取单元节点平均（立方体 1.5、六棱柱 5.0，恰为质心 x），切面值应逐点等于 x。
    let mut field = crate::ResultField {
        name: "x".to_owned(),
        association: crate::FieldAssociation::Node,
        components: 1,
        units: String::new(),
        values: mesh.nodes[..mesh.source_node_count()]
            .iter()
            .map(|node| node[0])
            .collect(),
    };
    let section = |field: &crate::ResultField| {
        crate::section_surface_field(
            &mesh,
            field,
            crate::FieldComponent::Component(0),
            [0.0, 0.0, 0.5],
            [0.0, 0.0, 1.0],
            &crate::Progress::new(),
        )
    };
    let result = section(&field)?.ok_or("cancelled")?;
    assert!(!result.values.is_empty());
    for (triangle, values) in result.surface.triangles.iter().zip(&result.values) {
        for (corner, value) in triangle.iter().zip(values) {
            assert!((corner[0] - value).abs() < 1e-9, "{corner:?} {value}");
        }
    }
    // 节点场长度按源节点计，含中心节点的长度被拒绝。
    field.values.extend([0.0, 0.0]);
    assert!(section(&field).is_err());
    Ok(())
}
