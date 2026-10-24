//! 混合单元体网格基准（task 030 PR 3）：棱柱边界层 + 金字塔/四面体过渡层 + 六面体核心的合成网格，
//! 测流式写出 v4 混合资产、从资产构建 `.pchunk` v3，以及分块切面、单元场切面、逐类型质量，
//! 并与逐次扫描资产（无派生分块时的回退路径）对比。
//!
//! 用法：`cargo run --release -p panta-mesh --example mixed_bench -- [选项]`
//! - `--cells N`：目标原始单元数（支持 K/M/B 后缀），默认 2M。
//! - `--chunk N`：每块原始单元上限，默认 262144。
//! - `--budget-mib N`：块缓存预算，默认 1024。
//! - `--dir PATH`：文件目录，默认系统临时目录；`--keep` 保留文件。
//! - `--reader auto|mmap|pread`：块读取实现，默认 auto（映射，失败回退定位读）。
//! - `--verify`：完整解码资产为驻留网格，校验分块切面 / 单元场切面 / 质量与驻留算法一致（大网格耗内存）。
//!
//! IO：资产写出与读取用 Linux `/proc/self/io`（syscr / syscw 系统调用数、rchar / wchar 字节）阶段差值；
//! 分块读取用 `ChunkedVolume::io_counts`（读调用、读取字节、映射访问、打开文件）。
//! 峰值内存取 `/proc/self/status` 的 VmHWM（阶段间经 `/proc/self/clear_refs` 重置）。其他平台输出 N/A。
//! 不含渲染；无 GPU 时帧率不测。

use std::time::Instant;

use panta_mesh::chunked::{
    ChunkBuildOptions, ChunkError, ChunkFieldInfo, ChunkReadMode, ChunkedVolume, FieldProvider,
    build_chunked_from_asset_with_fields,
};
use panta_mesh::{
    CellType, FieldAssociation, FieldComponent, MeshAsset, Progress, ResultField,
    SyntheticMixedBlock, VolumeAssetChunks,
};

/// 按需生成的合成结果场：节点标量 z 与原始单元标量 cell_id。
struct SyntheticFields<'a>(&'a SyntheticMixedBlock);

const FIELDS: [(&str, FieldAssociation); 2] = [
    ("z", FieldAssociation::Node),
    ("cell_id", FieldAssociation::Cell),
];

impl FieldProvider for SyntheticFields<'_> {
    fn infos(&self) -> Vec<ChunkFieldInfo> {
        FIELDS
            .iter()
            .map(|&(name, association)| ChunkFieldInfo {
                name: name.to_owned(),
                association,
                components: 1,
            })
            .collect()
    }

    fn load(&mut self, index: usize) -> Result<ResultField, ChunkError> {
        let (name, association) = FIELDS[index];
        let values = match association {
            FieldAssociation::Node => (0..self.0.node_count())
                .map(|i| self.0.node(i)[2])
                .collect(),
            FieldAssociation::Cell => (0..self.0.cell_count()).map(|i| i as f64).collect(),
        };
        Ok(ResultField {
            name: name.to_owned(),
            association,
            components: 1,
            units: String::new(),
            values,
        })
    }
}

fn parse_count(text: &str) -> Result<usize, String> {
    let (digits, factor) = match text.chars().last() {
        Some('K' | 'k') => (&text[..text.len() - 1], 1e3),
        Some('M' | 'm') => (&text[..text.len() - 1], 1e6),
        Some('B' | 'b' | 'G' | 'g') => (&text[..text.len() - 1], 1e9),
        _ => (text, 1.0),
    };
    let value: f64 = digits.parse().map_err(|_| format!("无效数量 {text}"))?;
    Ok((value * factor) as usize)
}

fn status_kib(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with(field))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn reset_peak() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

fn peak_mib() -> String {
    status_kib("VmHWM:").map_or_else(
        || "null".to_owned(),
        |kib| format!("{:.1}", kib as f64 / 1024.0),
    )
}

/// `/proc/self/io` 计数（syscr, rchar, syscw, wchar）。
#[derive(Clone, Copy, Default)]
struct ProcIo {
    syscr: u64,
    rchar: u64,
    syscw: u64,
    wchar: u64,
}

fn proc_io() -> Option<ProcIo> {
    let text = std::fs::read_to_string("/proc/self/io").ok()?;
    let get = |key: &str| -> Option<u64> {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.trim().parse().ok())
    };
    Some(ProcIo {
        syscr: get("syscr:")?,
        rchar: get("rchar:")?,
        syscw: get("syscw:")?,
        wchar: get("wchar:")?,
    })
}

