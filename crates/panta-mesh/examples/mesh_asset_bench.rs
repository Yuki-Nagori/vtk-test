//! Mesh 资产编解码基准：`encode <binary.stl> <out.pmesh>` 或 `decode|stream <in.pmesh>`。
//! 每次只做一个阶段，便于用 `/usr/bin/time -v` 分别测峰值 RSS。

use panta_mesh::{
    MeshAsset, MeshAssetReader, SurfaceMesh, decode_mesh_asset_with_digest, encode_mesh_asset,
};
use std::time::Instant;

fn read_binary_stl(path: &str) -> Result<SurfaceMesh, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(path)?;
    let count = u32::from_le_bytes(bytes.get(80..84).ok_or("short stl")?.try_into()?) as usize;
    let mut triangles = Vec::with_capacity(count);
    for index in 0..count {
        let record = bytes
            .get(84 + index * 50..84 + index * 50 + 50)
            .ok_or("short stl")?;
        let mut triangle = [[0.0; 3]; 3];
        for (slot, value) in triangle.iter_mut().flatten().enumerate() {
            let offset = 12 + slot * 4;
            *value = f64::from(f32::from_le_bytes(record[offset..offset + 4].try_into()?));
        }
        triangles.push(triangle);
    }
    Ok(SurfaceMesh { triangles })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("encode") => {
            let mesh = MeshAsset::Surface(read_binary_stl(args.get(2).ok_or("stl")?)?);
            let started = Instant::now();
            let (bytes, digest) = encode_mesh_asset(&mesh)?;
            let elapsed = started.elapsed();
            std::fs::write(args.get(3).ok_or("out")?, &bytes)?;
            println!(
                "encode {:.3}s size {} bytes {digest}",
                elapsed.as_secs_f64(),
                bytes.len()
            );
        }
        Some("decode") => {
            let started = Instant::now();
            let bytes = std::fs::read(args.get(2).ok_or("pmesh")?)?;
            let read = started.elapsed();
            let (mesh, _) = decode_mesh_asset_with_digest(&bytes)?;
            let total = started.elapsed();
            let count = match mesh {
                MeshAsset::Surface(mesh) => mesh.triangles.len(),
                MeshAsset::Volume(mesh) => mesh.tets.len(),
            };
            println!(
                "decode read {:.3}s total {:.3}s elements {count}",
                read.as_secs_f64(),
                total.as_secs_f64()
            );
        }
        Some("stream") => {
            let started = Instant::now();
            let path = args.get(2).ok_or("pmesh")?;
            let file = std::io::BufReader::new(std::fs::File::open(path)?);
            let len = std::fs::metadata(path)?.len();
            let (mesh, _) = MeshAssetReader::open(file, len)?
                .read(|| false)?
                .ok_or("cancelled")?;
            let count = match mesh {
                MeshAsset::Surface(mesh) => mesh.triangles.len(),
                MeshAsset::Volume(mesh) => mesh.tets.len(),
            };
            println!(
                "stream total {:.3}s elements {count}",
                started.elapsed().as_secs_f64()
            );
        }
        _ => return Err("usage: encode <stl> <out> | decode <in> | stream <in>".into()),
    }
    Ok(())
}
