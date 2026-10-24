use std::error::Error;
use std::sync::Arc;

use super::format::{ChunkBytes, ChunkSource, IoCounts, TAIL_LEN};
use super::{
    ChunkBuildOptions, ChunkError, ChunkedVolume, SectionPlane, TetSource, build_chunked_volume,
};
use crate::{Progress, SyntheticTetBlock, TetMesh, section_surface, tet_quality};

/// 内存块源，便于测试 IO 计数与损坏数据。
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

fn synthetic_mesh(n: usize) -> TetMesh {
    let block = SyntheticTetBlock::with_cells_per_axis(n);
    TetMesh {
        nodes: (0..block.node_count()).map(|i| block.node(i)).collect(),
        tets: (0..block.tet_count()).map(|i| block.tet(i)).collect(),
        boundary: (0..block.boundary_count())
            .map(|i| block.boundary_face(i))
            .collect(),
        region_count: 1,
        boundary_group_count: 6,
        cells: None,
    }
}

fn build(source: &dyn TetSource, max: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut bytes = Vec::new();
    let options = ChunkBuildOptions {
        max_chunk_cells: max,
        simplify_ratio: 4,
    };
    build_chunked_volume(source, options, &mut bytes, &Progress::new())?.ok_or("cancelled")?;
    Ok(bytes)
}

fn open(bytes: Vec<u8>, budget: usize) -> Result<ChunkedVolume, ChunkError> {
    ChunkedVolume::open(
        Arc::new(MemorySource {
            data: bytes,
            reads: Default::default(),
        }),
        budget,
    )
}

fn area(triangles: &[[[f64; 3]; 3]]) -> f64 {
    triangles
        .iter()
        .map(|[a, b, c]| {
            let u = crate::volume::sub(*b, *a);
            let v = crate::volume::sub(*c, *a);
            let n = crate::volume::cross(u, v);
            crate::volume::dot(n, n).sqrt() / 2.0
        })
        .sum()
}

