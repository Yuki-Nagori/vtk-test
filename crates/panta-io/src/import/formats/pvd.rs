//! ParaView 集合文件 `.pvd`（task 033）：`<VTKFile type="Collection">` 下的
//! `<DataSet timestep=".." file="..">` 按时间列出 VTU 文件。
//!
//! 快照把集合文件与它引用的 VTU 按时间升序打成一个包（条目布局同 OpenFOAM 算例包，包头不同），
//! 第一条是集合文件本身。更早步 VTU 总字节超出预算时只收最新一步。
//! 所有步必须共享同一拓扑（节点坐标逐位相等、单元类型与节点表相等），否则整体拒绝；
//! 网格随时间变化（自适应 / 运动网格）不在本 task 范围内。

use super::openfoam::bundle;
use super::{FormatError, FormatResult, RawGrid};
use panta_mesh::SurfaceMesh;

pub const MAGIC: &[u8] = b"PANTA-PVD 1\n";
const MALFORMED: &str = "malformed PVD collection snapshot";

/// 更早步 VTU 文件总字节上限，与 OpenFOAM 更早时间目录相同（2 GiB）。
pub const MAX_EARLIER_STEP_BYTES: u64 = bundle::MAX_EARLIER_STEP_BYTES;

/// 集合中的一步：物理时间、来源标签（`timestep` 原文）与相对集合文件的 VTU 路径。
#[derive(Debug, Clone, PartialEq)]
pub struct PvdStep {
    pub time: f64,
    pub label: String,
    pub file: String,
}

/// 解析集合文件并按时间升序返回各步。只接受单一 `part`、`.vtu` 数据集与互不相同的有限时间。
pub fn parse_collection(bytes: &[u8]) -> FormatResult<Vec<PvdStep>> {
    let tags = super::vtk_xml::scan_tags(bytes, bytes.len())?;
    let root = tags
        .iter()
        .find(|tag| tag.name == "VTKFile" && !tag.closing)
        .ok_or_else(|| FormatError::new("missing VTKFile element"))?;
    if root.attributes.get("type").map(String::as_str) != Some("Collection") {
        return Err(FormatError::new("VTKFile type is not Collection"));
    }
    let mut steps = Vec::new();
    let mut part: Option<&str> = None;
    for tag in tags
        .iter()
        .filter(|tag| tag.name == "DataSet" && !tag.closing)
    {
        let file = tag
            .attributes
            .get("file")
            .map(|file| file.trim())
            .filter(|file| !file.is_empty())
            .ok_or_else(|| FormatError::new("DataSet has no file attribute"))?;
        let label = tag
            .attributes
            .get("timestep")
            .map(|label| label.trim())
            .ok_or_else(|| FormatError::new(format!("DataSet {file} has no timestep")))?;
        let time = label
            .parse::<f64>()
            .ok()
            .filter(|time| time.is_finite())
            .ok_or_else(|| {
                FormatError::new(format!("DataSet {file} has invalid timestep {label:?}"))
            })?;
        let this_part = tag.attributes.get("part").map_or("0", |part| part.trim());
        match part {
            None => part = Some(this_part),
            Some(first) if first != this_part => {
                return Err(FormatError::new(
                    "collections with several parts are not supported",
                ));
            }
            Some(_) => {}
        }
        if !file.to_ascii_lowercase().ends_with(".vtu") {
            return Err(FormatError::new(format!(
                "DataSet {file}: only .vtu datasets are supported in collections"
            )));
        }
        steps.push(PvdStep {
            time,
            label: label.to_owned(),
            file: file.to_owned(),
        });
    }
    if steps.is_empty() {
        return Err(FormatError::new("collection has no DataSet"));
    }
    steps.sort_by(|left, right| left.time.total_cmp(&right.time));
    if let Some(pair) = steps.windows(2).find(|pair| pair[0].time == pair[1].time) {
        return Err(FormatError::new(format!(
            "timestep {} appears more than once",
            pair[1].label
        )));
    }
    Ok(steps)
}

