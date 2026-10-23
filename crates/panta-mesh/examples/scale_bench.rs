//! 大规模网格基准（task 019）：生成合成 v3 资产 → 流式构建显示 LOD → （可选）整网格加载。
//!
//! 用法见 ai-docs/task/019-large-mesh-lod.md：
//! `cargo run --release -p panta-mesh --example scale_bench -- --triangles 1000000000 --dir /data/bench`
//!
//! 参数：
//! - `--triangles N`：面数，支持 `1M`、`10M`、`1B` 后缀（默认 1M）。
//! - `--dir PATH`：资产输出目录（默认系统临时目录）；文件名 `synthetic-<N>.pmesh`。
//! - `--budget N`：LOD 面数预算（默认 2M）。
//! - `--reuse`：已存在同名资产时跳过生成。
//! - `--full auto|always|never`：整网格加载；auto 在展开体积 < 可用内存 70% 时执行（默认 auto）。
//! - `--keep`：结束后保留资产文件（默认删除）。
//!
//! 峰值内存取 Linux `/proc/self/status` 的 VmHWM，并在阶段间经 `/proc/self/clear_refs`
//! 重置；其他平台输出 N/A，可用 `/usr/bin/time -l`（macOS）或任务管理器测量。
//! 帧率需要 GPU 与窗口，本工具不测量。

use panta_mesh::{
    MeshAssetReader, Progress, SyntheticTorus, build_surface_lod, surface_mesh_expanded_bytes,
    write_surface_asset_streaming,
};
use std::path::PathBuf;
use std::time::Instant;

fn parse_count(text: &str) -> Result<usize, String> {
    let text = text.trim();
    let (digits, scale) = match text.chars().last() {
        Some('k' | 'K') => (&text[..text.len() - 1], 1_000),
        Some('m' | 'M') => (&text[..text.len() - 1], 1_000_000),
        Some('b' | 'B' | 'g' | 'G') => (&text[..text.len() - 1], 1_000_000_000),
        _ => (text, 1),
    };
    let value: f64 = digits.parse().map_err(|_| format!("无效数量：{text}"))?;
    Ok((value * scale as f64).round() as usize)
}

fn status_kib(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|line| line.starts_with(field))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn available_bytes() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = info
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))?;
    line.split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()
        .map(|kib| kib * 1024)
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut triangles = 1_000_000_usize;
    let mut dir = std::env::temp_dir();
    let mut budget = panta_mesh::DEFAULT_LOD_TRIANGLE_BUDGET;
    let mut reuse = false;
    let mut keep = false;
    let mut full = "auto".to_owned();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--triangles" => triangles = parse_count(&args.next().ok_or("--triangles 缺少值")?)?,
            "--dir" => dir = PathBuf::from(args.next().ok_or("--dir 缺少值")?),
            "--budget" => budget = parse_count(&args.next().ok_or("--budget 缺少值")?)?,
            "--full" => full = args.next().ok_or("--full 缺少值")?,
            "--reuse" => reuse = true,
            "--keep" => keep = true,
            "--help" | "-h" => {
                println!("见源码头部注释或 ai-docs/task/019-large-mesh-lod.md");
                return Ok(());
            }
            other => return Err(format!("未知参数：{other}").into()),
        }
    }
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("synthetic-{triangles}.pmesh"));
    let expanded = surface_mesh_expanded_bytes(triangles as u64);
    println!(
        "面数 {triangles}，展开体积 {:.2} GiB，资产 {}",
        expanded as f64 / 1073741824.0,
        path.display()
    );

    reset_peak();
    let started = Instant::now();
    let generated = if reuse && path.exists() {
        println!("[生成] 复用已有资产");
        None
    } else {
        let torus = SyntheticTorus::new(triangles);
        let file = std::io::BufWriter::with_capacity(8 << 20, std::fs::File::create(&path)?);
        let progress = Progress::new();
        let (len, _) = write_surface_asset_streaming(
            file,
            triangles,
            |r, o| torus.fill(r, o),
            Some(&progress),
        )?
        .ok_or("生成被取消")?;
        Some(len)
    };
    let generate_seconds = started.elapsed().as_secs_f64();
    let generate_peak = peak_mib();
    let file_len = std::fs::metadata(&path)?.len();
    if generated.is_some() {
        println!(
            "[生成] {generate_seconds:.2}s，文件 {:.2} GiB，峰值 RSS {generate_peak} MiB",
            file_len as f64 / 1073741824.0
        );
    }

    reset_peak();
    let started = Instant::now();
    let lod = build_surface_lod(&path, budget, None)?.ok_or("LOD 被取消")?;
    let lod_seconds = started.elapsed().as_secs_f64();
    let lod_peak = peak_mib();
    let lod_triangles = lod.mesh.triangles.len();
    println!(
        "[LOD]  {lod_seconds:.2}s（两遍流式扫描），输出 {} 面（预算 {budget}，格分辨率 {}），峰值 RSS {lod_peak} MiB",
        lod.mesh.triangles.len(),
        lod.grid_resolution
    );

    drop(lod);
    let run_full = match full.as_str() {
        "always" => true,
        "never" => false,
        _ => available_bytes().is_some_and(|available| (expanded as f64) < available as f64 * 0.7),
    };
    let (full_seconds, full_peak) = if run_full {
        reset_peak();
        let started = Instant::now();
        let file = std::fs::File::open(&path)?;
        let mesh = MeshAssetReader::open(std::io::BufReader::new(file), file_len)?
            .read(|| false)?
            .ok_or("读取被取消")?;
        let seconds = started.elapsed().as_secs_f64();
        let peak = peak_mib();
        drop(mesh);
        println!("[整网格] {seconds:.2}s，峰值 RSS {peak} MiB");
        (format!("{seconds:.3}"), peak)
    } else {
        println!("[整网格] 跳过（--full {full}；展开体积超出可用内存 70% 或被禁用）");
        ("null".to_owned(), "N/A".to_owned())
    };
    println!("[帧率] N/A：需要 GPU 与窗口，请在实机上用应用测量");
    let quote = |text: &str| {
        if text == "N/A" {
            "null".to_owned()
        } else {
            text.to_owned()
        }
    };
    println!(
        "JSON {{\"triangles\":{triangles},\"file_bytes\":{file_len},\"generate_s\":{},\"generate_peak_mib\":{},\"lod_s\":{lod_seconds:.3},\"lod_triangles\":{},\"lod_peak_mib\":{},\"full_load_s\":{full_seconds},\"full_peak_mib\":{},\"fps\":null}}",
        if generated.is_some() {
            format!("{generate_seconds:.3}")
        } else {
            "null".to_owned()
        },
        quote(&generate_peak),
        lod_triangles,
        quote(&lod_peak),
        quote(&full_peak),
    );
    if !keep {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}
