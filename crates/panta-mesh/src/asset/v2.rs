//! COMPAT(task 011; remove-task 012): 只读解码 task 011 之前写入的 v2 sidecar
//! （单个 Zstandard 流、整段 SHA-256）。现有工程的清单仍登记 `format_version = 2`，
//! 新写入一律为 v3；删除条件与验证入口见 `ai-docs/task/012-mesh-asset-v2-removal.md`。

use super::{HEADER_SIZE, MeshAsset, MeshAssetError, SURFACE_KIND, TET_KIND};
use crate::{SurfaceMesh, TetMesh, validate_tet_mesh};
use sha2::{Digest, Sha256};
use std::io::Read;
use zstd::stream::Decoder;

pub(super) const VERSION: u32 = 2;
const SURFACE_COUNT_BYTES: usize = size_of::<u64>();
const SURFACE_TRIANGLE_BYTES: usize = 3 * 3 * size_of::<f64>();
const ZSTD_CODEC: u8 = 1;

/// v2 头部第 14 字节必须是 Zstandard，第 15 字节保留为 0。
pub(super) fn check_header(header: &[u8]) -> Result<(), MeshAssetError> {
    if header[14] != ZSTD_CODEC {
        return Err(MeshAssetError::UnsupportedCodec(header[14]));
    }
    if header[15] != 0 {
        return Err(MeshAssetError::InvalidHeader);
    }
    Ok(())
}

/// `bytes` 为完整文件且头部已通过校验；摘要覆盖整个压缩 payload。
pub(super) fn decode(bytes: &[u8]) -> Result<(MeshAsset, String), MeshAssetError> {
    let payload = &bytes[HEADER_SIZE..];
    let digest = Sha256::digest(payload);
    if digest[..] != bytes[24..56] {
        return Err(MeshAssetError::ChecksumMismatch);
    }
    let mesh = match bytes[12] {
        SURFACE_KIND => decode_surface_compressed(payload).map(MeshAsset::Surface),
        TET_KIND => decode_volume_compressed(payload).map(MeshAsset::Volume),
        other => Err(MeshAssetError::UnsupportedKind(other)),
    }?;
    Ok((mesh, format!("sha256:{digest:x}")))
}

fn decode_surface_compressed(payload: &[u8]) -> Result<SurfaceMesh, MeshAssetError> {
    let mut decoder = Decoder::new(payload).map_err(|_| MeshAssetError::CodecFailed)?;
    let count =
        usize::try_from(read_u64(&mut decoder)?).map_err(|_| MeshAssetError::InvalidLength)?;
    if count == 0
        || count
            .checked_mul(SURFACE_TRIANGLE_BYTES)
            .and_then(|size| size.checked_add(SURFACE_COUNT_BYTES))
            .is_none()
    {
        return Err(MeshAssetError::InvalidLength);
    }
    let mut triangles = Vec::new();
    triangles
        .try_reserve_exact(count)
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    for _ in 0..count {
        let mut encoded_triangle = [0; SURFACE_TRIANGLE_BYTES];
        read_exact(&mut decoder, &mut encoded_triangle)?;
        let mut triangle = [[0.0; 3]; 3];
        for (index, coordinate) in triangle.iter_mut().flatten().enumerate() {
            let offset = index * 8;
            *coordinate = f64::from_le_bytes(
                encoded_triangle[offset..offset + 8]
                    .try_into()
                    .map_err(|_| MeshAssetError::InvalidLength)?,
            );
            if !coordinate.is_finite() {
                return Err(MeshAssetError::InvalidMesh);
            }
        }
        triangles.push(triangle);
    }
    ensure_decompressed_eof(&mut decoder)?;
    Ok(SurfaceMesh { triangles })
}