fn sorted_keys(triangles: &[[[f64; 3]; 3]]) -> Vec<[u64; 9]> {
    let mut keys: Vec<[u64; 9]> = triangles
        .iter()
        .map(|t| std::array::from_fn(|i| t[i / 3][i % 3].to_bits()))
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn chunks_preserve_order_ids_and_cover_every_cell() -> Result<(), Box<dyn Error>> {
    let mesh = synthetic_mesh(6);
    let volume = open(build(&mesh, 150)?, usize::MAX)?;
    assert!(volume.headers().len() >= 8);
    assert_eq!(volume.tet_count(), mesh.tets.len() as u64);
    let ids: Vec<usize> = (0..volume.headers().len()).collect();
    let chunks = volume.cache().get_many(&ids)?;
    let mut seen = vec![false; mesh.tets.len()];
    for cells in &chunks {
        assert!(cells.global_cells.windows(2).all(|pair| pair[0] < pair[1]));
        for (tet, &global) in cells.tets.iter().zip(&cells.global_cells) {
            let original = mesh.tets[global as usize];
            let nodes = tet
                .nodes
                .map(|node| cells.global_nodes[node as usize] as u32);
            assert_eq!(nodes, original.nodes);
            assert_eq!(
                tet.nodes.map(|n| cells.nodes[n as usize]),
                original.nodes.map(|n| mesh.nodes[n as usize])
            );
            seen[global as usize] = true;
        }
    }
    assert!(seen.iter().all(|&value| value));
    Ok(())
}

#[test]
fn fine_section_matches_resident_section_exactly() -> Result<(), Box<dyn Error>> {
    let mesh = synthetic_mesh(7);
    let volume = open(build(&mesh, 200)?, usize::MAX)?;
    for (origin, normal) in [
        ([50.0, 50.0, 50.0], [0.3, 0.2, 1.0]),
        ([10.0, 70.0, 20.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, 0.0], [-1.0, -1.0, -1.0]),
    ] {
        let expected = section_surface(&mesh, origin, normal, &Progress::new())?.ok_or("cancel")?;
        let plane = SectionPlane::new(origin, normal)?;
        let actual = volume.fine_section(&plane, &|| false)?.ok_or("cancel")?;
        assert_eq!(
            sorted_keys(&actual.triangles),
            sorted_keys(&expected.triangles)
        );
        // 第二次走结果缓存，结果不变。
        let again = volume.fine_section(&plane, &|| false)?.ok_or("cancel")?;
        assert_eq!(again.triangles.len(), actual.triangles.len());
        let coarse = volume.coarse_section(&plane);
        assert!(!coarse.triangles.is_empty());
        let (fine_area, coarse_area) = (area(&actual.triangles), area(&coarse.triangles));
        assert!(
            coarse_area > 0.3 * fine_area,
            "{coarse_area} vs {fine_area}"
        );
    }
    Ok(())
}

#[test]
fn quality_matches_resident_statistics() -> Result<(), Box<dyn Error>> {
    let mesh = synthetic_mesh(5);
    let volume = open(build(&mesh, 100)?, 64 << 10)?;
    let expected = tet_quality(&mesh, &Progress::new()).ok_or("cancel")?;
    let actual = volume.quality(&|| false)?.ok_or("cancel")?;
    assert_eq!(actual.tet_count, expected.tet_count);
    assert_eq!(actual.poor_count, expected.poor_count);
    assert_eq!(actual.min_dihedral_degrees, expected.min_dihedral_degrees);
    assert_eq!(actual.max_aspect_ratio, expected.max_aspect_ratio);
    assert!((actual.mean_aspect_ratio - expected.mean_aspect_ratio).abs() < 1e-9);
    Ok(())
}

#[test]
fn cache_respects_budget_and_coalesces_reads() -> Result<(), Box<dyn Error>> {
    let block = SyntheticTetBlock::with_cells_per_axis(8);
    let bytes = build(&block, 300)?;
    let volume = open(bytes.clone(), usize::MAX)?;
    let opened = volume.io_counts().reads;
    assert!(opened <= 2, "open used {opened} reads");
    let ids: Vec<usize> = (0..volume.headers().len()).collect();
    volume.cache().get_many(&ids)?;
    // 所有块在单元段内连续，一次合并读即可。
    assert_eq!(volume.io_counts().reads - opened, 1);

    let small = open(bytes, 1)?;
    let cells = small.cache().get_many(&[0, 1, 2])?;
    assert_eq!(cells.len(), 3);
    let (resident, count, _, _) = small.cache().stats();
    assert!(count <= 1 && (count == 0 || resident == cells[2].resident_bytes() || resident > 0));
    small.cache().set_budget(0);
    assert_eq!(small.cache().stats().1, 0);
    Ok(())
}

#[test]
fn prefetch_warms_cache_without_blocking() -> Result<(), Box<dyn Error>> {
    let block = SyntheticTetBlock::with_cells_per_axis(6);
    let volume = open(build(&block, 200)?, usize::MAX)?;
    let prefetcher = super::Prefetcher::new(volume.cache().clone());
    prefetcher.request(vec![0, 1]);
    for _ in 0..200 {
        if volume.cache().contains(0) && volume.cache().contains(1) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(volume.cache().contains(0) && volume.cache().contains(1));
    drop(prefetcher);
    Ok(())
}

#[test]
fn corrupt_or_truncated_files_are_rejected() -> Result<(), Box<dyn Error>> {
    let block = SyntheticTetBlock::with_cells_per_axis(4);
    let bytes = build(&block, 100)?;
    let mut bad_magic = bytes.clone();
    let end = bad_magic.len();
    bad_magic[end - 1] ^= 0xff;
    assert!(matches!(
        open(bad_magic, 1 << 20),
        Err(ChunkError::Format(_))
    ));
    assert!(
        open(
            bytes[..bytes.len() - TAIL_LEN as usize - 3].to_vec(),
            1 << 20
        )
        .is_err()
    );
    let mut flipped = bytes.clone();
    flipped[40] ^= 0x55;
    let volume = open(flipped, 1 << 20)?;
    let ids: Vec<usize> = (0..volume.headers().len()).collect();
    assert!(volume.cache().get_many(&ids).is_err());
    assert!(matches!(
        volume.cache().get_many(&[9999]),
        Err(ChunkError::Invalid(_))
    ));
    Ok(())
}

#[test]
fn invalid_inputs_are_rejected() -> Result<(), Box<dyn Error>> {
    assert!(SectionPlane::new([0.0; 3], [0.0; 3]).is_err());
    let block = SyntheticTetBlock::with_cells_per_axis(2);
    let options = ChunkBuildOptions {
        max_chunk_cells: 0,
        simplify_ratio: 1,
    };
    assert!(build_chunked_volume(&block, options, Vec::new(), &Progress::new()).is_err());
    let progress = Progress::new();
    progress.cancel();
    let result = build_chunked_volume(&block, ChunkBuildOptions::default(), Vec::new(), &progress)?;
    assert!(result.is_none());
    Ok(())
}

/// 测试用临时文件，析构时删除。
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn new(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let path =
            std::env::temp_dir().join(format!("panta-chunk-{}-{name}.pchunk", std::process::id()));
        std::fs::write(&path, bytes)?;
        Ok(Self(path))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn mmap_and_positioned_reads_give_identical_results() -> Result<(), Box<dyn Error>> {
    use super::ChunkReadMode;
    let block = SyntheticTetBlock::with_cells_per_axis(8);
    let file = TempFile::new("modes", &build(&block, 200)?)?;
    let mut results = Vec::new();
    for (mode, kind) in [
        (ChunkReadMode::Mmap, "mmap"),
        (ChunkReadMode::Read, "pread"),
        (ChunkReadMode::Auto, "mmap"),
    ] {
        let volume = ChunkedVolume::open_file_with(&file.0, 1 << 20, mode)?;
        assert_eq!(volume.reader_kind(), kind);
        let [low, high] = volume.bounds();
        let centre = std::array::from_fn(|axis| 0.43 * low[axis] + 0.57 * high[axis]);
        let plane = SectionPlane::new(centre, [1.0, 0.3, 0.1])?;
        let section = volume.fine_section(&plane, &|| false)?.ok_or("cancelled")?;
        let quality = volume.quality(&|| false)?.ok_or("cancelled")?;
        let counts = volume.io_counts();
        assert_eq!(counts.files, 1);
        if kind == "mmap" {
            assert_eq!(counts.reads, 0);
            assert!(counts.mapped > 0);
        } else {
            assert!(counts.reads > 0);
            assert_eq!(counts.mapped, 0);
        }
        results.push((sorted_keys(&section.triangles), quality.tet_count));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
    Ok(())
}

#[test]
fn mapped_files_validate_head_tail_and_chunks() -> Result<(), Box<dyn Error>> {
    use super::ChunkReadMode;
    let block = SyntheticTetBlock::with_cells_per_axis(4);
    let bytes = build(&block, 100)?;
    let mut bad_head = bytes.clone();
    bad_head[0] ^= 0xff;
    let truncated = &bytes[..bytes.len() - 5];
    let mut flipped = bytes.clone();
    flipped[40] ^= 0x55;
    let empty = TempFile::new("empty", &[])?;
    let bad_head = TempFile::new("head", &bad_head)?;
    let truncated = TempFile::new("truncated", truncated)?;
    let flipped = TempFile::new("flipped", &flipped)?;
    for mode in [ChunkReadMode::Mmap, ChunkReadMode::Read] {
        assert!(ChunkedVolume::open_file_with(&empty.0, 1 << 20, mode).is_err());
        assert!(matches!(
            ChunkedVolume::open_file_with(&bad_head.0, 1 << 20, mode),
            Err(ChunkError::Format(_))
        ));
        assert!(ChunkedVolume::open_file_with(&truncated.0, 1 << 20, mode).is_err());
        let volume = ChunkedVolume::open_file_with(&flipped.0, 1 << 20, mode)?;
        let ids: Vec<usize> = (0..volume.headers().len()).collect();
        assert!(volume.cache().get_many(&ids).is_err());
    }
    Ok(())
}

#[test]
fn exclusive_lock_forces_positioned_read_fallback() -> Result<(), Box<dyn Error>> {
    use super::ChunkReadMode;
    let block = SyntheticTetBlock::with_cells_per_axis(4);
    let file = TempFile::new("locked", &build(&block, 100)?)?;
    // 另一句柄持有排他锁（模拟写入方）：映射拒绝打开。Unix 建议锁不拦读，自动模式回退定位读；
    // Windows 排他锁是强制锁，定位读也被拒（ERROR_LOCK_VIOLATION），打开以 IO 错误失败而不是读到半写数据。
    let writer = std::fs::OpenOptions::new().read(true).open(&file.0)?;
    writer.lock()?;
    assert!(ChunkedVolume::open_file_with(&file.0, 1 << 20, ChunkReadMode::Mmap).is_err());
    let fallback = ChunkedVolume::open_file_with(&file.0, 1 << 20, ChunkReadMode::Auto);
    if cfg!(windows) {
        assert!(matches!(fallback, Err(ChunkError::Io(_))));
    } else {
        assert_eq!(fallback?.reader_kind(), "pread");
    }
    writer.unlock()?;
    drop(writer);
    let volume = ChunkedVolume::open_file_with(&file.0, 1 << 20, ChunkReadMode::Auto)?;
    assert_eq!(volume.reader_kind(), "mmap");
    Ok(())
}

#[test]
fn plane_through_nodes_matches_resident_section_position() -> Result<(), Box<dyn Error>> {
    let mesh = synthetic_mesh(6);
    let volume = open(build(&mesh, 120)?, usize::MAX)?;
    for normal in [[0.3, 0.2, 1.0], [1.0, 0.0, 0.0], [-1.0, -2.0, 0.5]] {
        for fraction in [0.0, 0.37, 0.5, 1.0, 1.5] {
            let plane = volume.plane_through_nodes(normal, fraction)?;
            let length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
            let unit = normal.map(|value| value / length);
            let origin = crate::section_plane_origin(&mesh.nodes, unit, fraction);
            assert_eq!(plane.normal, unit);
            assert_eq!(plane.origin, origin, "{normal:?} {fraction}");
            let expected =
                section_surface(&mesh, origin, unit, &Progress::new())?.ok_or("cancel")?;
            let actual = volume.fine_section(&plane, &|| false)?.ok_or("cancel")?;
            assert_eq!(
                sorted_keys(&actual.triangles),
                sorted_keys(&expected.triangles)
            );
        }
    }
    assert!(volume.plane_through_nodes([0.0; 3], 0.5).is_err());
    assert!(
        volume
            .plane_through_nodes([0.0, 0.0, 1.0], f64::NAN)
            .is_err()
    );
    Ok(())
}

#[test]
fn build_from_v3_asset_matches_build_from_resident_mesh() -> Result<(), Box<dyn Error>> {
    let block = SyntheticTetBlock::with_cells_per_axis(5);
    let path = std::env::temp_dir().join(format!("panta-chunk-{}-asset.pmesh", std::process::id()));
    let asset = TempFile(path.clone());
    crate::write_volume_asset_streaming(
        std::fs::File::create(&asset.0)?,
        block.counts(),
        &block,
        None,
    )?
    .ok_or("cancelled")?;
    let mut chunks = crate::VolumeAssetChunks::open(&asset.0)?;
    let mut from_asset = Vec::new();
    let progress = Progress::new();
    super::build_chunked_from_asset(
        &mut chunks,
        ChunkBuildOptions::default(),
        &mut from_asset,
        &progress,
    )?
    .ok_or("cancelled")?;
    assert!((progress.fraction() - 1.0).abs() < 1e-9);
    let mut from_mesh = Vec::new();
    build_chunked_volume(
        &synthetic_mesh(5),
        ChunkBuildOptions::default(),
        &mut from_mesh,
        &Progress::new(),
    )?
    .ok_or("cancelled")?;
    assert_eq!(from_asset, from_mesh);
    let cancelled = Progress::new();
    cancelled.cancel();
    let mut chunks = crate::VolumeAssetChunks::open(&asset.0)?;
    assert!(
        super::build_chunked_from_asset(
            &mut chunks,
            ChunkBuildOptions::default(),
            Vec::new(),
            &cancelled
        )?
        .is_none()
    );
    Ok(())
}
