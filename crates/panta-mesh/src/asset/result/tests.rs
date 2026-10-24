use super::{FieldAssociation, ResultAssetReader, ResultField, ResultSet, encode_result_asset};
use crate::MeshAssetError;
use std::path::PathBuf;

const DIGEST: &str = "sha256:00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn sample_set(nodes: usize, cells: usize) -> ResultSet {
    let mut pressure: Vec<f64> = (0..nodes).map(|index| index as f64 * 0.5 - 1.0).collect();
    pressure[1] = f64::NAN;
    let velocity: Vec<f64> = (0..cells * 3).map(|index| (index % 3) as f64).collect();
    ResultSet {
        mesh_digest: DIGEST.to_owned(),
        node_count: nodes as u64,
        cell_count: cells as u64,
        fields: vec![
            ResultField {
                name: "pressure".to_owned(),
                association: FieldAssociation::Node,
                components: 1,
                units: String::new(),
                values: pressure,
            },
            ResultField {
                name: "velocity".to_owned(),
                association: FieldAssociation::Cell,
                components: 3,
                units: "m/s".to_owned(),
                values: velocity,
            },
        ],
    }
}

struct TempFile(PathBuf);

impl TempFile {
    fn write(name: &str, bytes: &[u8]) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "panta-result-{}-{name}.presult",
            std::process::id()
        ));
        std::fs::write(&path, bytes)?;
        Ok(Self(path))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn same_values(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.to_bits() == right.to_bits())
}

#[test]
fn result_asset_round_trips_and_keeps_missing_values() -> Result<(), Box<dyn std::error::Error>> {
    let set = sample_set(10, 4);
    let (bytes, digest) = encode_result_asset(&set)?;
    let file = TempFile::write("round-trip", &bytes)?;
    let reader = ResultAssetReader::open(&file.0)?;
    assert_eq!(reader.digest(), digest);
    assert_eq!(reader.mesh_digest(), DIGEST);
    assert_eq!((reader.node_count(), reader.cell_count()), (10, 4));
    let summaries = reader.summaries().to_vec();
    assert_eq!(summaries.len(), 2);
    // NaN 计为缺失，不按 0 参与范围。
    assert_eq!(summaries[0].missing, 1);
    assert_eq!(summaries[0].range, Some([-1.0, 3.5]));
    assert_eq!(summaries[1].association, FieldAssociation::Cell);
    assert_eq!(summaries[1].units, "m/s");
    assert_eq!(summaries[1].range, Some([5.0_f64.sqrt(), 5.0_f64.sqrt()]));
    let decoded = reader.read_all()?;
    for (left, right) in decoded.fields.iter().zip(&set.fields) {
        assert_eq!(left.name, right.name);
        assert!(same_values(&left.values, &right.values));
    }
    Ok(())
}

#[test]
fn large_field_splits_into_chunks_that_do_not_cross_fields()
-> Result<(), Box<dyn std::error::Error>> {
    // 单块约 4 MiB = 524 288 个 f64；120 万节点的标量跨 3 块。
    let set = sample_set(1_200_000, 3);
    let (bytes, _) = encode_result_asset(&set)?;
    let file = TempFile::write("chunks", &bytes)?;
    let mut reader = ResultAssetReader::open(&file.0)?;
    assert_eq!(reader.field_chunks[0].len(), 3);
    assert_eq!(reader.field_chunks[1].len(), 1);
    let velocity = reader.read_field(1)?;
    assert!(same_values(&velocity.values, &set.fields[1].values));
    Ok(())
}

#[test]
fn invalid_sets_are_rejected() {
    let mut set = sample_set(4, 2);
    set.fields[0].values.pop();
    assert_eq!(encode_result_asset(&set), Err(MeshAssetError::InvalidMesh));
    let mut set = sample_set(4, 2);
    set.fields[1].name = "pressure".to_owned();
    set.fields[1].association = FieldAssociation::Node;
    set.fields[1].values.truncate(12);
    assert_eq!(encode_result_asset(&set), Err(MeshAssetError::InvalidMesh));
    let mut set = sample_set(4, 2);
    set.mesh_digest = "sha256:xyz".to_owned();
    assert_eq!(encode_result_asset(&set), Err(MeshAssetError::InvalidMesh));
    let mut set = sample_set(4, 2);
    set.fields.clear();
    assert_eq!(encode_result_asset(&set), Err(MeshAssetError::InvalidMesh));
    let mut set = sample_set(4, 2);
    set.fields[0].components = 0;
    assert_eq!(encode_result_asset(&set), Err(MeshAssetError::InvalidMesh));
}

#[test]
fn result_and_mesh_readers_reject_each_other() -> Result<(), Box<dyn std::error::Error>> {
    let (bytes, _) = encode_result_asset(&sample_set(4, 2))?;
    // 网格读取端拒绝结果场类型。
    assert_eq!(
        crate::decode_mesh_asset(&bytes),
        Err(MeshAssetError::UnsupportedKind(3))
    );
    let file = TempFile::write("kind", &bytes)?;
    assert!(crate::VolumeAssetChunks::open(&file.0).is_err());
    let (mesh_bytes, _) =
        crate::encode_mesh_asset(&crate::MeshAsset::Surface(crate::SurfaceMesh {
            triangles: vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        }))?;
    let mesh_file = TempFile::write("mesh", &mesh_bytes)?;
    assert!(ResultAssetReader::open(&mesh_file.0).is_err());
    Ok(())
}

#[test]
fn corrupted_values_fail_checksum() -> Result<(), Box<dyn std::error::Error>> {
    let (mut bytes, _) = encode_result_asset(&sample_set(64, 8))?;
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    let file = TempFile::write("corrupt", &bytes)?;
    let mut reader = ResultAssetReader::open(&file.0)?;
    assert_eq!(reader.read_field(0).err(), None);
    assert_eq!(
        reader.read_field(1).err(),
        Some(MeshAssetError::ChecksumMismatch)
    );
    Ok(())
}
