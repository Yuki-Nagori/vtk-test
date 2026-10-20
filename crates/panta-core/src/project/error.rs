//! 工程可恢复错误与稳定诊断字段。
use std::fmt::{Display, Formatter};

/// 工程服务的可恢复错误；跨语言消费者使用显式字段，Display 仅供日志。
#[derive(Debug)]
pub enum ProjectError {
    EmptyName,
    InvalidName(String),
    LocationEmpty,
    LocationNotAbsolute(String),
    LocationCreateFailed(String),
    FileMissing(String),
    InvalidFile(String),
    AlreadyExists(String),
    ManifestInvalid(String),
    UnsupportedSchema(u32),
    NoProject,
    CommandInvalid(String),
    ImportFileMissing(String),
    ImportInvalidFile(String),
    ImportUnsupportedMeshType(String),
    ImportUnsupportedUnits(String),
    ImportParseFailed(String),
    ImportSourceChanged(String),
    ImportAssetWriteFailed(String),
    ImportRecordMissing(String),
    Io(String),
}

impl ProjectError {
    /// 稳定错误码，不依赖系统错误文案或文件名。
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyName => "project.empty_name",
            Self::InvalidName(_) => "project.invalid_name",
            Self::LocationEmpty => "project.location_empty",
            Self::LocationNotAbsolute(_) => "project.location_not_absolute",
            Self::LocationCreateFailed(_) => "project.location_create_failed",
            Self::FileMissing(_) => "project.file_missing",
            Self::InvalidFile(_) => "project.invalid_file",
            Self::AlreadyExists(_) => "project.already_exists",
            Self::ManifestInvalid(_) => "project.manifest_invalid",
            Self::UnsupportedSchema(_) => "project.unsupported_schema",
            Self::NoProject => "project.no_project",
            Self::CommandInvalid(_) => "project.command_invalid",
            Self::ImportFileMissing(_) => "project.import_file_missing",
            Self::ImportInvalidFile(_) => "project.import_invalid_file",
            Self::ImportUnsupportedMeshType(_) => "project.import_unsupported_mesh_type",
            Self::ImportUnsupportedUnits(_) => "project.import_unsupported_units",
            Self::ImportParseFailed(_) => "project.import_parse_failed",
            Self::ImportSourceChanged(_) => "project.import_source_changed",
            Self::ImportAssetWriteFailed(_) => "project.import_asset_write_failed",
            Self::ImportRecordMissing(_) => "project.import_record_missing",
            Self::Io(_) => "project.io",
        }
    }

    /// 稳定诊断类别；本地化消息由界面按 code 选择。
    pub fn category(&self) -> &'static str {
        match self {
            Self::EmptyName
            | Self::InvalidName(_)
            | Self::LocationEmpty
            | Self::LocationNotAbsolute(_)
            | Self::CommandInvalid(_) => "validation",
            Self::FileMissing(_) | Self::ImportFileMissing(_) | Self::ImportRecordMissing(_) => {
                "missing"
            }
            Self::AlreadyExists(_) | Self::ImportSourceChanged(_) => "conflict",
            Self::NoProject => "state",
            Self::InvalidFile(_)
            | Self::ManifestInvalid(_)
            | Self::UnsupportedSchema(_)
            | Self::ImportInvalidFile(_)
            | Self::ImportUnsupportedMeshType(_)
            | Self::ImportUnsupportedUnits(_)
            | Self::ImportParseFailed(_) => "format",
            Self::LocationCreateFailed(_) | Self::ImportAssetWriteFailed(_) | Self::Io(_) => "io",
        }
    }

    /// 原始上下文，可包含路径、参数或系统错误；不得用它推断错误类型。
    pub fn detail(&self) -> String {
        match self {
            Self::EmptyName | Self::LocationEmpty | Self::NoProject => String::new(),
            Self::InvalidName(name)
            | Self::LocationNotAbsolute(name)
            | Self::LocationCreateFailed(name)
            | Self::FileMissing(name)
            | Self::InvalidFile(name)
            | Self::AlreadyExists(name)
            | Self::ManifestInvalid(name)
            | Self::CommandInvalid(name)
            | Self::ImportFileMissing(name)
            | Self::ImportInvalidFile(name)
            | Self::ImportUnsupportedMeshType(name)
            | Self::ImportUnsupportedUnits(name)
            | Self::ImportParseFailed(name)
            | Self::ImportSourceChanged(name)
            | Self::ImportAssetWriteFailed(name)
            | Self::ImportRecordMissing(name)
            | Self::Io(name) => name.clone(),
            Self::UnsupportedSchema(schema) => schema.to_string(),
        }
    }
}

impl Display for ProjectError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let detail = self.detail();
        if detail.is_empty() {
            write!(formatter, "{}", self.code())
        } else {
            write!(formatter, "{}: {detail}", self.code())
        }
    }
}

impl std::error::Error for ProjectError {}
