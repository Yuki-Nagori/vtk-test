//! 混合单元分块（task 030 PR 3）测试：合成边界层块经流式资产与驻留网格两条路径分块，
//! 结果、原始单元 ID、逐类型质量、切面与单元场切面都与驻留算法一致。

use std::error::Error;
use std::sync::Arc;

use super::format::{ChunkBytes, ChunkSource, IoCounts};
use super::{
    ChunkBuildOptions, ChunkError, ChunkedVolume, ResidentFields, SectionPlane,
    build_chunked_from_asset_with_fields, build_chunked_volume, build_chunked_volume_with_fields,
};
use crate::{
    CellType, FieldAssociation, FieldComponent, FieldSection, MeshAsset, Progress, ResultField,
    SyntheticMixedBlock, TetMesh, TetQuality, section_surface, section_surface_field, tet_quality,
    validate_tet_mesh,
};

type TestResult = Result<(), Box<dyn Error>>;

struct MemorySource(Vec<u8>);

impl ChunkSource for MemorySource {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&self, offset: u64, len: usize) -> Result<ChunkBytes, ChunkError> {
        self.0
            .get(offset as usize..offset as usize + len)
            .map(|bytes| ChunkBytes::Owned(bytes.to_vec()))
            .ok_or_else(|| ChunkError::Format("range".to_owned()))
    }
    fn io_counts(&self) -> IoCounts {
        IoCounts::default()
    }
    fn kind(&self) -> &'static str {
        "memory"
    }
}

fn open(bytes: Vec<u8>) -> Result<ChunkedVolume, ChunkError> {
    ChunkedVolume::open(Arc::new(MemorySource(bytes)), usize::MAX)
}

struct TempPath(std::path::PathBuf);

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 流式写出合成块，返回（资产字节，完整解码并校验后的驻留网格）。
fn streamed(
    block: &SyntheticMixedBlock,
    name: &str,
) -> Result<(TempPath, TetMesh), Box<dyn Error>> {
    streamed_source(block, block.counts(), name)
}

fn streamed_source<S: crate::VolumeSource>(
    block: &S,
    counts: crate::VolumeStreamCounts,
    name: &str,
) -> Result<(TempPath, TetMesh), Box<dyn Error>> {
    let path = TempPath(std::env::temp_dir().join(format!(
        "panta-mixed-chunk-{}-{name}.pmesh",
        std::process::id()
    )));
    crate::write_volume_asset_streaming(std::fs::File::create(&path.0)?, counts, block, None)?
        .ok_or("cancelled")?;
    let MeshAsset::Volume(mesh) = crate::decode_mesh_asset(&std::fs::read(&path.0)?)? else {
        return Err("not a volume".into());
    };
    Ok((path, mesh))
}

fn options(max: usize) -> ChunkBuildOptions {
    ChunkBuildOptions {
        max_chunk_cells: max,
        simplify_ratio: 4,
    }
}

fn assert_quality_eq(actual: &TetQuality, expected: &TetQuality) {
    assert_eq!(actual.tet_count, expected.tet_count);
    assert_eq!(actual.poor_count, expected.poor_count);
    assert_eq!(actual.min_dihedral_degrees, expected.min_dihedral_degrees);
    assert_eq!(actual.max_aspect_ratio, expected.max_aspect_ratio);
    assert!((actual.mean_aspect_ratio - expected.mean_aspect_ratio).abs() < 1e-9);
    for (a, e) in actual.by_type.iter().zip(&expected.by_type) {
        assert_eq!(a.count, e.count);
        assert_eq!(a.poor_count, e.poor_count);
        assert_eq!(a.min_scaled_jacobian, e.min_scaled_jacobian);
        assert!((a.mean_scaled_jacobian - e.mean_scaled_jacobian).abs() < 1e-9);
    }
}

fn sorted_keys(triangles: &[[[f64; 3]; 3]]) -> Vec<[u64; 9]> {
    let mut keys: Vec<[u64; 9]> = triangles
        .iter()
        .map(|t| std::array::from_fn(|i| t[i / 3][i % 3].to_bits()))
        .collect();
    keys.sort_unstable();
    keys
}

