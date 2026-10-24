use crate::export::{export_volume_asset_vtu, write_tet_mesh_vtu};
use crate::import::formats::{parse_vtk_legacy, parse_vtk_legacy_grid, parse_vtu, parse_vtu_grid};
use panta_mesh::{
    CellType, FieldAssociation, FieldComponent, MeshAsset, Progress, TetMesh, VolumeAssetChunks,
    encode_mesh_asset,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 六面体 [0,1]³（VTK 12）+ 立方体多面体 [1,2]×[0,1]²（与六面体共享 x=1 四边形面）
/// + 两个上下叠放的五棱柱多面体（共享五边形面）。
fn points() -> Vec<[f64; 3]> {
    let mut points = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
        [2.0, 0.0, 0.0],
        [2.0, 1.0, 0.0],
        [2.0, 0.0, 1.0],
        [2.0, 1.0, 1.0],
    ];
    for layer in 0..3 {
        for k in 0..5 {
            let angle = std::f64::consts::TAU * f64::from(k) / 5.0;
            points.push([5.0 + angle.cos(), angle.sin(), f64::from(layer)]);
        }
    }
    points
}

fn faces() -> Vec<Vec<Vec<u64>>> {
    let cube = vec![
        vec![1, 2, 9, 8],
        vec![5, 10, 11, 6],
        vec![1, 5, 6, 2],
        vec![8, 9, 11, 10],
        vec![1, 8, 10, 5],
        vec![2, 6, 11, 9],
    ];
    let prism = |base: u64| {
        let top = base + 5;
        let mut faces = vec![
            (0..5).rev().map(|k| base + k).collect::<Vec<_>>(),
            (0..5).map(|k| top + k).collect(),
        ];
        for k in 0..5 {
            let next = (k + 1) % 5;
            faces.push(vec![base + k, base + next, top + next, top + k]);
        }
        faces
    };
    vec![cube, prism(12), prism(17)]
}

fn stream(faces: &[Vec<u64>]) -> Vec<u64> {
    let mut stream = vec![faces.len() as u64];
    for face in faces {
        stream.push(face.len() as u64);
        stream.extend(face);
    }
    stream
}

fn unique(faces: &[Vec<u64>]) -> Vec<u64> {
    let mut nodes: Vec<u64> = faces.iter().flatten().copied().collect();
    nodes.sort_unstable();
    nodes.dedup();
    nodes
}

