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
    /// 体网格导入：含非线性四面体单元或四面体校验失败（task 021）。
    ImportVolumeUnsupported(String),
    /// 体网格命令作用于非体网格（task 021）。
    VolumeUnavailable(String),
    /// STEP → 体网格生成失败：来源不可读、参数非法或生成结果未通过校验（task 023）。
    VolumeGenerationFailed(String),
    ImportSourceChanged(String),
    ImportAssetWriteFailed(String),
    ImportRecordMissing(String),
    Io(String),
    /// 导出目标扩展名不是可写出的网格格式。
    ExportUnsupportedFormat(String),
    /// 网格超出目标格式的计数 / 索引上限。
    ExportTooLarge(String),
    /// 读取或校验待导出的已提交资产失败；detail 前缀为原激活错误码。
    ExportSourceUnavailable(String),
    ExportWriteFailed(String),
    /// 宿主在写出完成前取消了导出；目标文件未被改动。
    ExportCancelled,
    /// 流式读取完整网格资产失败：资产缺失、摘要与激活时不一致或块校验失败（task 025）。
    StreamedAssetFailed(String),
    /// 大网格设置超出范围或预算不小于阈值；detail 为字段名（task 026）。
    LodSettingsInvalid(String),
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
            Self::ImportVolumeUnsupported(_) => "project.import_volume_unsupported",
            Self::VolumeUnavailable(_) => "project.volume_unavailable",
            Self::VolumeGenerationFailed(_) => "project.volume_generation_failed",
            Self::ImportSourceChanged(_) => "project.import_source_changed",
            Self::ImportAssetWriteFailed(_) => "project.import_asset_write_failed",
            Self::ImportRecordMissing(_) => "project.import_record_missing",
            Self::Io(_) => "project.io",
            Self::ExportUnsupportedFormat(_) => "project.export_unsupported_format",
            Self::ExportTooLarge(_) => "project.export_too_large",
            Self::ExportSourceUnavailable(_) => "project.export_source_unavailable",
            Self::ExportWriteFailed(_) => "project.export_write_failed",
            Self::ExportCancelled => "project.export_cancelled",
            Self::StreamedAssetFailed(_) => "project.streamed_asset_failed",
            Self::LodSettingsInvalid(_) => "project.lod_settings_invalid",
        }
    }

    /// 稳定诊断类别；本地化消息由界面按 code 选择。
    pub fn category(&self) -> &'static str {
        match self {
            Self::EmptyName
            | Self::InvalidName(_)
            | Self::LocationEmpty
            | Self::LocationNotAbsolute(_)
            | Self::CommandInvalid(_)
            | Self::LodSettingsInvalid(_) => "validation",
            Self::FileMissing(_) | Self::ImportFileMissing(_) | Self::ImportRecordMissing(_) => {
                "missing"
            }
            Self::AlreadyExists(_) | Self::ImportSourceChanged(_) => "conflict",
            Self::NoProject | Self::ExportCancelled | Self::VolumeUnavailable(_) => "state",
            Self::InvalidFile(_)
            | Self::ManifestInvalid(_)
            | Self::UnsupportedSchema(_)
            | Self::ImportInvalidFile(_)
            | Self::ImportUnsupportedMeshType(_)
            | Self::ImportUnsupportedUnits(_)
            | Self::ImportParseFailed(_)
            | Self::ImportVolumeUnsupported(_)
            | Self::VolumeGenerationFailed(_)
            | Self::ExportUnsupportedFormat(_)
            | Self::ExportTooLarge(_)
            | Self::ExportSourceUnavailable(_) => "format",
            Self::LocationCreateFailed(_)
            | Self::ImportAssetWriteFailed(_)
            | Self::ExportWriteFailed(_)
            | Self::StreamedAssetFailed(_)
            | Self::Io(_) => "io",
        }
    }

    /// 原始上下文，可包含路径、参数或系统错误；不得用它推断错误类型。
    pub fn detail(&self) -> String {
        match self {
            Self::EmptyName | Self::LocationEmpty | Self::NoProject | Self::ExportCancelled => {
                String::new()
            }
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
            | Self::ImportVolumeUnsupported(name)
            | Self::VolumeUnavailable(name)
            | Self::VolumeGenerationFailed(name)
            | Self::ImportSourceChanged(name)
            | Self::ImportAssetWriteFailed(name)
            | Self::ImportRecordMissing(name)
            | Self::ExportUnsupportedFormat(name)
            | Self::ExportTooLarge(name)
            | Self::ExportSourceUnavailable(name)
            | Self::ExportWriteFailed(name)
            | Self::StreamedAssetFailed(name)
            | Self::LodSettingsInvalid(name)
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
