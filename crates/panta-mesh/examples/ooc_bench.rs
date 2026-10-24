//! out-of-core 分块体网格原型基准（task 027；task 028 增加映射读取对比）。
//!
//! 用法：`cargo run --release -p panta-mesh --example ooc_bench -- [选项]`
//! - `--cells N`：合成四面体单元数（支持 K/M/B 后缀），默认 100M。
//! - `--chunk N`：每块单元上限，默认 262144。
//! - `--budget-mib N`：块缓存预算，默认 1024。
//! - `--dir PATH`：资产目录，默认系统临时目录；`--reuse` 复用已有文件；`--keep` 保留文件。
//! - `--reader auto|mmap|pread`：块读取实现（task 028），默认 auto（映射，失败回退定位读）。
//! - `--fields`：构建时写入 3 个合成结果场（节点标量、节点矢量、单元标量；task 029），
//!   并测场着色剖面的延迟与 IO（读调用、读取字节、打开文件数）。
//!
//! 只测几何计算延迟与 IO（读调用次数、读取字节数、打开文件数；映射实现另计映射访问次数，
//! 缺页由 OS 处理不计为读调用），不含渲染；无 GPU 时帧率不测。
//! 峰值内存取 Linux `/proc/self/status` 的 VmHWM，阶段间经 `/proc/self/clear_refs` 重置；
//! 映射页计入 RSS（文件页，可回收），因此另报当前 RssAnon / RssFile 拆分。其他平台输出 N/A。

use std::time::Instant;

use panta_mesh::chunked::{
    ChunkBuildOptions, ChunkError, ChunkFieldInfo, ChunkReadMode, ChunkedVolume, FieldProvider,
    IoCounts, NoFields, Prefetcher, build_chunked_volume_with_fields,
};
use panta_mesh::{FieldAssociation, FieldComponent, Progress, ResultField, SyntheticTetBlock};

/// 按需生成的合成结果场：一次只驻留一个场。
struct SyntheticFields<'a>(&'a SyntheticTetBlock);

const FIELDS: [(&str, FieldAssociation, u8); 3] = [
    ("pressure", FieldAssociation::Node, 1),
    ("velocity", FieldAssociation::Node, 3),
    ("cell_id", FieldAssociation::Cell, 1),
];

