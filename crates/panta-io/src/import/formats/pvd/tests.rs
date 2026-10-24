use super::*;
use crate::import::{ImportError, ImportFormat, read_source};
use panta_mesh::{CellType, FieldAssociation};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mesh/pvd")
        .join(name)
}

fn values<'a>(fields: &'a [panta_mesh::ResultField], name: &str) -> Option<&'a [f64]> {
    fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.values.as_slice())
}

fn collection(entries: &[(&str, &str)]) -> String {
    let rows: String = entries
        .iter()
        .map(|(time, file)| format!("<DataSet timestep=\"{time}\" part=\"0\" file=\"{file}\"/>"))
        .collect();
    format!("<VTKFile type=\"Collection\"><Collection>{rows}</Collection></VTKFile>")
}

/// 手工拼快照（条目 = 集合文件 + 给定的 VTU）。
fn snapshot(
    collection: &str,
    files: &[(&str, &Path)],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend(u32::try_from(files.len() + 1)?.to_le_bytes());
    bundle::push_entry_header(&mut bytes, "c.pvd", collection.len() as u64)?;
    bytes.extend(collection.as_bytes());
    for (relative, path) in files {
        let content = std::fs::read(path)?;
        bundle::push_entry_header(&mut bytes, relative, content.len() as u64)?;
        bytes.extend(content);
    }
    Ok(bytes)
}

#[test]
fn collection_steps_are_sorted_and_share_the_latest_mesh() -> TestResult {
    let source = read_source(&fixture("flow.pvd"))?;
    assert_eq!(source.format, ImportFormat::Pvd);
    let grid = parse_pvd_grid(&source.bytes)?;
    assert_eq!(grid.points.len(), 9);
    assert_eq!(grid.types, [CellType::Hexahedron, CellType::Tetra]);
    assert!(grid.skipped_fields.is_empty(), "{:?}", grid.skipped_fields);
    let latest: Vec<f64> = (0..9).map(|index| 100.0 + f64::from(index)).collect();
    assert_eq!(values(&grid.fields, "T"), Some(latest.as_slice()));
    assert_eq!(values(&grid.fields, "init"), None);
    let series = grid.time_series.as_ref().ok_or("time series")?;
    assert_eq!(
        (series.latest_time, series.latest_label.as_str()),
        (1.0, "1")
    );
    let earlier: Vec<(f64, &str)> = series
        .earlier
        .iter()
        .map(|step| (step.time, step.label.as_str()))
        .collect();
    assert_eq!(earlier, [(0.0, "0"), (0.5, "0.5")]);
    assert_eq!(
        values(&series.earlier[0].fields, "init"),
        Some([7.0, 8.0].as_slice())
    );
    assert_eq!(
        values(&series.earlier[1].fields, "p"),
        Some([0.5, 1.5].as_slice())
    );
    let middle = series.earlier[1]
        .fields
        .iter()
        .find(|field| field.name == "T")
        .ok_or("T")?;
    assert_eq!(middle.association, FieldAssociation::Node);
    assert_eq!(middle.values[8], 58.0);
    assert!(!parse_pvd_surface(&source.bytes)?.triangles.is_empty());
    Ok(())
}

#[test]
fn changing_topology_rejects_the_collection() -> TestResult {
    let source = read_source(&fixture("moving.pvd"))?;
    let error = parse_pvd_grid(&source.bytes)
        .err()
        .ok_or("moving mesh accepted")?;
    assert!(
        error
            .0
            .contains("steps/flow_0.vtu (timestep 0): mesh differs"),
        "{error}"
    );
    Ok(())
}

#[test]
fn collection_files_are_validated() {
    let reject = |text: &str, needle: &str| {
        let error = parse_collection(text.as_bytes()).err();
        assert!(
            error.as_ref().is_some_and(|error| error.0.contains(needle)),
            "{text}: {error:?}"
        );
    };
    reject("<VTKFile type=\"UnstructuredGrid\"/>", "not Collection");
    reject(&collection(&[]), "no DataSet");
    reject(&collection(&[("0", "a.vtp")]), "only .vtu");
    reject(&collection(&[("x", "a.vtu")]), "invalid timestep");
    reject(&collection(&[("inf", "a.vtu")]), "invalid timestep");
    reject(
        &collection(&[("1", "a.vtu"), ("1.0", "b.vtu")]),
        "more than once",
    );
    reject(
        "<VTKFile type=\"Collection\"><DataSet file=\"a.vtu\"/></VTKFile>",
        "no timestep",
    );
    reject(
        "<VTKFile type=\"Collection\"><DataSet timestep=\"0\" part=\"0\" file=\"a.vtu\"/>\
         <DataSet timestep=\"0\" part=\"1\" file=\"b.vtu\"/></VTKFile>",
        "several parts",
    );
}

#[test]
fn single_dataset_and_budget_limited_snapshots_have_no_time_series() -> TestResult {
    let last = fixture("steps/flow_2.vtu");
    let single = snapshot(&collection(&[("2", "x.vtu")]), &[("x.vtu", &last)])?;
    let grid = parse_pvd_grid(&single)?;
    assert!(grid.time_series.is_none() && grid.skipped_fields.is_empty());
    // 超预算时读取只收最新一步：解析记录原因，只导入最新步。
    let limited = snapshot(
        &collection(&[("0", "a.vtu"), ("1", "b.vtu")]),
        &[("b.vtu", &last)],
    )?;
    let grid = parse_pvd_grid(&limited)?;
    assert!(grid.time_series.is_none());
    assert_eq!(grid.skipped_fields.len(), 1);
    assert!(grid.skipped_fields[0].contains("only the latest step 1"));
    // 条目与集合不符（顺序或名称）视为损坏的快照。
    let mismatched = snapshot(
        &collection(&[("0", "a.vtu"), ("1", "b.vtu")]),
        &[("b.vtu", &last), ("a.vtu", &last)],
    )?;
    assert!(parse_pvd_grid(&mismatched).is_err());
    Ok(())
}

#[test]
fn missing_datasets_fail_the_read() -> TestResult {
    let directory = std::env::temp_dir().join(format!("panta-pvd-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("missing.pvd");
    std::fs::write(&path, collection(&[("0", "nowhere.vtu")]))?;
    let result = read_source(&path);
    std::fs::remove_dir_all(&directory)?;
    assert!(matches!(result, Err(ImportError::Read(message)) if message.contains("nowhere.vtu")));
    Ok(())
}
