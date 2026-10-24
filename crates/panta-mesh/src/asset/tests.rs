use super::{
    CODEC_NONE, EncodedChunk, HEADER_SIZE, MeshAsset, MeshAssetError, MeshAssetReader, Stream,
    TOC_ENTRY_BYTES, TOC_PREFIX_BYTES, assemble, decode_mesh_asset, decode_mesh_asset_with_digest,
    encode_mesh_asset, mesh_asset_encoded_len, surface_mesh_expanded_bytes,
};
use crate::{SurfaceMesh, SurfaceTriangle, TetMesh, Tetrahedron};
use sha2::{Digest, Sha256};

type EntryEdit = fn(&mut [u8]);
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn surface(count: usize) -> MeshAsset {
    let triangles = (0..count)
        .map(|index| {
            let x = index as f64 * 0.25;
            [[x, 0.0, 1.0], [x + 0.5, 1.0, 1.0], [x, 2.0, -3.5]]
        })
        .collect();
    MeshAsset::Surface(SurfaceMesh { triangles })
}

fn tet_mesh() -> TetMesh {
    TetMesh {
        nodes: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ],
        tets: vec![Tetrahedron {
            nodes: [0, 1, 2, 3],
            region: 0,
        }],
        boundary: vec![SurfaceTriangle {
            nodes: [0, 1, 2],
            group: 0,
        }],
        region_count: 1,
        boundary_group_count: 1,
        cells: None,
    }
}

fn raw_chunk(stream: Stream, record_count: usize, raw: Vec<u8>) -> EncodedChunk {
    let digest = Sha256::digest(&raw).into();
    EncodedChunk {
        stream,
        codec: CODEC_NONE,
        record_count,
        stored: raw,
        digest,
    }
}

fn chunk_count(bytes: &[u8]) -> usize {
    super::read_u64_at(bytes, HEADER_SIZE) as usize
}

/// 改写第 `index` 个目录条目后重算目录摘要，模拟“摘要正确但语义不受支持”的文件。
fn edit_entry(bytes: &mut [u8], index: usize, edit: impl FnOnce(&mut [u8])) {
    let start = HEADER_SIZE + TOC_PREFIX_BYTES + index * TOC_ENTRY_BYTES;
    edit(&mut bytes[start..start + TOC_ENTRY_BYTES]);
    let toc_end = HEADER_SIZE + TOC_PREFIX_BYTES + chunk_count(bytes) * TOC_ENTRY_BYTES;
    let digest = Sha256::digest(&bytes[HEADER_SIZE..toc_end]);
    bytes[24..56].copy_from_slice(&digest);
}

fn read_streaming(bytes: &[u8]) -> Result<Option<(MeshAsset, String)>, MeshAssetError> {
    MeshAssetReader::open(bytes, bytes.len() as u64)?.read(|| false)
}

#[test]
fn expanded_surface_size_counts_header_count_and_records() {
    assert_eq!(surface_mesh_expanded_bytes(2), 56 + 8 + 2 * 72);
    assert_eq!(surface_mesh_expanded_bytes(0), 64);
    assert_eq!(surface_mesh_expanded_bytes(u64::MAX), u64::MAX);
}

#[test]
fn surface_round_trips_and_digest_covers_the_table_of_contents() -> TestResult {
    let mesh = surface(3);
    let (mut bytes, digest) = encode_mesh_asset(&mesh)?;
    let toc_end = HEADER_SIZE + TOC_PREFIX_BYTES + TOC_ENTRY_BYTES;
    assert_eq!(
        digest,
        format!("sha256:{:x}", Sha256::digest(&bytes[HEADER_SIZE..toc_end]))
    );
    assert_eq!(
        decode_mesh_asset_with_digest(&bytes)?,
        (mesh.clone(), digest.clone())
    );
    assert_eq!(read_streaming(&bytes)?, Some((mesh, digest)));

    *bytes.last_mut().ok_or("empty")? ^= 1;
    assert_eq!(
        decode_mesh_asset(&bytes),
        Err(MeshAssetError::ChecksumMismatch)
    );
    assert_eq!(
        read_streaming(&bytes),
        Err(MeshAssetError::ChecksumMismatch)
    );
    bytes[HEADER_SIZE + 9] ^= 1;
    assert_eq!(
        decode_mesh_asset(&bytes),
        Err(MeshAssetError::ChecksumMismatch)
    );
    Ok(())
}

