//! 分块结果场（task 029 PR 2）测试。

use std::error::Error;
use std::sync::Arc;

use super::format::{ChunkBytes, ChunkSource, IoCounts};
use super::{
    ChunkBuildOptions, ChunkError, ChunkedVolume, ResidentFields, SectionPlane,
    build_chunked_volume_with_fields,
};
use crate::{
    FieldAssociation, FieldComponent, FieldSection, Progress, ResultField, SyntheticTetBlock,
    TetMesh, section_surface, section_surface_field,
};

struct MemorySource {
    data: Vec<u8>,
    reads: std::sync::atomic::AtomicU64,
}

impl ChunkSource for MemorySource {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }
    fn read_at(&self, offset: u64, len: usize) -> Result<ChunkBytes, ChunkError> {
        self.reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.data
            .get(offset as usize..offset as usize + len)
            .map(|bytes| ChunkBytes::Owned(bytes.to_vec()))
            .ok_or_else(|| ChunkError::Format("range".to_owned()))
    }
    fn io_counts(&self) -> IoCounts {
        IoCounts {
            reads: self.reads.load(std::sync::atomic::Ordering::Relaxed),
            ..IoCounts::default()
        }
    }
    fn kind(&self) -> &'static str {
        "memory"
    }
}

fn mesh(n: usize) -> TetMesh {
    let block = SyntheticTetBlock::with_cells_per_axis(n);
    TetMesh {
        nodes: (0..block.node_count()).map(|i| block.node(i)).collect(),
        tets: (0..block.tet_count()).map(|i| block.tet(i)).collect(),
        boundary: (0..block.boundary_count())
            .map(|i| block.boundary_face(i))
            .collect(),
        region_count: 1,
        boundary_group_count: 6,
    }
}

fn linear(point: [f64; 3]) -> f64 {
    point[0] + 2.0 * point[1] - 0.5 * point[2]
}

fn fields(mesh: &TetMesh) -> Vec<ResultField> {
    let mut vector: Vec<f64> = mesh.nodes.iter().flat_map(|p| *p).collect();
    vector[3 * 5] = f64::NAN;
    vec![
        ResultField {
            name: "linear".to_owned(),
            association: FieldAssociation::Node,
            components: 1,
            units: String::new(),
            values: mesh.nodes.iter().map(|&p| linear(p)).collect(),
        },
        ResultField {
            name: "position".to_owned(),
            association: FieldAssociation::Node,
            components: 3,
            units: "mm".to_owned(),
            values: vector,
        },
        ResultField {
            name: "id".to_owned(),
            association: FieldAssociation::Cell,
            components: 1,
            units: String::new(),
            values: (0..mesh.tets.len()).map(|i| i as f64).collect(),
        },
    ]
}

fn build(mesh: &TetMesh, fields: &[ResultField]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut bytes = Vec::new();
    let options = ChunkBuildOptions {
        max_chunk_cells: 200,
        simplify_ratio: 4,
    };
    build_chunked_volume_with_fields(
        mesh,
        &mut ResidentFields(fields),
        options,
        &mut bytes,
        &Progress::new(),
    )?
    .ok_or("cancelled")?;
    Ok(bytes)
}

fn open(bytes: Vec<u8>) -> Result<ChunkedVolume, ChunkError> {
    ChunkedVolume::open(
        Arc::new(MemorySource {
            data: bytes,
            reads: Default::default(),
        }),
        usize::MAX,
    )
}

/// （三角形坐标位，三个顶点值位）排序后的多重集合。
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

const PLANES: [([f64; 3], [f64; 3]); 3] = [
    ([50.0, 50.0, 50.0], [0.3, 0.2, 1.0]),
    ([10.0, 70.0, 20.0], [1.0, 0.0, 0.0]),
    ([0.0, 0.0, 0.0], [-1.0, -1.0, -1.0]),
];

