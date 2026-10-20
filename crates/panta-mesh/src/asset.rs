//! 面向工程旁置文件的版本化 Mesh 编解码；不依赖 VTK / Netgen 内存布局。

use crate::{SurfaceMesh, TetMesh, validate_tet_mesh};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::io::{self, Read, Write};
use zstd::stream::{Decoder, Encoder};

const MAGIC: &[u8; 8] = b"PNTMESH\0";
pub const MESH_ASSET_FORMAT_VERSION: u32 = 2;
const HEADER_SIZE: usize = 56;
const SURFACE_COUNT_BYTES: usize = size_of::<u64>();
const SURFACE_TRIANGLE_BYTES: usize = 3 * 3 * size_of::<f64>();
const SURFACE_KIND: u8 = 1;
const TET_KIND: u8 = 2;
const ZSTD_CODEC: u8 = 1;

/// 未压缩表面 Mesh 资产的布局字节数，包含文件头与面数，不估算进程总内存。
/// 超出 `u64` 时饱和，供资源提醒使用；不据此分配或限制网格规模。
pub fn surface_mesh_expanded_bytes(triangle_count: u64) -> u64 {
    triangle_count
        .saturating_mul(SURFACE_TRIANGLE_BYTES as u64)
        .saturating_add((HEADER_SIZE + SURFACE_COUNT_BYTES) as u64)
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeshAsset {
    Surface(SurfaceMesh),
    Volume(TetMesh),
}

pub enum MeshAssetRef<'a> {
    Surface(&'a SurfaceMesh),
    Volume(&'a TetMesh),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshAssetError {
    InvalidHeader,
    UnsupportedVersion(u32),
    UnsupportedKind(u8),
    UnsupportedUnits(u8),
    InvalidLength,
    ChecksumMismatch,
    UnsupportedCodec(u8),
    CodecFailed,
    InvalidMesh,
    SizeOverflow,
    AllocationFailed,
}

impl Display for MeshAssetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MeshAssetError {}

/// 编码固定小端 IEEE-754 / u32 字段，并以 Zstandard 压缩 payload。
pub fn encode_mesh_asset(mesh: &MeshAsset) -> Result<(Vec<u8>, String), MeshAssetError> {
    match mesh {
        MeshAsset::Surface(mesh) => encode_surface_asset(mesh),
        MeshAsset::Volume(mesh) => encode_volume_asset(mesh),
    }
}

pub fn encode_mesh_asset_ref(mesh: MeshAssetRef<'_>) -> Result<(Vec<u8>, String), MeshAssetError> {
    match mesh {
        MeshAssetRef::Surface(mesh) => encode_surface_asset(mesh),
        MeshAssetRef::Volume(mesh) => encode_volume_asset(mesh),
    }
}

fn encode_payload(
    kind: u8,
    write_payload: impl FnOnce(&mut Encoder<'static, FallibleVecWriter>) -> Result<(), MeshAssetError>,
) -> Result<(Vec<u8>, String), MeshAssetError> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(HEADER_SIZE)
        .map_err(|_| MeshAssetError::AllocationFailed)?;
    output.resize(HEADER_SIZE, 0);
    let mut encoder = Encoder::new(FallibleVecWriter(output), 3).map_err(map_codec_error)?;
    write_payload(&mut encoder)?;
    let mut bytes = encoder.finish().map_err(map_codec_error)?.into_inner();
    let payload_len =
        u64::try_from(bytes.len() - HEADER_SIZE).map_err(|_| MeshAssetError::SizeOverflow)?;
    let digest = Sha256::digest(&bytes[HEADER_SIZE..]);
    let digest_text = format!("sha256:{digest:x}");
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..12].copy_from_slice(&MESH_ASSET_FORMAT_VERSION.to_le_bytes());
    bytes[12] = kind;
    bytes[13] = 1; // Coordinates are stored in millimeters.
    bytes[14] = ZSTD_CODEC;
    bytes[16..24].copy_from_slice(&payload_len.to_le_bytes());
    bytes[24..56].copy_from_slice(&digest);
    Ok((bytes, digest_text))
}

struct FallibleVecWriter(Vec<u8>);

impl FallibleVecWriter {
    fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

impl Write for FallibleVecWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0
            .try_reserve(buffer.len())
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        self.0.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn map_codec_error(error: io::Error) -> MeshAssetError {
    if error.kind() == io::ErrorKind::OutOfMemory {
        MeshAssetError::AllocationFailed
    } else {
        MeshAssetError::CodecFailed
    }
}

/// 验证格式、长度与内容摘要后解码，并对所有浮点坐标和体网格连接关系校验。
pub fn decode_mesh_asset(bytes: &[u8]) -> Result<MeshAsset, MeshAssetError> {
    decode_mesh_asset_with_digest(bytes).map(|(mesh, _)| mesh)
}

pub fn decode_mesh_asset_with_digest(bytes: &[u8]) -> Result<(MeshAsset, String), MeshAssetError> {
    if bytes.len() < HEADER_SIZE || &bytes[..8] != MAGIC {
        return Err(MeshAssetError::InvalidHeader);
    }
    let declared_len = mesh_asset_encoded_len(&bytes[..HEADER_SIZE])?;
    if declared_len != bytes.len() {
        return Err(MeshAssetError::InvalidLength);
    }
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

/// 从完整固定长度头部验证格式并返回总字节长度，以便流式读取端先拒绝异常文件。
pub fn mesh_asset_encoded_len(header: &[u8]) -> Result<usize, MeshAssetError> {
    if header.len() != HEADER_SIZE || &header[..8] != MAGIC {
        return Err(MeshAssetError::InvalidHeader);
    }
    let version = u32::from_le_bytes(
        header[8..12]
            .try_into()
            .map_err(|_| MeshAssetError::InvalidHeader)?,
    );
    if version != MESH_ASSET_FORMAT_VERSION {
        return Err(MeshAssetError::UnsupportedVersion(version));
    }
    if !matches!(header[12], SURFACE_KIND | TET_KIND) {
        return Err(MeshAssetError::UnsupportedKind(header[12]));
    }
    if header[13] != 1 {
        return Err(MeshAssetError::UnsupportedUnits(header[13]));
    }
    if header[14] != ZSTD_CODEC {
        return Err(MeshAssetError::UnsupportedCodec(header[14]));
    }
    if header[15] != 0 {
        return Err(MeshAssetError::InvalidHeader);
    }
    let payload_len = u64::from_le_bytes(
        header[16..24]
            .try_into()
            .map_err(|_| MeshAssetError::InvalidHeader)?,
    );
    let payload_len = usize::try_from(payload_len).map_err(|_| MeshAssetError::InvalidLength)?;
    let total_len = HEADER_SIZE
        .checked_add(payload_len)
        .ok_or(MeshAssetError::InvalidLength)?;
    Ok(total_len)
}

fn encode_surface_asset(mesh: &SurfaceMesh) -> Result<(Vec<u8>, String), MeshAssetError> {
    if mesh.triangles.is_empty()
        || mesh
            .triangles
            .iter()
            .flatten()
            .flatten()
            .any(|coordinate| !coordinate.is_finite())
    {
        return Err(MeshAssetError::InvalidMesh);
    }
    let count = u64::try_from(mesh.triangles.len()).map_err(|_| MeshAssetError::SizeOverflow)?;
    encode_payload(SURFACE_KIND, |encoder| {
        encoder
            .write_all(&count.to_le_bytes())
            .map_err(map_codec_error)?;
        for triangle in &mesh.triangles {
            let mut encoded_triangle = [0; SURFACE_TRIANGLE_BYTES];
            for (index, coordinate) in triangle.iter().flatten().enumerate() {
                let offset = index * 8;
                encoded_triangle[offset..offset + 8].copy_from_slice(&coordinate.to_le_bytes());
            }
            encoder
                .write_all(&encoded_triangle)
                .map_err(map_codec_error)?;
        }
        Ok(())
    })
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

fn encode_volume_asset(mesh: &TetMesh) -> Result<(Vec<u8>, String), MeshAssetError> {
    if !validate_tet_mesh(mesh).is_ok() {
        return Err(MeshAssetError::InvalidMesh);
    }
    let node_count = u64::try_from(mesh.nodes.len()).map_err(|_| MeshAssetError::SizeOverflow)?;
    let tet_count = u64::try_from(mesh.tets.len()).map_err(|_| MeshAssetError::SizeOverflow)?;
    let boundary_count =
        u64::try_from(mesh.boundary.len()).map_err(|_| MeshAssetError::SizeOverflow)?;
    encode_payload(TET_KIND, |encoder| {
        encoder
            .write_all(&node_count.to_le_bytes())
            .map_err(map_codec_error)?;
        encoder
            .write_all(&tet_count.to_le_bytes())
            .map_err(map_codec_error)?;
        encoder
            .write_all(&boundary_count.to_le_bytes())
            .map_err(map_codec_error)?;
        encoder
            .write_all(&mesh.region_count.to_le_bytes())
            .map_err(map_codec_error)?;
        encoder
            .write_all(&mesh.boundary_group_count.to_le_bytes())
            .map_err(map_codec_error)?;
        for node in &mesh.nodes {
            let mut encoded_node = [0; 24];
            for (index, coordinate) in node.iter().enumerate() {
                let offset = index * 8;
                encoded_node[offset..offset + 8].copy_from_slice(&coordinate.to_le_bytes());
            }
            encoder.write_all(&encoded_node).map_err(map_codec_error)?;
        }
        for tet in &mesh.tets {
            let mut encoded_tet = [0; 20];
            for (index, node) in tet.nodes.into_iter().enumerate() {
                let offset = index * 4;
                encoded_tet[offset..offset + 4].copy_from_slice(&node.to_le_bytes());
            }
            encoded_tet[16..20].copy_from_slice(&tet.region.to_le_bytes());
            encoder.write_all(&encoded_tet).map_err(map_codec_error)?;
        }
        for triangle in &mesh.boundary {
            let mut encoded_triangle = [0; 16];
            for (index, node) in triangle.nodes.into_iter().enumerate() {
                let offset = index * 4;
                encoded_triangle[offset..offset + 4].copy_from_slice(&node.to_le_bytes());
            }
            encoded_triangle[12..16].copy_from_slice(&triangle.group.to_le_bytes());
            encoder
                .write_all(&encoded_triangle)
                .map_err(map_codec_error)?;
        }
        Ok(())
    })
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

#[cfg(test)]
mod tests {
    use super::{
        HEADER_SIZE, MeshAsset, MeshAssetError, decode_mesh_asset, encode_mesh_asset,
        mesh_asset_encoded_len, surface_mesh_expanded_bytes,
    };
    use crate::{SurfaceMesh, SurfaceTriangle, TetMesh, Tetrahedron};
    use sha2::{Digest, Sha256};
    use std::io::Read;

    #[test]
    fn expanded_surface_size_matches_the_encoded_layout() -> Result<(), Box<dyn std::error::Error>>
    {
        let mesh = MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[1.0, 2.0, 3.0]; 3]; 2],
        });
        let (bytes, _) = encode_mesh_asset(&mesh)?;
        let raw_payload = zstd::stream::decode_all(&bytes[HEADER_SIZE..])?;
        assert_eq!(
            surface_mesh_expanded_bytes(2),
            (HEADER_SIZE + raw_payload.len()) as u64
        );
        assert_eq!(surface_mesh_expanded_bytes(0), 64);
        assert_eq!(surface_mesh_expanded_bytes(u64::MAX), u64::MAX);
        Ok(())
    }

    fn sidecar_with_repacked_payload(
        original: &[u8],
        raw_payload: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let payload = zstd::stream::encode_all(raw_payload, 3)?;
        sidecar_with_compressed_payload(original, &payload)
    }

    fn sidecar_with_compressed_payload(
        original: &[u8],
        payload: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut bytes = original[..56].to_vec();
        bytes.extend_from_slice(payload);
        bytes[16..24].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        let digest = Sha256::digest(payload);
        bytes[24..56].copy_from_slice(&digest);
        Ok(bytes)
    }

    #[test]
    fn surface_mesh_round_trips_and_checks_payload_digest() -> Result<(), Box<dyn std::error::Error>>
    {
        let mesh = MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[1.0, 2.0, 3.0]; 3]],
        });
        let (mut bytes, digest) = encode_mesh_asset(&mesh)?;
        assert!(digest.starts_with("sha256:"));
        assert_eq!(decode_mesh_asset(&bytes)?, mesh);
        *bytes.last_mut().ok_or(MeshAssetError::InvalidLength)? ^= 1;
        assert_eq!(
            decode_mesh_asset(&bytes),
            Err(MeshAssetError::ChecksumMismatch)
        );
        Ok(())
    }

    #[test]
    fn volume_mesh_round_trips_and_rejects_invalid_connectivity()
    -> Result<(), Box<dyn std::error::Error>> {
        let mesh = MeshAsset::Volume(TetMesh {
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
        });
        let (bytes, _) = encode_mesh_asset(&mesh)?;
        assert_eq!(decode_mesh_asset(&bytes)?, mesh);

        let mut decoder = zstd::stream::read::Decoder::new(&bytes[HEADER_SIZE..])?;
        let mut raw_payload = Vec::new();
        decoder.read_to_end(&mut raw_payload)?;
        // 固定头 32 B + 4 个 24 B 节点后是首个四面体的节点索引。
        raw_payload[128..132].copy_from_slice(&99_u32.to_le_bytes());
        let corrupted = sidecar_with_repacked_payload(&bytes, &raw_payload)?;
        assert_eq!(
            decode_mesh_asset(&corrupted),
            Err(MeshAssetError::InvalidMesh)
        );
        Ok(())
    }

    #[test]
    fn zstd_sidecar_compresses_repeated_surface_data() -> Result<(), Box<dyn std::error::Error>> {
        let mesh = MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[1.0, 2.0, 3.0]; 3]; 1_000],
        });
        let (bytes, _) = encode_mesh_asset(&mesh)?;

        assert_eq!(bytes[14], super::ZSTD_CODEC);
        assert!(bytes.len() < 56 + 8 + 1_000 * 72);
        assert_eq!(decode_mesh_asset(&bytes)?, mesh);
        Ok(())
    }

    #[test]
    fn unsupported_version_is_reported() -> Result<(), Box<dyn std::error::Error>> {
        let (mut bytes, _) = encode_mesh_asset(&MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[0.0; 3]; 3]],
        }))?;
        bytes[8..12].copy_from_slice(&99_u32.to_le_bytes());
        assert_eq!(
            decode_mesh_asset(&bytes),
            Err(MeshAssetError::UnsupportedVersion(99))
        );
        Ok(())
    }

    #[test]
    fn unsupported_codec_is_reported() -> Result<(), Box<dyn std::error::Error>> {
        let (mut bytes, _) = encode_mesh_asset(&MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[0.0; 3]; 3]],
        }))?;
        bytes[14] = 99;

        assert_eq!(
            decode_mesh_asset(&bytes),
            Err(MeshAssetError::UnsupportedCodec(99))
        );
        Ok(())
    }

    #[test]
    fn decompressed_payload_must_match_mesh_count_exactly() -> Result<(), Box<dyn std::error::Error>>
    {
        let (bytes, _) = encode_mesh_asset(&MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[0.0; 3]; 3]],
        }))?;
        let mut decoder = zstd::stream::read::Decoder::new(&bytes[56..])?;
        let mut raw_payload = Vec::new();
        decoder.read_to_end(&mut raw_payload)?;

        raw_payload.push(0);
        let bytes = sidecar_with_repacked_payload(&bytes, &raw_payload)?;
        assert_eq!(
            decode_mesh_asset(&bytes),
            Err(MeshAssetError::InvalidLength)
        );
        Ok(())
    }

    #[test]
    fn truncated_zstd_frame_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let (original, _) = encode_mesh_asset(&MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[0.0; 3]; 3]],
        }))?;
        let mut truncated_payload = original[56..].to_vec();
        truncated_payload.pop();
        let bytes = sidecar_with_compressed_payload(&original, &truncated_payload)?;

        assert!(decode_mesh_asset(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn element_count_larger_than_decompressed_data_is_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let (original, _) = encode_mesh_asset(&MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[0.0; 3]; 3]],
        }))?;
        let mut decoder = zstd::stream::read::Decoder::new(&original[56..])?;
        let mut raw_payload = Vec::new();
        decoder.read_to_end(&mut raw_payload)?;
        raw_payload[..8].copy_from_slice(&2_u64.to_le_bytes());
        let bytes = sidecar_with_repacked_payload(&original, &raw_payload)?;

        assert_eq!(
            decode_mesh_asset(&bytes),
            Err(MeshAssetError::InvalidLength)
        );
        Ok(())
    }

    #[test]
    fn asset_larger_than_two_gib_is_not_rejected_by_format_policy()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut bytes, _) = encode_mesh_asset(&MeshAsset::Surface(SurfaceMesh {
            triangles: vec![[[0.0; 3]; 3]],
        }))?;
        let payload_size = (2_u64 * 1024 * 1024 * 1024).to_le_bytes();
        bytes[16..24].copy_from_slice(&payload_size);

        assert!(mesh_asset_encoded_len(&bytes[..56]).is_ok());
        assert_eq!(
            decode_mesh_asset(&bytes),
            Err(MeshAssetError::InvalidLength)
        );
        Ok(())
    }
}