#[test]
fn large_surface_spans_aligned_chunks_and_decodes_in_parallel() -> TestResult {
    let count = Stream::SurfaceTriangles.records_per_chunk() * 5 + 17;
    let mesh = surface(count);
    let (bytes, digest) = encode_mesh_asset(&mesh)?;
    assert_eq!(chunk_count(&bytes), 6);
    for index in 0..6 {
        let entry = HEADER_SIZE + TOC_PREFIX_BYTES + index * TOC_ENTRY_BYTES;
        assert_eq!(super::read_u64_at(&bytes, entry + 8) % 64, 0);
    }
    // 规则数据压缩后应远小于展开体积。
    assert!((bytes.len() as u64) < surface_mesh_expanded_bytes(count as u64) / 4);
    assert_eq!(decode_mesh_asset(&bytes)?, mesh);
    assert_eq!(read_streaming(&bytes)?, Some((mesh, digest)));
    Ok(())
}

#[test]
fn streaming_reader_honours_cancellation_and_rejects_length_mismatch() -> TestResult {
    let (bytes, _) = encode_mesh_asset(&surface(4))?;
    let reader = MeshAssetReader::open(bytes.as_slice(), bytes.len() as u64)?;
    assert!(reader.digest().is_some());
    assert_eq!(reader.read(|| true)?, None);
    assert!(matches!(
        MeshAssetReader::open(bytes.as_slice(), bytes.len() as u64 + 1),
        Err(MeshAssetError::InvalidLength)
    ));
    let mut grown = bytes.clone();
    grown.push(0);
    assert_eq!(
        MeshAssetReader::open(grown.as_slice(), bytes.len() as u64)?.read(|| false),
        Err(MeshAssetError::InvalidLength)
    );
    Ok(())
}

#[test]
fn volume_round_trips_and_rejects_invalid_connectivity() -> TestResult {
    let mesh = MeshAsset::Volume(tet_mesh());
    let (bytes, digest) = encode_mesh_asset(&mesh)?;
    assert_eq!(decode_mesh_asset(&bytes)?, mesh);
    assert_eq!(read_streaming(&bytes)?, Some((mesh, digest)));

    let valid = tet_mesh();
    let mut counts = valid.region_count.to_le_bytes().to_vec();
    counts.extend_from_slice(&valid.boundary_group_count.to_le_bytes());
    let mut nodes = Vec::new();
    for coordinate in valid.nodes.iter().flatten() {
        nodes.extend_from_slice(&coordinate.to_le_bytes());
    }
    let mut tets = Vec::new();
    for value in [0_u32, 1, 2, 99, 0] {
        tets.extend_from_slice(&value.to_le_bytes());
    }
    let (corrupted, _) = assemble(
        super::TET_KIND,
        vec![
            raw_chunk(Stream::VolumeCounts, 1, counts),
            raw_chunk(Stream::VolumeNodes, 4, nodes),
            raw_chunk(Stream::VolumeTets, 1, tets),
        ],
    )?;
    assert_eq!(
        decode_mesh_asset(&corrupted),
        Err(MeshAssetError::InvalidMesh)
    );
    Ok(())
}

#[test]
fn table_of_contents_rejects_out_of_contract_entries() -> TestResult {
    let (original, _) = encode_mesh_asset(&surface(2))?;
    let cases: [(EntryEdit, MeshAssetError); 7] = [
        (|entry| entry[2] = 4, MeshAssetError::UnsupportedLayout),
        (|entry| entry[3] = 1, MeshAssetError::UnsupportedLayout),
        (|entry| entry[4] = 1, MeshAssetError::UnsupportedLayout),
        (|entry| entry[0] = 9, MeshAssetError::UnsupportedLayout),
        (|entry| entry[0] = 3, MeshAssetError::UnsupportedLayout),
        (|entry| entry[1] = 7, MeshAssetError::UnsupportedCodec(7)),
        (|entry| entry[24] = 3, MeshAssetError::InvalidLength),
    ];
    for (edit, expected) in cases {
        let mut bytes = original.clone();
        edit_entry(&mut bytes, 0, edit);
        assert_eq!(decode_mesh_asset(&bytes), Err(expected));
        assert_eq!(read_streaming(&bytes), Err(expected));
    }
    Ok(())
}