fn keyed(section: &FieldSection) -> Vec<([u64; 9], [u64; 3])> {
    let mut keys: Vec<_> = section
        .surface
        .triangles
        .iter()
        .zip(&section.values)
        .map(|(t, v)| {
            (
                std::array::from_fn(|i| t[i / 3][i % 3].to_bits()),
                v.map(f64::to_bits),
            )
        })
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn synthetic_mixed_block_streams_a_valid_conformal_asset() -> TestResult {
    let block = SyntheticMixedBlock::with_layers(4, 2, 2);
    let (path, mesh) = streamed(&block, "valid")?;
    let report = validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert_eq!(mesh.cell_count(), block.cell_count());
    assert_eq!(mesh.tets.len(), block.tet_count());
    assert_eq!(mesh.cell_type_counts(), block.type_counts());
    assert_eq!(mesh.boundary.len(), block.boundary_count());
    // 解析边界与拆分四面体的外表面（按节点集合）一致。
    let face_key = |nodes: [u32; 3]| {
        let mut key = nodes;
        key.sort_unstable();
        key
    };
    let mut expected: Vec<_> = crate::boundary_faces(&mesh.tets)
        .iter()
        .map(|face| face_key(face.nodes))
        .collect();
    let mut actual: Vec<_> = mesh
        .boundary
        .iter()
        .map(|face| face_key(face.nodes))
        .collect();
    expected.sort_unstable();
    actual.sort_unstable();
    assert_eq!(actual, expected);
    // 体积 = 100 × 100 × 总厚度。
    let height = block.node(block.node_count() - 2 * 16 - 1)[2];
    assert!((report.volume_mm3 - 1e4 * height).abs() < 1e-6 * report.volume_mm3);
    // 流式写出与驻留编码逐字节一致。
    let (bytes, _) = crate::encode_mesh_asset(&MeshAsset::Volume(mesh))?;
    assert_eq!(std::fs::read(&path.0)?, bytes);
    Ok(())
}

#[test]
fn streaming_writer_rejects_invalid_mixed_records() -> TestResult {
    struct Broken {
        block: SyntheticMixedBlock,
        node_out_of_range: bool,
    }
    impl crate::VolumeSource for Broken {
        fn nodes(&self, range: std::ops::Range<usize>, out: &mut Vec<[f64; 3]>) {
            self.block.nodes(range, out);
        }
        fn tets(&self, _: std::ops::Range<usize>, _: &mut Vec<crate::Tetrahedron>) {}
        fn boundary(&self, range: std::ops::Range<usize>, out: &mut Vec<crate::SurfaceTriangle>) {
            self.block.boundary(range, out);
        }
        fn cells(&self, range: std::ops::Range<usize>, out: &mut crate::CellBlock) {
            let start = out.connectivity.len();
            self.block.cells(range, out);
            if self.node_out_of_range {
                out.connectivity[start] = u32::MAX - 1;
            }
        }
    }
    let block = SyntheticMixedBlock::with_layers(3, 1, 1);
    let broken = Broken {
        block: block.clone(),
        node_out_of_range: true,
    };
    let sink = || std::io::Cursor::new(Vec::new());
    assert!(crate::write_volume_asset_streaming(sink(), block.counts(), &broken, None).is_err());
    // 拆分四面体总数与声明不符。
    let mut counts = block.counts();
    counts.tets += 1;
    let honest = Broken {
        block: block.clone(),
        node_out_of_range: false,
    };
    assert!(crate::write_volume_asset_streaming(sink(), counts, &honest, None).is_err());
    let mut counts = block.counts();
    counts.cells = counts.tets + 1;
    assert!(crate::write_volume_asset_streaming(sink(), counts, &honest, None).is_err());
    assert!(crate::write_volume_asset_streaming(sink(), block.counts(), &honest, None)?.is_some());
    Ok(())
}

fn cell_field(mesh: &TetMesh) -> ResultField {
    ResultField {
        name: "cell-id".to_owned(),
        association: FieldAssociation::Cell,
        components: 1,
        units: String::new(),
        values: (0..mesh.cell_count())
            .map(|cell| cell as f64 * 0.5)
            .collect(),
    }
}

/// 节点场只覆盖源节点（多面体中心节点除外）。
fn node_field(mesh: &TetMesh) -> ResultField {
    ResultField {
        name: "z".to_owned(),
        association: FieldAssociation::Node,
        components: 1,
        units: "mm".to_owned(),
        values: mesh.nodes[..mesh.source_node_count()]
            .iter()
            .map(|p| p[2])
            .collect(),
    }
}

/// 三分量节点场（多面体中心节点值为各唯一节点的平均，按分量）。
fn vector_field(mesh: &TetMesh) -> ResultField {
    ResultField {
        name: "u".to_owned(),
        association: FieldAssociation::Node,
        components: 3,
        units: "m/s".to_owned(),
        values: mesh.nodes[..mesh.source_node_count()]
            .iter()
            .flat_map(|p| [p[1] - 50.0, 0.3 * p[0], (0.05 * p[2]).sin()])
            .collect(),
    }
}

#[test]
fn mixed_chunks_keep_original_cells_and_match_resident_algorithms() -> TestResult {
    let block = SyntheticMixedBlock::with_layers(5, 3, 2);
    let (path, mesh) = streamed(&block, "chunks")?;
    let height = mesh.nodes.iter().map(|p| p[2]).fold(0.0, f64::max);
    let planes = [
        ([50.0, 50.0, 0.02 * height], [0.0, 0.0, 1.0]),
        ([50.0, 50.0, 0.5 * height], [0.3, 0.2, 1.0]),
        ([31.0, 47.0, 0.4 * height], [1.0, 0.1, 0.0]),
    ];
    assert_chunks_match_resident(&path, &mesh, block.type_counts(), 120, &planes)
}

/// 多面体（task 031 PR 2）：加密界面合成块经资产与驻留两条路径分块，面流映射回全局后与驻留一致，
/// 正交质量、几何切面、标量 / 矢量节点场（含中心节点插值）与单元场切面逐位一致。
#[test]
fn polyhedral_chunks_match_resident_algorithms() -> TestResult {
    let block = crate::SyntheticPolyBlock::with_cells_per_axis(4);
    let (path, mesh) = streamed_source(&block, block.counts(), "poly")?;
    let report = validate_tet_mesh(&mesh);
    assert!(report.is_ok(), "{:?}", report.issues);
    assert!((report.volume_mm3 - 1e6).abs() < 1e-6);
    assert_eq!(mesh.cell_type_counts(), block.type_counts());
    assert_eq!(mesh.tets.len(), block.tet_count());
    // 解析边界与拆分四面体外表面一致（共形）。
    let key = |mut nodes: [u32; 3]| {
        nodes.sort_unstable();
        nodes
    };
    let mut expected: Vec<_> = crate::boundary_faces(&mesh.tets)
        .iter()
        .map(|face| key(face.nodes))
        .collect();
    let mut actual: Vec<_> = mesh.boundary.iter().map(|face| key(face.nodes)).collect();
    expected.sort_unstable();
    actual.sort_unstable();
    assert_eq!(actual, expected);
    let (bytes, _) = crate::encode_mesh_asset(&MeshAsset::Volume(mesh.clone()))?;
    assert_eq!(std::fs::read(&path.0)?, bytes);
    let planes = [
        ([50.0, 50.0, 75.0], [0.0, 0.0, 1.0]),
        ([50.0, 50.0, 50.0], [0.3, 0.2, 1.0]),
        ([31.0, 47.0, 60.0], [1.0, 0.1, 0.4]),
    ];
    assert_chunks_match_resident(&path, &mesh, block.type_counts(), 40, &planes)
}

fn assert_chunks_match_resident(
    path: &TempPath,
    mesh: &TetMesh,
    type_counts: [u64; crate::CELL_TYPE_COUNT],
    max_chunk_cells: usize,
    planes: &[([f64; 3], [f64; 3])],
) -> TestResult {
    let fields = vec![node_field(mesh), cell_field(mesh), vector_field(mesh)];
    // 资产路径与驻留路径产出逐字节相同的 .pchunk。
    let mut from_mesh = Vec::new();
    build_chunked_volume_with_fields(
        mesh,
        &mut ResidentFields(&fields),
        options(max_chunk_cells),
        &mut from_mesh,
        &Progress::new(),
    )?
    .ok_or("cancelled")?;
    let mut chunks = crate::VolumeAssetChunks::open(&path.0)?;
    let mut from_asset = Vec::new();
    let summary = build_chunked_from_asset_with_fields(
        &mut chunks,
        &mut ResidentFields(&fields),
        options(max_chunk_cells),
        &mut from_asset,
        &Progress::new(),
    )?
    .ok_or("cancelled")?;
    assert_eq!(summary.cell_count, mesh.cell_count() as u64);
    assert_eq!(from_asset, from_mesh);
    let volume = open(from_asset)?;
    assert_eq!(volume.cell_count(), mesh.cell_count() as u64);
    assert_eq!(volume.tet_count(), mesh.tets.len() as u64);
    assert!(volume.headers().len() >= 8);

    // 每块：原始单元整体落在同一块；拆分四面体与布局映射回全局后与驻留网格一致。
    let layout = mesh.cells.as_ref().ok_or("layout")?;
    let ids: Vec<usize> = (0..volume.headers().len()).collect();
    let mut seen = vec![0_usize; mesh.cell_count()];
    for (header, cells) in volume.headers().iter().zip(volume.cache().get_many(&ids)?) {
        let chunk_layout = cells.layout.as_ref().ok_or("mixed chunk has layout")?;
        assert_eq!(cells.cell_count(), header.cell_count as usize);
        for local in 0..cells.cell_count() {
            let first = chunk_layout
                .tet_cells
                .partition_point(|&owner| (owner as usize) < local);
            let global = cells.global_cells[first] as usize;
            seen[global] += 1;
            assert_eq!(chunk_layout.types[local], layout.types[global]);
            let mut nodes = Vec::new();
            crate::cells::map_cell_nodes(
                chunk_layout.types[local],
                chunk_layout.cell_nodes(local),
                &mut nodes,
                |node| cells.global_nodes[node as usize] as u32,
            );
            assert_eq!(nodes, layout.cell_nodes(global));
        }
        for (index, tet) in cells.tets.iter().enumerate() {
            let global = cells.global_cells[index] as usize;
            assert_eq!(cells.tet_cell(index), local_of(chunk_layout, index));
            let nodes = tet
                .nodes
                .map(|node| cells.global_nodes[node as usize] as u32);
            let resident = (0..mesh.tets.len())
                .filter(|&t| mesh.tet_cell(t) == global)
                .any(|t| mesh.tets[t].nodes == nodes && mesh.tets[t].region == tet.region);
            assert!(resident, "tet of cell {global} not found");
        }
    }
    assert!(seen.iter().all(|&count| count == 1));

    // 逐类型质量与驻留统计一致。
    let expected = tet_quality(mesh, &Progress::new()).ok_or("cancel")?;
    let actual = volume.quality(&|| false)?.ok_or("cancel")?;
    assert_quality_eq(&actual, &expected);
    assert_eq!(actual.by_type.map(|q| q.count as u64), type_counts);
    assert_eq!(actual.tet_count, type_counts[0] as usize);

    // 几何切面与单元场 / 节点场切面与驻留一致。
    for &(origin, normal) in planes {
        let plane = SectionPlane::new(origin, normal)?;
        let plain = section_surface(mesh, origin, normal, &Progress::new())?.ok_or("cancel")?;
        let chunked = volume.fine_section(&plane, &|| false)?.ok_or("cancel")?;
        assert!(!plain.triangles.is_empty());
        assert_eq!(
            sorted_keys(&chunked.triangles),
            sorted_keys(&plain.triangles)
        );
        for (index, field) in fields.iter().enumerate() {
            let resident = section_surface_field(
                mesh,
                field,
                FieldComponent::Magnitude,
                origin,
                normal,
                &Progress::new(),
            )?
            .ok_or("cancel")?;
            let chunked = volume
                .fine_section_field(&plane, index, FieldComponent::Magnitude, &|| false)?
                .ok_or("cancel")?;
            assert_eq!(keyed(&chunked), keyed(&resident), "field {index}");
        }
    }
    Ok(())
}

/// 第 `tet` 个拆分四面体的块内单元下标。
fn local_of(layout: &super::ChunkCellLayout, tet: usize) -> usize {
    layout.tet_cells[tet] as usize
}

#[test]
fn pure_tet_chunks_have_no_layout_and_old_versions_are_rejected() -> TestResult {
    let block = crate::SyntheticTetBlock::with_cells_per_axis(3);
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
    let mut bytes = Vec::new();
    build_chunked_volume(&mesh, options(40), &mut bytes, &Progress::new())?.ok_or("cancel")?;
    let volume = open(bytes.clone())?;
    assert_eq!(volume.cell_count(), volume.tet_count());
    let ids: Vec<usize> = (0..volume.headers().len()).collect();
    for cells in volume.cache().get_many(&ids)? {
        assert!(cells.layout.is_none());
        assert_eq!(cells.cell_count(), cells.tets.len());
    }
    // 头部与尾部版本号改为 2（task 029 格式）：拒绝打开，派生缓存因此重建。
    let mut old = bytes.clone();
    old[8..12].copy_from_slice(&2_u32.to_le_bytes());
    assert!(matches!(open(old), Err(ChunkError::Format(_))));
    let mut old = bytes;
    let tail = old.len() - 12;
    old[tail..tail + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(matches!(open(old), Err(ChunkError::Format(_))));
    // 类型：确保混合质量类型数组顺序未变。
    assert_eq!(CellType::ALL[0], CellType::Tetra);
    Ok(())
}