#[test]
fn field_sections_match_resident_sections_and_plain_geometry() -> Result<(), Box<dyn Error>> {
    let mesh = mesh(7);
    let fields = fields(&mesh);
    let volume = open(build(&mesh, &fields)?)?;
    assert_eq!(volume.fields().len(), 3);
    assert_eq!(volume.field_index("id", FieldAssociation::Cell), Some(2));
    assert_eq!(volume.field_index("id", FieldAssociation::Node), None);
    // 块级区间落在全局区间内，且整体覆盖全局区间。
    for (index, field) in fields.iter().enumerate() {
        let [low, high] = field.summary().range.ok_or("range")?;
        let mut seen = [f64::INFINITY, f64::NEG_INFINITY];
        for header in volume.headers() {
            let [a, b] = header.field_ranges[index];
            assert!(a >= low && b <= high && a <= b);
            seen = [seen[0].min(a), seen[1].max(b)];
        }
        assert_eq!(seen, [low, high]);
    }
    let cases = [
        (0, FieldComponent::Magnitude),
        (1, FieldComponent::Magnitude),
        (1, FieldComponent::Component(2)),
        (2, FieldComponent::Component(0)),
    ];
    for (origin, normal) in PLANES {
        let plain = section_surface(&mesh, origin, normal, &Progress::new())?.ok_or("cancel")?;
        let plane = SectionPlane::new(origin, normal)?;
        let chunk_plain = volume.fine_section(&plane, &|| false)?.ok_or("cancel")?;
        for (field, component) in cases {
            let resident = section_surface_field(
                &mesh,
                &fields[field],
                component,
                origin,
                normal,
                &Progress::new(),
            )?
            .ok_or("cancel")?;
            assert_eq!(resident.surface, plain, "resident geometry");
            assert_eq!(resident.values.len(), plain.triangles.len());
            let chunked = volume
                .fine_section_field(&plane, field, component, &|| false)?
                .ok_or("cancel")?;
            assert_eq!(chunked.surface, chunk_plain, "chunked geometry");
            assert_eq!(keyed(&chunked), keyed(&resident));
        }
    }
    Ok(())
}

#[test]
fn node_fields_interpolate_linearly_and_cell_fields_are_constant() -> Result<(), Box<dyn Error>> {
    let mesh = mesh(5);
    let fields = fields(&mesh);
    let (origin, normal) = PLANES[0];
    let section = section_surface_field(
        &mesh,
        &fields[0],
        FieldComponent::Magnitude,
        origin,
        normal,
        &Progress::new(),
    )?
    .ok_or("cancel")?;
    for (triangle, values) in section.surface.triangles.iter().zip(&section.values) {
        for (point, value) in triangle.iter().zip(values) {
            assert!((linear(*point) - value).abs() < 1e-9, "{value}");
        }
    }
    let range = section.value_range().ok_or("range")?;
    assert!(range[0] < range[1]);
    assert_eq!(section.missing_vertices(), 0);
    let vector = section_surface_field(
        &mesh,
        &fields[1],
        FieldComponent::Component(0),
        origin,
        normal,
        &Progress::new(),
    )?
    .ok_or("cancel")?;
    for (triangle, values) in vector.surface.triangles.iter().zip(&vector.values) {
        for (point, value) in triangle.iter().zip(values) {
            assert!(value.is_nan() || (point[0] - value).abs() < 1e-9);
        }
    }
    let cells = section_surface_field(
        &mesh,
        &fields[2],
        FieldComponent::Magnitude,
        origin,
        normal,
        &Progress::new(),
    )?
    .ok_or("cancel")?;
    for values in &cells.values {
        assert!(values[0] == values[1] && values[1] == values[2]);
        assert!(values[0] >= 0.0 && values[0].fract() == 0.0);
    }
    Ok(())
}

#[test]
fn missing_values_propagate_as_nan() -> Result<(), Box<dyn Error>> {
    let mesh = mesh(4);
    let fields = fields(&mesh);
    // 节点 5 的矢量缺失：所有平面上，含该节点的外表面三角形顶点必为 NaN。
    let section = section_surface_field(
        &mesh,
        &fields[1],
        FieldComponent::Magnitude,
        [1000.0, 1000.0, 1000.0],
        [0.0, 0.0, 1.0],
        &Progress::new(),
    )?
    .ok_or("cancel")?;
    let target = mesh.nodes[5];
    let mut found = false;
    for (triangle, values) in section.surface.triangles.iter().zip(&section.values) {
        for (point, value) in triangle.iter().zip(values) {
            if *point == target {
                assert!(value.is_nan());
                found = true;
            }
        }
    }
    assert_eq!(found, mesh.boundary.iter().any(|f| f.nodes.contains(&5)));
    Ok(())
}

#[test]
fn field_selection_reads_few_large_ranges() -> Result<(), Box<dyn Error>> {
    let mesh = mesh(7);
    let fields = fields(&mesh);
    let volume = open(build(&mesh, &fields)?)?;
    let plane = SectionPlane::new(PLANES[0].0, PLANES[0].1)?;
    let before = volume.io_counts().reads;
    volume
        .fine_section_field(&plane, 1, FieldComponent::Magnitude, &|| false)?
        .ok_or("cancel")?;
    let first = volume.io_counts().reads - before;
    // 外表面值 1 次 + 单元合并读 + 场值合并读（小文件各合并为 1 次）。
    assert_eq!(first, 3);
    let before = volume.io_counts().reads;
    volume
        .fine_section_field(&plane, 1, FieldComponent::Component(1), &|| false)?
        .ok_or("cancel")?;
    // 同一场：外表面值常驻，单元在缓存里，只剩命中块的场值一次合并读。
    assert_eq!(volume.io_counts().reads - before, 1);
    Ok(())
}