/// 阶段 IO 差值；减去读取 `/proc/self/io` 本身的那一次读调用。
fn io_since(start: Option<ProcIo>) -> Option<ProcIo> {
    let (start, now) = (start?, proc_io()?);
    Some(ProcIo {
        syscr: now.syscr.saturating_sub(start.syscr + 1),
        rchar: now.rchar.saturating_sub(start.rchar),
        syscw: now.syscw.saturating_sub(start.syscw),
        wchar: now.wchar.saturating_sub(start.wchar),
    })
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1048576.0
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut cells = 2_000_000;
    let mut chunk = 1 << 18;
    let mut budget_mib = 1024usize;
    let mut dir = std::env::temp_dir();
    let (mut keep, mut verify) = (false, false);
    let mut reader = ChunkReadMode::Auto;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cells" => cells = parse_count(&args.next().ok_or("--cells 缺少值")?)?,
            "--chunk" => chunk = parse_count(&args.next().ok_or("--chunk 缺少值")?)?,
            "--budget-mib" => budget_mib = parse_count(&args.next().ok_or("--budget-mib 缺少值")?)?,
            "--dir" => dir = args.next().ok_or("--dir 缺少值")?.into(),
            "--keep" => keep = true,
            "--verify" => verify = true,
            "--reader" => {
                reader = match args.next().ok_or("--reader 缺少值")?.as_str() {
                    "auto" => ChunkReadMode::Auto,
                    "mmap" => ChunkReadMode::Mmap,
                    "pread" => ChunkReadMode::Read,
                    other => return Err(format!("未知读取实现 {other}").into()),
                }
            }
            other => return Err(format!("未知参数 {other}").into()),
        }
    }
    let block = SyntheticMixedBlock::new(cells);
    let counts = block.counts();
    let types = block.type_counts();
    println!(
        "合成混合网格 n={} 棱柱层 {}×2 核心层 {}：原始单元 {}（四面体 {} / 六面体 {} / 楔形 {} / 金字塔 {}），拆分四面体 {}，节点 {}，边界三角形 {}，块上限 {chunk}，线程 {}",
        block.cells_per_axis(),
        block.prism_layers(),
        block.core_layers(),
        counts.cells,
        types[0],
        types[1],
        types[2],
        types[3],
        counts.tets,
        counts.nodes,
        counts.boundary,
        std::thread::available_parallelism().map_or(1, usize::from)
    );
    let has_proc_io = proc_io().is_some();
    if !has_proc_io {
        println!(
            "注：本平台无 /proc/self/io，资产写出 / 构建 / 回退扫描的系统调用计数不可用（显示为 0）"
        );
    }
    let stem = format!("synthetic-mixed-{}", counts.cells);
    let asset_path = dir.join(format!("{stem}.pmesh"));
    let chunk_path = dir.join(format!("{stem}.pchunk"));

    // 1. 流式写出混合资产。
    reset_peak();
    let io = proc_io();
    let started = Instant::now();
    let file = std::io::BufWriter::with_capacity(8 << 20, std::fs::File::create(&asset_path)?);
    let (asset_bytes, _) =
        panta_mesh::write_volume_asset_streaming(file, counts, &block, Some(&Progress::new()))?
            .ok_or("写出被取消")?;
    let write_s = started.elapsed().as_secs_f64();
    let write_io = io_since(io).unwrap_or_default();
    let write_peak = peak_mib();
    println!(
        "[资产写出] {write_s:.2}s，{:.1} MiB，写调用 {}（{:.1} MiB），打开文件 1，峰值 RSS {write_peak} MiB",
        mib(asset_bytes),
        write_io.syscw,
        mib(write_io.wchar)
    );

    // 2. 从资产构建 .pchunk（带节点场与单元场）。
    reset_peak();
    let io = proc_io();
    let started = Instant::now();
    let mut asset = VolumeAssetChunks::open(&asset_path)?;
    let sink = std::fs::File::create(&chunk_path)?;
    let summary = build_chunked_from_asset_with_fields(
        &mut asset,
        &mut SyntheticFields(&block),
        ChunkBuildOptions {
            max_chunk_cells: chunk,
            ..ChunkBuildOptions::default()
        },
        &sink,
        &Progress::new(),
    )?
    .ok_or("构建被取消")?;
    sink.sync_all()?;
    drop(sink);
    drop(asset);
    let build_s = started.elapsed().as_secs_f64();
    let build_io = io_since(io).unwrap_or_default();
    let build_peak = peak_mib();
    println!(
        "[分块构建] {build_s:.2}s，{} 块，{:.1} MiB；资产读调用 {}（{:.1} MiB，打开资产 1 次），写调用 {}，峰值 RSS {build_peak} MiB",
        summary.chunk_count,
        mib(summary.bytes_written),
        build_io.syscr,
        mib(build_io.rchar),
        summary.write_calls
    );

    // 3. 打开、切面、单元场切面、质量（分块）。
    let started = Instant::now();
    let volume = ChunkedVolume::open_file_with(&chunk_path, budget_mib << 20, reader)?;
    let open_ms = ms(started);
    let mut last = panta_mesh::chunked::IoCounts::default();
    let mut delta = || {
        let now = volume.io_counts();
        let d = (
            now.reads - last.reads,
            mib(now.bytes - last.bytes),
            now.mapped - last.mapped,
        );
        last = now;
        d
    };
    let (open_reads, open_mib, open_mapped) = delta();
    println!(
        "[打开] {} {open_ms:.1} ms，{} 块，读调用 {open_reads}，映射访问 {open_mapped}，读 {open_mib:.1} MiB",
        volume.reader_kind(),
        volume.headers().len()
    );
    let mut json = Vec::new();
    let mut planes = Vec::new();
    for (label, normal, fraction) in [
        ("boundary_layer_z", [0.0, 0.0, 1.0], 0.01),
        ("core_oblique", [0.3, 0.2, 1.0], 0.5),
        ("axis_x", [1.0, 0.0, 0.0], 0.37),
    ] {
        volume.cache().clear();
        let plane = volume.plane_through_nodes(normal, fraction)?;
        let hits = volume.hit_chunks(&plane).len();
        let started = Instant::now();
        let section = volume.fine_section(&plane, &|| false)?.ok_or("取消")?;
        let fine_ms = ms(started);
        let (reads, read_mib, mapped) = delta();
        let started = Instant::now();
        let field = volume
            .fine_section_field(&plane, 1, FieldComponent::Magnitude, &|| false)?
            .ok_or("取消")?;
        let field_ms = ms(started);
        let (field_reads, field_mib, field_mapped) = delta();
        println!(
            "[{label}] 冷缓存切面 {fine_ms:.1} ms（命中 {hits} 块，{} 三角形，读调用 {reads}，映射访问 {mapped}，读 {read_mib:.1} MiB）；单元场 cell_id {field_ms:.1} ms（读调用 {field_reads}，映射访问 {field_mapped}，读 {field_mib:.1} MiB）",
            section.triangles.len()
        );
        json.push(format!(
            "\"{label}\":{{\"hits\":{hits},\"triangles\":{},\"fine_ms\":{fine_ms:.1},\"reads\":{reads},\"mapped\":{mapped},\"read_mib\":{read_mib:.1},\"cell_field_ms\":{field_ms:.1},\"cell_field_reads\":{field_reads},\"cell_field_mapped\":{field_mapped}}}",
            section.triangles.len()
        ));
        planes.push((label, normal, fraction, plane, section, field));
    }
    volume.cache().clear();
    reset_peak();
    let started = Instant::now();
    let quality = volume.quality(&|| false)?.ok_or("取消")?;
    let quality_s = started.elapsed().as_secs_f64();
    let (quality_reads, quality_mib, quality_mapped) = delta();
    let quality_peak = peak_mib();
    let started = Instant::now();
    volume.quality(&|| false)?.ok_or("取消")?;
    let quality_cached_ms = ms(started);
    let by_type: Vec<String> = CellType::ALL
        .iter()
        .zip(&quality.by_type)
        .map(|(kind, q)| {
            format!(
                "{kind:?} {} 个 SJ min {:.3} / mean {:.3} / 劣质 {}",
                q.count, q.min_scaled_jacobian, q.mean_scaled_jacobian, q.poor_count
            )
        })
        .collect();
    println!(
        "[质量·分块] 首次 {quality_s:.2}s（读调用 {quality_reads}，映射访问 {quality_mapped}，{quality_mib:.1} MiB），缓存后 {quality_cached_ms:.2} ms，峰值 RSS {quality_peak} MiB；{}",
        by_type.join("；")
    );
    let chunk_files = volume.io_counts().files;

    // 4. 对比：无派生分块时逐次扫描资产。
    let io = proc_io();
    let started = Instant::now();
    let mut asset = VolumeAssetChunks::open(&asset_path)?;
    let scanned = panta_mesh::tet_quality_streaming(&mut asset, &Progress::new())?.ok_or("取消")?;
    let scan_quality_s = started.elapsed().as_secs_f64();
    let scan_quality_io = io_since(io).unwrap_or_default();
    let io = proc_io();
    let started = Instant::now();
    let mut asset = VolumeAssetChunks::open(&asset_path)?;
    let (_, normal, fraction, _, _, _) = &planes[1];
    let scan_section =
        panta_mesh::section_plane_streaming(&mut asset, *normal, *fraction, &Progress::new())?
            .ok_or("取消")?;
    let scan_section_ms = ms(started);
    let scan_section_io = io_since(io).unwrap_or_default();
    drop(asset);
    let same_counts = scanned.by_type.map(|q| q.count) == quality.by_type.map(|q| q.count)
        && scanned.poor_count == quality.poor_count
        && scan_section.triangles.len() == planes[1].4.triangles.len();
    println!(
        "[回退·逐次扫描资产] 质量 {scan_quality_s:.2}s（读调用 {}，{:.1} MiB）；core_oblique 切面 {scan_section_ms:.1} ms（读调用 {}，{:.1} MiB）；与分块结果计数一致：{same_counts}",
        scan_quality_io.syscr,
        mib(scan_quality_io.rchar),
        scan_section_io.syscr,
        mib(scan_section_io.rchar)
    );
    if !same_counts {
        return Err("分块与逐次扫描结果计数不一致".into());
    }

    // 5. 可选：与驻留算法逐项核对。
    let mut verified = "null";
    if verify {
        let MeshAsset::Volume(mesh) = panta_mesh::decode_mesh_asset(&std::fs::read(&asset_path)?)?
        else {
            return Err("不是体网格资产".into());
        };
        let resident = panta_mesh::tet_quality(&mesh, &Progress::new()).ok_or("取消")?;
        let mut ok = resident
            .by_type
            .map(|q| (q.count, q.poor_count, q.min_scaled_jacobian.to_bits()))
            == quality
                .by_type
                .map(|q| (q.count, q.poor_count, q.min_scaled_jacobian.to_bits()))
            && resident.poor_count == quality.poor_count;
        let field = SyntheticFields(&block).load(1)?;
        for (_, _, _, plane, section, chunk_field) in &planes {
            let sorted = |triangles: &[[[f64; 3]; 3]]| {
                let mut keys: Vec<[u64; 9]> = triangles
                    .iter()
                    .map(|t| std::array::from_fn(|i| t[i / 3][i % 3].to_bits()))
                    .collect();
                keys.sort_unstable();
                keys
            };
            let expected =
                panta_mesh::section_surface(&mesh, plane.origin, plane.normal, &Progress::new())?
                    .ok_or("取消")?;
            ok &= sorted(&expected.triangles) == sorted(&section.triangles);
            let expected_field = panta_mesh::section_surface_field(
                &mesh,
                &field,
                FieldComponent::Magnitude,
                plane.origin,
                plane.normal,
                &Progress::new(),
            )?
            .ok_or("取消")?;
            let mut a: Vec<u64> = chunk_field.values.iter().map(|v| v[0].to_bits()).collect();
            let mut b: Vec<u64> = expected_field
                .values
                .iter()
                .map(|v| v[0].to_bits())
                .collect();
            a.sort_unstable();
            b.sort_unstable();
            ok &= a == b;
        }
        verified = if ok { "true" } else { "false" };
        println!("[核对] 分块切面 / 单元场切面 / 逐类型质量与驻留算法一致：{verified}");
        if !ok {
            return Err("分块结果与驻留算法不一致".into());
        }
    }
    println!(
        "JSON {{\"proc_io\":{has_proc_io},\"reader\":\"{}\",\"cells\":{},\"tets\":{},\"nodes\":{},\"types\":[{},{},{},{}],\"asset_mib\":{:.1},\"write_s\":{write_s:.3},\"write_calls\":{},\"write_peak_mib\":{write_peak},\"build_s\":{build_s:.3},\"chunks\":{},\"pchunk_mib\":{:.1},\"build_asset_reads\":{},\"build_asset_read_mib\":{:.1},\"build_write_calls\":{},\"build_peak_mib\":{build_peak},\"open_ms\":{open_ms:.1},\"open_reads\":{open_reads},\"chunk_files\":{chunk_files},{},\"quality_s\":{quality_s:.3},\"quality_reads\":{quality_reads},\"quality_mapped\":{quality_mapped},\"quality_peak_mib\":{quality_peak},\"scan_quality_s\":{scan_quality_s:.3},\"scan_quality_reads\":{},\"scan_section_ms\":{scan_section_ms:.1},\"scan_section_reads\":{},\"verified\":{verified}}}",
        volume.reader_kind(),
        counts.cells,
        counts.tets,
        counts.nodes,
        types[0],
        types[1],
        types[2],
        types[3],
        mib(asset_bytes),
        write_io.syscw,
        summary.chunk_count,
        mib(summary.bytes_written),
        build_io.syscr,
        mib(build_io.rchar),
        summary.write_calls,
        json.join(","),
        scan_quality_io.syscr,
        scan_section_io.syscr
    );
    drop(volume);
    if !keep {
        std::fs::remove_file(&asset_path)?;
        std::fs::remove_file(&chunk_path)?;
    }
    Ok(())
}