/// 解码快照：各步与已收入的 VTU 内容（全部步，或超预算时只有最新步）。
fn decode(bytes: &[u8]) -> FormatResult<(Vec<PvdStep>, Vec<&[u8]>)> {
    let entries = bundle::decode_with(bytes, MAGIC, MALFORMED)?;
    let (collection, files) = entries
        .split_first()
        .ok_or_else(|| FormatError::new(MALFORMED))?;
    let steps = parse_collection(collection.bytes)?;
    let expected: &[PvdStep] = if files.len() == steps.len() {
        &steps
    } else {
        &steps[steps.len() - 1..]
    };
    if files.len() != expected.len()
        || files
            .iter()
            .zip(expected)
            .any(|(file, step)| file.relative != step.file)
    {
        return Err(FormatError::new(MALFORMED));
    }
    let contents = files.iter().map(|file| file.bytes).collect();
    Ok((steps, contents))
}

fn in_step(step: &PvdStep, error: FormatError) -> FormatError {
    FormatError::new(format!(
        "{} (timestep {}): {}",
        step.file, step.label, error.0
    ))
}

/// 表面导入只看最新一步。
pub fn parse_pvd_surface(bytes: &[u8]) -> FormatResult<SurfaceMesh> {
    let (steps, files) = decode(bytes)?;
    let (Some(step), Some(file)) = (steps.last(), files.last()) else {
        return Err(FormatError::new(MALFORMED));
    };
    super::parse_vtu(file).map_err(|error| in_step(step, error))
}

/// 体导入：最新一步给出网格与 `fields`，更早各步只取结果场并核对拓扑。
pub fn parse_pvd_grid(bytes: &[u8]) -> FormatResult<RawGrid> {
    let (steps, files) = decode(bytes)?;
    let (Some(last), Some(latest)) = (steps.last(), files.last()) else {
        return Err(FormatError::new(MALFORMED));
    };
    let mut grid = super::parse_vtu_grid(latest).map_err(|error| in_step(last, error))?;
    if steps.len() == 1 {
        return Ok(grid);
    }
    if files.len() == 1 {
        grid.skipped_fields.push(format!(
            "time steps: earlier VTU files exceed {MAX_EARLIER_STEP_BYTES} bytes; \
             only the latest step {} was imported",
            last.label
        ));
        return Ok(grid);
    }
    let mut latest_fields = std::mem::take(&mut grid.fields);
    let mut latest_notes = std::mem::take(&mut grid.skipped_fields);
    let list: Vec<(f64, String)> = steps
        .iter()
        .map(|step| (step.time, step.label.clone()))
        .collect();
    let mut failure = None;
    grid.collect_time_steps(&list, |grid, index| {
        if failure.is_some() {
            return;
        }
        if index + 1 == steps.len() {
            grid.fields = std::mem::take(&mut latest_fields);
            grid.skipped_fields.append(&mut latest_notes);
            return;
        }
        let step = &steps[index];
        match super::parse_vtu_grid(files[index]) {
            Err(error) => failure = Some(in_step(step, error)),
            Ok(other) if !same_topology(grid, &other) => {
                failure = Some(in_step(
                    step,
                    FormatError::new(
                        "mesh differs from the latest step; collections whose points or cells \
                         change over time are not supported",
                    ),
                ));
            }
            Ok(other) => {
                grid.fields = other.fields;
                grid.skipped_fields.extend(other.skipped_fields);
            }
        }
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(grid),
    }
}

fn same_topology(left: &RawGrid, right: &RawGrid) -> bool {
    left.types == right.types
        && left.connectivity == right.connectivity
        && left.points.len() == right.points.len()
        && left
            .points
            .iter()
            .zip(&right.points)
            .all(|(a, b)| a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits()))
}

#[cfg(test)]
mod tests;