#[test]
fn record_count_must_match_decompressed_chunk_exactly() -> TestResult {
    let mut raw = Vec::new();
    for _ in 0..9 {
        raw.extend_from_slice(&1.0_f64.to_le_bytes());
    }
    let mut chunk = raw_chunk(Stream::SurfaceTriangles, 1, raw);
    chunk.stored = zstd::bulk::compress(&chunk.stored, 3)?;
    chunk.codec = super::CODEC_ZSTD;
    chunk.record_count = 2;
    chunk.digest = Sha256::digest(&chunk.stored).into();
    let (bytes, _) = assemble(super::SURFACE_KIND, vec![chunk])?;
    assert!(decode_mesh_asset(&bytes).is_err());
    Ok(())
}

#[test]
fn non_finite_coordinates_are_rejected_on_decode() -> TestResult {
    let mut raw = Vec::new();
    for value in [f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    let (bytes, _) = assemble(
        super::SURFACE_KIND,
        vec![raw_chunk(Stream::SurfaceTriangles, 1, raw)],
    )?;
    assert_eq!(decode_mesh_asset(&bytes), Err(MeshAssetError::InvalidMesh));
    Ok(())
}

#[test]
fn unsupported_header_fields_are_reported() -> TestResult {
    let (original, _) = encode_mesh_asset(&surface(1))?;
    let mut bytes = original.clone();
    bytes[8..12].copy_from_slice(&99_u32.to_le_bytes());
    assert_eq!(
        decode_mesh_asset(&bytes),
        Err(MeshAssetError::UnsupportedVersion(99))
    );
    let mut bytes = original.clone();
    bytes[14] = 1;
    assert_eq!(
        decode_mesh_asset(&bytes),
        Err(MeshAssetError::InvalidHeader)
    );
    let mut bytes = original;
    bytes.pop();
    assert_eq!(
        decode_mesh_asset(&bytes),
        Err(MeshAssetError::InvalidLength)
    );
    Ok(())
}

#[test]
fn asset_larger_than_two_gib_is_not_rejected_by_format_policy() -> TestResult {
    let (mut bytes, _) = encode_mesh_asset(&surface(1))?;
    bytes[16..24].copy_from_slice(&(2_u64 * 1024 * 1024 * 1024).to_le_bytes());
    assert!(mesh_asset_encoded_len(&bytes[..HEADER_SIZE]).is_ok());
    assert_eq!(
        decode_mesh_asset(&bytes),
        Err(MeshAssetError::InvalidLength)
    );
    Ok(())
}

// COMPAT(task 011; remove-task 012): v2 旧 sidecar 只读回归，随 v2 读取一起删除。
#[test]
fn legacy_v2_sidecars_still_decode_with_their_original_digest() -> TestResult {
    let surface_bytes = include_bytes!("../../tests/data/surface-v2.pmesh");
    let expected_surface = MeshAsset::Surface(SurfaceMesh {
        triangles: vec![
            [[0.0, 0.0, 0.0], [1.5, 0.0, 0.0], [0.0, 2.25, -3.0]],
            [[1.0, 1.0, 1.0], [2.0, 1.0, 1.0], [1.0, 2.0, 1.0]],
        ],
    });
    let surface_digest =
        "sha256:217761895c321aab5531d3789be1e21dd2c8b47401adb076778f746f4019d237".to_owned();
    assert_eq!(
        decode_mesh_asset_with_digest(surface_bytes)?,
        (expected_surface.clone(), surface_digest.clone())
    );
    assert_eq!(
        read_streaming(surface_bytes)?,
        Some((expected_surface, surface_digest))
    );

    let volume_bytes = include_bytes!("../../tests/data/volume-v2.pmesh");
    assert_eq!(
        decode_mesh_asset_with_digest(volume_bytes)?,
        (
            MeshAsset::Volume(tet_mesh()),
            "sha256:70e946f8fffb8bfbbf0b159c9baf844503aa8e36996b01f21f42cbae17f7e98d".to_owned()
        )
    );

    let mut corrupted = surface_bytes.to_vec();
    *corrupted.last_mut().ok_or("empty")? ^= 1;
    assert_eq!(
        decode_mesh_asset(&corrupted),
        Err(MeshAssetError::ChecksumMismatch)
    );
    corrupted[14] = 9;
    assert_eq!(
        decode_mesh_asset(&corrupted),
        Err(MeshAssetError::UnsupportedCodec(9))
    );
    Ok(())
}

mod volume_streaming {
    use super::TestResult;
    use crate::{
        MeshAsset, MeshAssetReader, Progress, SyntheticTetBlock, TetMesh, VolumeAssetChunks,
        boundary_surface, boundary_surface_streaming, section_plane_origin,
        section_plane_streaming, section_surface, tet_quality, tet_quality_streaming,
        write_volume_asset_streaming,
    };

    struct Asset(std::path::PathBuf);

    impl Drop for Asset {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn write_block(n: usize, name: &str) -> Result<(Asset, TetMesh), Box<dyn std::error::Error>> {
        let block = SyntheticTetBlock::with_cells_per_axis(n);
        let path = std::env::temp_dir().join(format!(
            "panta-volume-stream-{name}-{}.pmesh",
            std::process::id()
        ));
        let asset = Asset(path.clone());
        let file = std::fs::File::create(&path)?;
        write_volume_asset_streaming(file, block.counts(), &block, None)?.ok_or("cancelled")?;
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
        Ok((asset, mesh))
    }

    #[test]
    fn streamed_synthetic_volume_passes_full_validation_and_round_trips() -> TestResult {
        let (asset, expected) = write_block(4, "roundtrip")?;
        let len = std::fs::metadata(&asset.0)?.len();
        let reader = MeshAssetReader::open(std::fs::File::open(&asset.0)?, len)?;
        let (decoded, _) = reader.read(|| false)?.ok_or("cancelled")?;
        // 完整读取执行 validate_tet_mesh：正向、无重复单元、边界面与单元面一致。
        assert_eq!(decoded, MeshAsset::Volume(expected.clone()));
        let chunks = VolumeAssetChunks::open(&asset.0)?;
        assert_eq!(chunks.counts().tets, 6 * 64);
        assert_eq!(chunks.counts().nodes, 125);
        assert_eq!(chunks.counts().boundary, 12 * 16);
        Ok(())
    }

    #[test]
    fn streaming_boundary_quality_and_section_match_in_memory_results() -> TestResult {
        let (asset, mesh) = write_block(5, "ops")?;
        let mut chunks = VolumeAssetChunks::open(&asset.0)?;
        let surface = boundary_surface_streaming(&mut chunks, None)?.ok_or("cancelled")?;
        assert_eq!(surface, boundary_surface(&mesh));

        let progress = Progress::new();
        let streamed = tet_quality_streaming(&mut chunks, &progress)?.ok_or("cancelled")?;
        let resident = tet_quality(&mesh, &Progress::new()).ok_or("cancelled")?;
        assert_eq!(streamed.tet_count, resident.tet_count);
        assert_eq!(streamed.poor_count, resident.poor_count);
        assert_eq!(streamed.min_dihedral_degrees, resident.min_dihedral_degrees);
        assert_eq!(streamed.max_aspect_ratio, resident.max_aspect_ratio);
        assert!((streamed.mean_aspect_ratio - resident.mean_aspect_ratio).abs() < 1e-12);
        assert_eq!(progress.fraction(), 1.0);

        let normal = [0.3, -0.5, 0.8];
        let length = (0.09_f64 + 0.25 + 0.64).sqrt();
        let unit = normal.map(|value| value / length);
        let origin = section_plane_origin(&mesh.nodes, unit, 0.4);
        let expected =
            section_surface(&mesh, origin, unit, &Progress::new())?.ok_or("cancelled")?;
        let section = section_plane_streaming(&mut chunks, normal, 0.4, &Progress::new())?
            .ok_or("cancelled")?;
        assert_eq!(section, expected);
        assert!(section_plane_streaming(&mut chunks, [0.0; 3], 0.4, &Progress::new()).is_err());

        let cancelled = Progress::new();
        cancelled.cancel();
        assert!(tet_quality_streaming(&mut chunks, &cancelled)?.is_none());
        assert!(section_plane_streaming(&mut chunks, normal, 0.4, &cancelled)?.is_none());
        Ok(())
    }

    #[test]
    fn streaming_volume_writer_rejects_out_of_range_records() {
        struct Broken;
        impl crate::VolumeSource for Broken {
            fn nodes(&self, range: std::ops::Range<usize>, out: &mut Vec<[f64; 3]>) {
                out.extend(range.map(|i| [i as f64, 0.0, 0.0]));
            }
            fn tets(&self, range: std::ops::Range<usize>, out: &mut Vec<crate::Tetrahedron>) {
                out.extend(range.map(|_| crate::Tetrahedron {
                    nodes: [0, 1, 2, 99],
                    region: 0,
                }));
            }
            fn boundary(&self, _: std::ops::Range<usize>, _: &mut Vec<crate::SurfaceTriangle>) {}
        }
        let counts = crate::VolumeStreamCounts {
            nodes: 4,
            tets: 1,
            cells: 1,
            boundary: 0,
            region_count: 1,
            boundary_group_count: 0,
        };
        let result =
            write_volume_asset_streaming(std::io::Cursor::new(Vec::new()), counts, &Broken, None);
        assert_eq!(result, Err(crate::MeshAssetError::InvalidMesh));
    }
}

mod mixed_cells {
    use super::{TestResult, read_streaming, tet_mesh};
    use crate::{
        MeshAsset, MeshAssetError, VolumeAssetChunks, decode_mesh_asset, encode_mesh_asset,
        mixed_tests::mixed_mesh,
    };

    #[test]
    fn mixed_asset_round_trips_as_v4_with_native_cells() -> TestResult {
        let mesh = mixed_mesh();
        let asset = MeshAsset::Volume(mesh.clone());
        let (bytes, digest) = encode_mesh_asset(&asset)?;
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into()?), 4);
        assert_eq!(decode_mesh_asset(&bytes)?, asset);
        assert_eq!(read_streaming(&bytes)?, Some((asset, digest)));

        // v4 专属流不能出现在 v3 头部下。
        let mut v3 = bytes.clone();
        v3[8..12].copy_from_slice(&3_u32.to_le_bytes());
        assert!(matches!(
            decode_mesh_asset(&v3),
            Err(MeshAssetError::UnsupportedLayout | MeshAssetError::InvalidMesh)
        ));
        Ok(())
    }

    #[test]
    fn v3_pure_tet_assets_remain_readable() -> TestResult {
        let asset = MeshAsset::Volume(tet_mesh());
        let (mut bytes, _) = encode_mesh_asset(&asset)?;
        bytes[8..12].copy_from_slice(&3_u32.to_le_bytes());
        assert_eq!(decode_mesh_asset(&bytes)?, asset);
        Ok(())
    }

    #[test]
    fn chunked_reader_decomposes_cells_and_keeps_cell_ids() -> TestResult {
        let mesh = mixed_mesh();
        let (bytes, _) = encode_mesh_asset(&MeshAsset::Volume(mesh.clone()))?;
        let path =
            std::env::temp_dir().join(format!("panta-mixed-chunks-{}.pmesh", std::process::id()));
        std::fs::write(&path, &bytes)?;
        let result = (|| -> TestResult {
            let mut chunks = VolumeAssetChunks::open(&path)?;
            assert!(chunks.is_mixed());
            assert_eq!(chunks.counts().cells, 4);
            assert_eq!(chunks.counts().tets, mesh.tets.len());
            let mut seen = Vec::new();
            chunks.visit_cell_tets(None, |batch| {
                seen.extend(batch.iter().flatten().copied());
                true
            })?;
            let expected: Vec<_> = mesh
                .tets
                .iter()
                .enumerate()
                .map(|(index, tet)| (mesh.tet_cell(index) as u32, *tet))
                .collect();
            assert_eq!(seen, expected);
            let mut plain = Vec::new();
            chunks.visit_tets(None, |batch| {
                plain.extend(batch.iter().flatten().copied());
                true
            })?;
            assert_eq!(plain, mesh.tets);
            Ok(())
        })();
        let _ = std::fs::remove_file(&path);
        result
    }
}