fn decode_volume_compressed(payload: &[u8]) -> Result<TetMesh, MeshAssetError> {
    let mut decoder = Decoder::new(payload).map_err(|_| MeshAssetError::CodecFailed)?;
    let node_count =
        usize::try_from(read_u64(&mut decoder)?).map_err(|_| MeshAssetError::InvalidLength)?;
    let tet_count =
        usize::try_from(read_u64(&mut decoder)?).map_err(|_| MeshAssetError::InvalidLength)?;
    let boundary_count =
        usize::try_from(read_u64(&mut decoder)?).map_err(|_| MeshAssetError::InvalidLength)?;
    let region_count = read_u32(&mut decoder)?;
    let boundary_group_count = read_u32(&mut decoder)?;
    node_count
        .checked_mul(24)
        .and_then(|size| {
            tet_count
                .checked_mul(20)
                .and_then(|other| size.checked_add(other))
        })
        .and_then(|size| {
            boundary_count
                .checked_mul(16)
                .and_then(|other| size.checked_add(other))
        })
        .and_then(|size| size.checked_add(32))
        .ok_or(MeshAssetError::SizeOverflow)?;
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(node_count)
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    for _ in 0..node_count {
        let mut encoded_node = [0; 24];
        read_exact(&mut decoder, &mut encoded_node)?;
        let mut node = [0.0; 3];
        for (index, coordinate) in node.iter_mut().enumerate() {
            let offset = index * 8;
            *coordinate = f64::from_le_bytes(
                encoded_node[offset..offset + 8]
                    .try_into()
                    .map_err(|_| MeshAssetError::InvalidLength)?,
            );
            if !coordinate.is_finite() {
                return Err(MeshAssetError::InvalidMesh);
            }
        }
        nodes.push(node);
    }
    let mut tets = Vec::new();
    tets.try_reserve_exact(tet_count)
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    for _ in 0..tet_count {
        let mut encoded_tet = [0; 20];
        read_exact(&mut decoder, &mut encoded_tet)?;
        tets.push(crate::Tetrahedron {
            nodes: [
                u32::from_le_bytes(
                    encoded_tet[0..4]
                        .try_into()
                        .map_err(|_| MeshAssetError::InvalidLength)?,
                ),
                u32::from_le_bytes(
                    encoded_tet[4..8]
                        .try_into()
                        .map_err(|_| MeshAssetError::InvalidLength)?,
                ),
                u32::from_le_bytes(
                    encoded_tet[8..12]
                        .try_into()
                        .map_err(|_| MeshAssetError::InvalidLength)?,
                ),
                u32::from_le_bytes(
                    encoded_tet[12..16]
                        .try_into()
                        .map_err(|_| MeshAssetError::InvalidLength)?,
                ),
            ],
            region: u32::from_le_bytes(
                encoded_tet[16..20]
                    .try_into()
                    .map_err(|_| MeshAssetError::InvalidLength)?,
            ),
        });
    }
    let mut boundary = Vec::new();
    boundary
        .try_reserve_exact(boundary_count)
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    for _ in 0..boundary_count {
        let mut encoded_triangle = [0; 16];
        read_exact(&mut decoder, &mut encoded_triangle)?;
        boundary.push(crate::SurfaceTriangle {
            nodes: [
                u32::from_le_bytes(
                    encoded_triangle[0..4]
                        .try_into()
                        .map_err(|_| MeshAssetError::InvalidLength)?,
                ),
                u32::from_le_bytes(
                    encoded_triangle[4..8]
                        .try_into()
                        .map_err(|_| MeshAssetError::InvalidLength)?,
                ),
                u32::from_le_bytes(
                    encoded_triangle[8..12]
                        .try_into()
                        .map_err(|_| MeshAssetError::InvalidLength)?,
                ),
            ],
            group: u32::from_le_bytes(
                encoded_triangle[12..16]
                    .try_into()
                    .map_err(|_| MeshAssetError::InvalidLength)?,
            ),
        });
    }
    ensure_decompressed_eof(&mut decoder)?;
    let mesh = TetMesh {
        nodes,
        tets,
        boundary,
        region_count,
        boundary_group_count,
        cells: None,
    };
    if !validate_tet_mesh(&mesh).is_ok() {
        return Err(MeshAssetError::InvalidMesh);
    }
    Ok(mesh)
}

fn read_exact<R: Read>(reader: &mut R, bytes: &mut [u8]) -> Result<(), MeshAssetError> {
    reader
        .read_exact(bytes)
        .map_err(|_| MeshAssetError::InvalidLength)
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, MeshAssetError> {
    let mut bytes = [0; 8];
    read_exact(reader, &mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, MeshAssetError> {
    let mut bytes = [0; 4];
    read_exact(reader, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn ensure_decompressed_eof<R: Read>(reader: &mut R) -> Result<(), MeshAssetError> {
    let mut extra = [0; 1];
    if reader
        .read(&mut extra)
        .map_err(|_| MeshAssetError::CodecFailed)?
        == 0
    {
        Ok(())
    } else {
        Err(MeshAssetError::InvalidLength)
    }
}