impl FieldProvider for SyntheticFields<'_> {
    fn infos(&self) -> Vec<ChunkFieldInfo> {
        FIELDS
            .iter()
            .map(|&(name, association, components)| ChunkFieldInfo {
                name: name.to_owned(),
                association,
                components,
            })
            .collect()
    }

    fn load(&mut self, index: usize) -> Result<ResultField, ChunkError> {
        let (name, association, components) = FIELDS[index];
        let block = self.0;
        let values = match index {
            0 => (0..block.node_count())
                .map(|i| {
                    let p = block.node(i);
                    p[0] + 2.0 * p[1] - p[2]
                })
                .collect(),
            1 => (0..block.node_count())
                .flat_map(|i| {
                    let p = block.node(i);
                    [p[1], -p[0], 0.5 * p[2]]
                })
                .collect(),
            _ => (0..block.tet_count()).map(|i| i as f64).collect(),
        };
        Ok(ResultField {
            name: name.to_owned(),
            association,
            components,
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
        || "N/A".to_owned(),
        |kib| format!("{:.1}", kib as f64 / 1024.0),
    )
}

fn rss_mib() -> String {
    status_kib("VmRSS:").map_or_else(
        || "N/A".to_owned(),
        |kib| format!("{:.1}", kib as f64 / 1024.0),
    )
}

/// 当前匿名 / 文件页 RSS（MiB）。
fn rss_split() -> String {
    match (status_kib("RssAnon:"), status_kib("RssFile:")) {
        (Some(anon), Some(file)) => format!(
            "匿名 {:.1} / 文件页 {:.1} MiB",
            anon as f64 / 1024.0,
            file as f64 / 1024.0
        ),
        _ => "N/A".to_owned(),
    }
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn percentile(values: &[f64], fraction: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted
        .get(((sorted.len() as f64 - 1.0) * fraction).round() as usize)
        .copied()
        .unwrap_or(0.0)
}

struct Io<'a> {
    volume: &'a ChunkedVolume,
    last: IoCounts,
}

impl Io<'_> {
    /// 自上次调用以来的（读调用次数，读取 / 访问 MiB，映射访问次数）。
    fn delta(&mut self) -> (u64, f64, u64) {
        let now = self.volume.io_counts();
        let delta = (
            now.reads - self.last.reads,
            (now.bytes - self.last.bytes) as f64 / 1048576.0,
            now.mapped - self.last.mapped,
        );
        self.last = now;
        delta
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut cells = 100_000_000;
    let mut chunk = 1 << 18;
    let mut budget_mib = 1024usize;
    let mut dir = std::env::temp_dir();
    let (mut reuse, mut keep, mut with_fields) = (false, false, false);
    let mut reader = ChunkReadMode::Auto;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cells" => cells = parse_count(&args.next().ok_or("--cells 缺少值")?)?,
            "--chunk" => chunk = parse_count(&args.next().ok_or("--chunk 缺少值")?)?,
            "--budget-mib" => budget_mib = parse_count(&args.next().ok_or("--budget-mib 缺少值")?)?,
            "--dir" => dir = args.next().ok_or("--dir 缺少值")?.into(),
            "--reuse" => reuse = true,
            "--keep" => keep = true,
            "--fields" => with_fields = true,
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
    let block = SyntheticTetBlock::new(cells);
    let suffix = if with_fields { "-fields" } else { "" };
    let path = dir.join(format!(
        "synthetic-tet-{}{suffix}.pchunk",
        block.tet_count()
    ));
    println!(
        "合成四面体 {} / 节点 {}，块上限 {chunk}，缓存预算 {budget_mib} MiB，线程 {}",
        block.tet_count(),
        block.node_count(),
        std::thread::available_parallelism().map_or(1, usize::from)
    );

    let mut build_json = "null".to_owned();
    if !(reuse && path.exists()) {
        reset_peak();
        let started = Instant::now();
        let partial = path.with_extension("pchunk.partial");
        let file = std::fs::File::create(&partial)?;
        let options = ChunkBuildOptions {
            max_chunk_cells: chunk,
            ..ChunkBuildOptions::default()
        };
        let summary = if with_fields {
            build_chunked_volume_with_fields(
                &block,
                &mut SyntheticFields(&block),
                options,
                &file,
                &Progress::new(),
            )
        } else {
            build_chunked_volume_with_fields(
                &block,
                &mut NoFields,
                options,
                &file,
                &Progress::new(),
            )
        }?
        .ok_or("构建被取消")?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&partial, &path)?;
        let seconds = started.elapsed().as_secs_f64();
        println!(
            "[分块构建] {seconds:.2}s，{} 块，{:.2} GiB，写调用 {}，打开文件 1，峰值 RSS {} MiB",
            summary.chunk_count,
            summary.bytes_written as f64 / 1073741824.0,
            summary.write_calls,
            peak_mib()
        );
        build_json = format!(
            "{{\"seconds\":{seconds:.3},\"chunks\":{},\"bytes\":{},\"write_calls\":{},\"peak_mib\":{}}}",
            summary.chunk_count,
            summary.bytes_written,
            summary.write_calls,
            peak_mib()
        );
    }

    reset_peak();
    let started = Instant::now();
    let volume = ChunkedVolume::open_file_with(&path, budget_mib << 20, reader)?;
    let open_ms = ms(started);
    let kind = volume.reader_kind();
    let mut io = Io {
        volume: &volume,
        last: IoCounts::default(),
    };
    let (open_reads, open_mib, open_mapped) = io.delta();
    println!(
        "[打开] 读取实现 {kind}，{open_ms:.1} ms，{} 块，常驻索引+表面约 {:.1} MiB，读调用 {open_reads}，映射访问 {open_mapped}，读 {open_mib:.1} MiB，RSS {} MiB",
        volume.headers().len(),
        volume.resident_index_bytes() as f64 / 1048576.0,
        rss_mib()
    );

    let mut json = Vec::new();
    for (label, normal) in [("oblique", [0.3, 0.2, 1.0]), ("axis_z", [0.0, 0.0, 1.0])] {
        volume.cache().clear();
        reset_peak();
        // 冷缓存松手：块不在 LRU 中，需读文件并解压（文件可能仍在 OS 页缓存，无 root 无法清除）。
        let plane = volume.plane_at(normal, 0.5)?;
        let hits = volume.hit_chunks(&plane).len();
        let started = Instant::now();
        let fine = volume.fine_section(&plane, &|| false)?.ok_or("取消")?;
        let cold_ms = ms(started);
        let (cold_reads, cold_mib, cold_mapped) = io.delta();
        println!(
            "[{label}] 冷缓存精细：{cold_ms:.1} ms，命中 {hits} 块，{} 三角形，读调用 {cold_reads}，映射访问 {cold_mapped}，读 {cold_mib:.1} MiB",
            fine.triangles.len()
        );
        let started = Instant::now();
        volume.fine_section(&plane, &|| false)?.ok_or("取消")?;
        let repeat_ms = ms(started);
        io.delta();
        if !volume.fields().is_empty() {
            // 场着色：首次选场（读外表面值 + 命中块场值）、同场换分量、换场。
            let mut runs = Vec::new();
            for (field, component, what) in [
                (1, FieldComponent::Magnitude, "选场 velocity"),
                (1, FieldComponent::Component(0), "换分量"),
                (0, FieldComponent::Magnitude, "换场 pressure"),
                (2, FieldComponent::Magnitude, "换场 cell_id"),
            ] {
                let started = Instant::now();
                let section = volume
                    .fine_section_field(&plane, field, component, &|| false)?
                    .ok_or("取消")?;
                let took = ms(started);
                let (reads, mib, mapped) = io.delta();
                println!(
                    "[{label}] 场剖面 {what}：{took:.1} ms，{} 三角形，读调用 {reads}，映射访问 {mapped}，读 {mib:.1} MiB",
                    section.surface.triangles.len()
                );
                runs.push(format!(
                    "{{\"ms\":{took:.1},\"reads\":{reads},\"read_mib\":{mib:.1}}}"
                ));
            }
            json.push(format!("\"{label}_field\":[{}]", runs.join(",")));
        }

        // 拖动：0.30 → 0.70，每步 1%，每步出粗级别，同时预取下一位置的命中块。
        volume.cache().clear();
        let prefetcher = Prefetcher::new(volume.cache().clone());
        let fractions: Vec<f64> = (30..=70).map(|step| f64::from(step) / 100.0).collect();
        let mut coarse = Vec::new();
        let mut coarse_triangles = 0;
        for (index, &fraction) in fractions.iter().enumerate() {
            let plane = volume.plane_at(normal, fraction)?;
            let started = Instant::now();
            let mesh = volume.coarse_section(&plane);
            coarse.push(ms(started));
            coarse_triangles = coarse_triangles.max(mesh.triangles.len());
            if let Some(&next) = fractions.get(index + 1) {
                prefetcher.request(volume.hit_chunks(&volume.plane_at(normal, next)?));
            }
            // 模拟 60 Hz 拖动事件间隔，给预取线程时间。
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        let release = volume.plane_at(normal, 0.70)?;
        let started = Instant::now();
        volume.fine_section(&release, &|| false)?.ok_or("取消")?;
        let release_ms = ms(started);
        let (drag_reads, drag_mib, drag_mapped) = io.delta();

        // 逐步停顿：每个位置都算精细（有预取），统计精细延迟分布。
        let mut fine_steps = Vec::new();
        for (index, &fraction) in fractions.iter().enumerate() {
            let plane = volume.plane_at(normal, 1.0 - fraction)?;
            let started = Instant::now();
            volume.fine_section(&plane, &|| false)?.ok_or("取消")?;
            fine_steps.push(ms(started));
            if let Some(&next) = fractions.get(index + 1) {
                prefetcher.request(volume.hit_chunks(&volume.plane_at(normal, 1.0 - next)?));
            }
        }
        drop(prefetcher);
        let (step_reads, step_mib, step_mapped) = io.delta();
        let (cache_bytes, cache_chunks, hits_count, misses) = volume.cache().stats();
        let peak = peak_mib();
        println!(
            "[{label}] 拖动粗级别 p50 {:.1} / p95 {:.1} / max {:.1} ms（≤{coarse_triangles} 三角形）；松手精细 {release_ms:.1} ms；重复同平面 {repeat_ms:.1} ms",
            percentile(&coarse, 0.5),
            percentile(&coarse, 0.95),
            percentile(&coarse, 1.0)
        );
        println!(
            "[{label}] 逐步精细 p50 {:.1} / p95 {:.1} / max {:.1} ms；拖动读调用 {drag_reads} / 映射访问 {drag_mapped}（{drag_mib:.1} MiB），逐步读调用 {step_reads} / 映射访问 {step_mapped}（{step_mib:.1} MiB）；缓存 {cache_chunks} 块 {:.1} MiB，命中 {hits_count} / 未命中 {misses}；峰值 RSS {peak} MiB（当前 {}）",
            percentile(&fine_steps, 0.5),
            percentile(&fine_steps, 0.95),
            percentile(&fine_steps, 1.0),
            cache_bytes as f64 / 1048576.0,
            rss_split()
        );
        json.push(format!(
            "\"{label}\":{{\"hits\":{hits},\"cold_fine_ms\":{cold_ms:.1},\"cold_reads\":{cold_reads},\"cold_mapped\":{cold_mapped},\"cold_read_mib\":{cold_mib:.1},\"repeat_ms\":{repeat_ms:.1},\"coarse_p50_ms\":{:.1},\"coarse_max_ms\":{:.1},\"release_fine_ms\":{release_ms:.1},\"drag_reads\":{drag_reads},\"step_fine_p50_ms\":{:.1},\"step_fine_max_ms\":{:.1},\"step_reads\":{step_reads},\"peak_mib\":{peak}}}",
            percentile(&coarse, 0.5),
            percentile(&coarse, 1.0),
            percentile(&fine_steps, 0.5),
            percentile(&fine_steps, 1.0)
        ));
    }

    volume.cache().clear();
    reset_peak();
    let started = Instant::now();
    let quality = volume.quality(&|| false)?.ok_or("取消")?;
    let quality_s = started.elapsed().as_secs_f64();
    let (quality_reads, quality_mib, quality_mapped) = io.delta();
    let started = Instant::now();
    volume.quality(&|| false)?.ok_or("取消")?;
    let quality_cached_ms = ms(started);
    println!(
        "[质量] 首次 {quality_s:.2}s（读调用 {quality_reads}，映射访问 {quality_mapped}，{quality_mib:.1} MiB），缓存后 {quality_cached_ms:.2} ms，最小二面角 {:.2}°，峰值 RSS {} MiB（当前 {}）",
        quality.min_dihedral_degrees,
        peak_mib(),
        rss_split()
    );
    println!(
        "JSON {{\"reader\":\"{kind}\",\"tets\":{},\"build\":{build_json},\"open_ms\":{open_ms:.1},\"open_reads\":{open_reads},\"open_files\":{},{},\"quality_s\":{quality_s:.3},\"quality_reads\":{quality_reads}}}",
        block.tet_count(),
        volume.io_counts().files,
        json.join(",")
    );
    drop(volume);
    if !keep {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}