fn line(values: &[u64]) -> String {
    values
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

fn point_lines() -> String {
    points()
        .iter()
        .map(|[x, y, z]| format!("{x:?} {y:?} {z:?}\n"))
        .collect()
}

const HEX: [u64; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

fn legacy() -> String {
    let streams: Vec<Vec<u64>> = faces().iter().map(|faces| stream(faces)).collect();
    let size = 9 + streams.iter().map(|s| s.len() + 1).sum::<usize>();
    let mut text = format!(
        "# vtk DataFile Version 4.2\npoly\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 27 double\n{}CELLS 4 {size}\n8 {}\n",
        point_lines(),
        line(&HEX)
    );
    for stream in &streams {
        text.push_str(&format!("{} {}\n", stream.len(), line(stream)));
    }
    text.push_str("CELL_TYPES 4\n12\n42\n42\n42\n");
    text.push_str("CELL_DATA 4\nSCALARS id double 1\nLOOKUP_TABLE default\n0 1 2 3\n");
    let x: Vec<String> = points().iter().map(|p| format!("{:?}", p[0])).collect();
    text.push_str(&format!(
        "POINT_DATA 27\nSCALARS x double 1\nLOOKUP_TABLE default\n{}\n",
        x.join(" ")
    ));
    text
}

/// 手写 VTK 9.4（VTKFile 2.3）布局：polyhedron_offsets / polyhedron_to_faces /
/// face_offsets / face_connectivity，偏移数组省略首个 0。
fn vtu_94() -> String {
    let polys = faces();
    let mut connectivity = HEX.to_vec();
    let mut offsets = vec![8_u64];
    let (mut face_connectivity, mut face_offsets) = (Vec::new(), Vec::new());
    let (mut to_faces, mut poly_offsets) = (Vec::new(), vec![0_u64]);
    for faces in &polys {
        connectivity.extend(unique(faces));
        offsets.push(connectivity.len() as u64);
        for face in faces {
            to_faces.push(face_offsets.len() as u64);
            face_connectivity.extend(face);
            face_offsets.push(face_connectivity.len() as u64);
        }
        poly_offsets.push(to_faces.len() as u64);
    }
    let array = |name: &str, values: &[u64]| {
        format!(
            "<DataArray type=\"Int64\" Name=\"{name}\" format=\"ascii\">{}</DataArray>\n",
            line(values)
        )
    };
    format!(
        "<?xml version=\"1.0\"?>\n<VTKFile type=\"UnstructuredGrid\" version=\"2.3\" byte_order=\"LittleEndian\">\n\
<UnstructuredGrid><Piece NumberOfPoints=\"27\" NumberOfCells=\"4\">\n\
<Points><DataArray type=\"Float64\" NumberOfComponents=\"3\" format=\"ascii\">{}</DataArray></Points>\n\
<Cells>{}{}{}{}{}{}{}</Cells></Piece></UnstructuredGrid></VTKFile>\n",
        point_lines(),
        array("connectivity", &connectivity),
        array("offsets", &offsets),
        "<DataArray type=\"UInt8\" Name=\"types\" format=\"ascii\">12 42 42 42</DataArray>\n",
        array("face_connectivity", &face_connectivity),
        array("face_offsets", &face_offsets),
        array("polyhedron_to_faces", &to_faces),
        array("polyhedron_offsets", &poly_offsets),
    )
}

fn total_volume(mesh: &TetMesh) -> f64 {
    mesh.tets
        .iter()
        .map(|tet| {
            let [a, b, c, d] = tet.nodes.map(|node| mesh.nodes[node as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
            (u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
                + u[2] * (v[0] * w[1] - v[1] * w[0]))
                / 6.0
        })
        .sum()
}

fn expected_volume() -> f64 {
    let pentagon = 2.5 * (std::f64::consts::TAU / 5.0).sin();
    2.0 + 2.0 * pentagon
}

#[test]
fn legacy_42_imports_with_fields_and_round_trips_through_vtu() -> TestResult {
    let mut grid = parse_vtk_legacy_grid(legacy().as_bytes())?;
    assert_eq!(
        grid.types,
        vec![
            CellType::Hexahedron,
            CellType::Polyhedron,
            CellType::Polyhedron,
            CellType::Polyhedron
        ]
    );
    let field = |grid: &crate::import::formats::RawGrid, name: &str| {
        grid.fields
            .iter()
            .find(|field| field.name == name)
            .cloned()
            .ok_or(format!("field {name}"))
    };
    let id = field(&grid, "id")?;
    let x = field(&grid, "x")?;
    assert_eq!(id.association, FieldAssociation::Cell);
    assert_eq!(id.values, vec![0.0, 1.0, 2.0, 3.0]);
    assert_eq!(x.association, FieldAssociation::Node);
    assert_eq!(x.values.len(), 27, "节点场按源节点计");
    let mesh = grid.take_mesh()?;
    assert_eq!(mesh.cell_count(), 4);
    assert_eq!(mesh.source_node_count(), 27);
    assert_eq!(mesh.nodes.len(), 30, "每个多面体一个辅助中心节点");
    assert_eq!(mesh.cell_type_counts()[CellType::Polyhedron.ordinal()], 3);
    assert!((total_volume(&mesh) - expected_volume()).abs() < 1e-12);
    let layout = mesh.cells.as_deref().ok_or("cell layout")?;

    // 字段绑定：节点场线性 x 的切面逐点等于 x；单元场切多面体得原单元值。
    let section = |field: &panta_mesh::ResultField, origin: [f64; 3]| {
        panta_mesh::section_surface_field(
            &mesh,
            field,
            FieldComponent::Component(0),
            origin,
            [0.0, 0.0, 1.0],
            &Progress::new(),
        )
    };
    let nodes = section(&x, [0.0, 0.0, 0.5])?.ok_or("cancelled")?;
    assert!(!nodes.values.is_empty());
    for (triangle, values) in nodes.surface.triangles.iter().zip(&nodes.values) {
        for (corner, value) in triangle.iter().zip(values) {
            assert!((corner[0] - value).abs() < 1e-9, "{corner:?} {value}");
        }
    }
    let cells = section(&id, [0.0, 0.0, 1.5])?.ok_or("cancelled")?;
    assert!(!cells.values.is_empty());
    // 切面结果含被保留一侧的外表面；平面 z=1.5 上的三角形只来自上层五棱柱（id 3）。
    let mut on_plane = 0;
    for (triangle, values) in cells.surface.triangles.iter().zip(&cells.values) {
        if triangle
            .iter()
            .all(|corner| (corner[2] - 1.5).abs() < 1e-12)
        {
            on_plane += 1;
            assert_eq!(values, &[3.0; 3]);
        }
    }
    assert!(on_plane > 0);

    // VTU 导出（旧式 faces/faceoffsets）→ 再导入：类型、连接（含中心节点编号）与源节点一致。
    let mut bytes = Vec::new();
    write_tet_mesh_vtu(&mesh, &mut bytes)?;
    let text = String::from_utf8(bytes.clone())?;
    assert!(text.contains("NumberOfPoints=\"27\""));
    assert!(text.contains("Name=\"faces\"") && text.contains("Name=\"faceoffsets\""));
    let mut reread = parse_vtu_grid(&bytes)?;
    assert_eq!(reread.points, mesh.nodes[..27]);
    let again = reread.take_mesh()?;
    let again_layout = again.cells.as_deref().ok_or("cell layout")?;
    assert_eq!(again_layout.types, layout.types);
    assert_eq!(again_layout.connectivity, layout.connectivity);
    assert_eq!(again.nodes, mesh.nodes);
    assert_eq!(again.tets.len(), mesh.tets.len());

    // 流式资产导出与驻留导出逐字节一致。
    let directory = std::env::temp_dir().join(format!("panta-poly-io-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let asset = directory.join("poly.pmesh");
    let (encoded, _) = encode_mesh_asset(&MeshAsset::Volume(mesh.clone()))?;
    std::fs::write(&asset, encoded)?;
    let mut chunks = VolumeAssetChunks::open(&asset)?;
    let streamed = directory.join("streamed.vtu");
    export_volume_asset_vtu(&mut chunks, &streamed, &Progress::new())?;
    let streamed_bytes = std::fs::read(&streamed)?;
    std::fs::remove_dir_all(&directory)?;
    assert_eq!(streamed_bytes, bytes);
    Ok(())
}

#[test]
fn vtk_94_polyhedron_arrays_match_legacy_import() -> TestResult {
    let legacy = parse_vtk_legacy_grid(legacy().as_bytes())?.take_mesh()?;
    let modern = parse_vtu_grid(vtu_94().as_bytes())?.take_mesh()?;
    assert_eq!(modern.nodes, legacy.nodes);
    assert_eq!(modern.cells, legacy.cells);
    assert_eq!(modern.tets.len(), legacy.tets.len());
    Ok(())
}

#[test]
fn surface_import_cancels_shared_quad_and_polygon_faces() -> TestResult {
    // 外表面：六面体 + 立方体多面体 10 个四边形（20 三角形）；五棱柱 2 个五边形（各 3）+ 10 个四边形。
    let expected = 20 + 2 * 3 + 20;
    let legacy_surface = parse_vtk_legacy(legacy().as_bytes())?;
    assert_eq!(legacy_surface.triangles.len(), expected);
    assert_eq!(parse_vtu(vtu_94().as_bytes())?.triangles.len(), expected);
    let mesh = parse_vtk_legacy_grid(legacy().as_bytes())?.take_mesh()?;
    let mut bytes = Vec::new();
    write_tet_mesh_vtu(&mesh, &mut bytes)?;
    assert_eq!(parse_vtu(&bytes)?.triangles.len(), expected);
    Ok(())
}

#[test]
fn malformed_polyhedra_are_rejected() {
    // VTU 多面体缺面数组。
    let missing = vtu_94()
        .lines()
        .filter(|line| !line.contains("polyhedron_offsets"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = parse_vtu_grid(missing.as_bytes())
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.contains("polyhedron"), "{error}");
    assert!(parse_vtu(missing.as_bytes()).is_err());
    // 面流长度与节点表不符、面数不足、面节点越界。
    let text = legacy();
    let first = stream(&faces()[0]);
    let declared = format!("{} {}", first.len(), line(&first));
    let mut short = first.clone();
    short.truncate(1 + 3 * 5);
    short[0] = 3;
    for bad in [
        format!("{} {}", first.len(), line(&first[..first.len() - 1])),
        format!("{} {}", first.len(), line(&first).replacen("6 4", "6 5", 1)),
        format!("{} {}", short.len(), line(&short)),
        declared.replace(" 11 ", " 99 "),
    ] {
        let broken = text.replace(&declared, &bad);
        assert_ne!(broken, text);
        assert!(parse_vtk_legacy_grid(broken.as_bytes()).is_err(), "{bad}");
    }
}
