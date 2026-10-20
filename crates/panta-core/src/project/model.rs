//! 工程应用命令、状态与清单数据；不依赖 Qt、CXX 或命令行解析。
use super::{FillSettings, GateLocationSettings, ImportRecord, ProjectError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 可被工程服务接受的命令；扩展命令时必须增加对应行为与持久化测试。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectCommand {
    Open { path: PathBuf },
    Rename { name: String },
}

/// 提供给 UI/FFI 的工程值快照；path 标识主文件，不暴露可变会话状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSnapshot {
    pub path: PathBuf,
    pub name: String,
    pub revision: u64,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProjectState {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) revision: u64,
    pub(super) dirty: bool,
    // 最近成功读取 / 提交的磁盘修订，与尚未保存的内存 revision 分开。
    pub(super) persisted_revision: Option<u64>,
    pub(super) imports: Vec<ImportRecord>,
    pub(super) mesh_assets: BTreeMap<String, MeshAssetRef>,
    pub(super) analysis_sequences: BTreeMap<String, String>,
    pub(super) materials: BTreeMap<String, String>,
    pub(super) fill_settings: BTreeMap<String, FillSettings>,
    pub(super) gate_location_settings: BTreeMap<String, GateLocationSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ProjectManifest {
    pub(super) schema: u32,
    pub(super) name: String,
    pub(super) revision: u64,
    #[serde(default)]
    pub(super) imports: Vec<ImportRecord>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) mesh_assets: BTreeMap<String, MeshAssetRef>,
    // 稀疏覆盖表；未设置的方案采用领域默认 Fill。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) analysis_sequences: BTreeMap<String, String>,
    // 只保存已确认的材料引用；空表表示尚未分配材料。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) materials: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) fill_settings: BTreeMap<String, FillSettings>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) gate_location_settings: BTreeMap<String, GateLocationSettings>,
}

/// `.panta` 中独立的权威 Mesh 索引项；来源文件元数据仍留在 ImportRecord。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MeshAssetRef {
    pub(super) id: String,
    pub(super) revision: u64,
    pub(super) format_version: u32,
    pub(super) kind: String,
    pub(super) relative_path: String,
    pub(super) digest: String,
    pub(super) units: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) source_revision: Option<String>,
}

pub(super) fn validate_name(name: &str) -> Result<(), ProjectError> {
    if name.is_empty() {
        return Err(ProjectError::EmptyName);
    }
    if name.len() > 255
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name.ends_with('.')
        || name.ends_with(' ')
        || name.to_ascii_lowercase().ends_with(".panta")
        || name.chars().any(|character| character.is_control())
    {
        return Err(ProjectError::InvalidName(name.to_owned()));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        return Err(ProjectError::InvalidName(name.to_owned()));
    }
    Ok(())
}

/// 修订是过期请求的相关性键；耗尽时必须在任何变更或 I/O 前拒绝。
pub(super) fn next_revision(revision: u64) -> Result<u64, ProjectError> {
    revision
        .checked_add(1)
        .ok_or_else(|| ProjectError::CommandInvalid("project revision exhausted".to_owned()))
}