#[test]
fn corrupt_field_blocks_only_fail_field_sections() -> Result<(), Box<dyn Error>> {
    let mesh = mesh(6);
    let fields = fields(&mesh);
    let bytes = build(&mesh, &fields)?;
    let probe = open(bytes.clone())?;
    let plane = SectionPlane::new(PLANES[1].0, PLANES[1].1)?;
    let hit = probe.hit_chunks(&plane)[0];
    let block = probe.headers()[hit].field_blocks[0];
    let mut corrupt = bytes;
    corrupt[block.values_offset as usize + block.values_len as usize / 2] ^= 0xff;
    let volume = open(corrupt)?;
    assert!(volume.fine_section(&plane, &|| false)?.is_some());
    assert!(
        volume
            .fine_section_field(&plane, 0, FieldComponent::Magnitude, &|| false)
            .is_err()
    );
    assert!(
        volume
            .fine_section_field(&plane, 2, FieldComponent::Magnitude, &|| false)?
            .is_some()
    );
    Ok(())
}

#[test]
fn invalid_field_requests_and_inputs_are_rejected() -> Result<(), Box<dyn Error>> {
    let mesh = mesh(3);
    let mut fields = fields(&mesh);
    let volume = open(build(&mesh, &fields)?)?;
    let plane = SectionPlane::new(PLANES[0].0, PLANES[0].1)?;
    for (field, component) in [
        (3, FieldComponent::Magnitude),
        (0, FieldComponent::Component(1)),
        (1, FieldComponent::Component(3)),
    ] {
        assert!(matches!(
            volume.fine_section_field(&plane, field, component, &|| false),
            Err(ChunkError::Invalid(_))
        ));
    }
    assert!(
        section_surface_field(
            &mesh,
            &fields[0],
            FieldComponent::Component(1),
            [0.0; 3],
            [0.0, 0.0, 1.0],
            &Progress::new()
        )
        .is_err()
    );
    fields[2].values.pop();
    assert!(build(&mesh, &fields).is_err());
    fields[2].values.push(0.0);
    fields[2].components = 2;
    assert!(build(&mesh, &fields).is_err());
    Ok(())
}

#[test]
fn no_field_files_keep_empty_field_tables() -> Result<(), Box<dyn Error>> {
    let mesh = mesh(3);
    let volume = open(build(&mesh, &[])?)?;
    assert!(volume.fields().is_empty());
    assert!(volume.headers().iter().all(|h| h.field_ranges.is_empty()));
    Ok(())
}

#[test]
fn build_from_asset_with_result_reader_matches_resident_fields() -> Result<(), Box<dyn Error>> {
    let block = SyntheticTetBlock::with_cells_per_axis(4);
    let dir = std::env::temp_dir().join(format!("panta-chunk-fields-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let asset_path = dir.join("volume.pmesh");
    crate::write_volume_asset_streaming(
        std::fs::File::create(&asset_path)?,
        block.counts(),
        &block,
        None,
    )?
    .ok_or("cancelled")?;
    let mut asset = crate::VolumeAssetChunks::open(&asset_path)?;
    let resident = mesh(4);
    let fields = fields(&resident);
    let (bytes, _) = crate::encode_result_asset(&crate::ResultSet {
        mesh_digest: asset.digest().to_owned(),
        node_count: resident.nodes.len() as u64,
        cell_count: resident.tets.len() as u64,
        fields: fields.clone(),
    })?;
    let result_path = dir.join("volume.presult");
    std::fs::write(&result_path, bytes)?;
    let mut reader = crate::ResultAssetReader::open(&result_path)?;
    let mut from_asset = Vec::new();
    super::build_chunked_from_asset_with_fields(
        &mut asset,
        &mut reader,
        ChunkBuildOptions::default(),
        &mut from_asset,
        &Progress::new(),
    )?
    .ok_or("cancelled")?;
    let mut from_mesh = Vec::new();
    build_chunked_volume_with_fields(
        &resident,
        &mut ResidentFields(&fields),
        ChunkBuildOptions::default(),
        &mut from_mesh,
        &Progress::new(),
    )?
    .ok_or("cancelled")?;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(from_asset, from_mesh);
    Ok(())
}
