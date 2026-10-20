//! schema 4 清单的分析序列目录与读取校验。
use super::{ProjectError, ProjectManifest};

/// 稳定领域 ID 与英文源文案；翻译由 Qt 展示层处理。
#[derive(Debug, Clone, Copy)]
pub struct AnalysisSequenceDefinition {
    pub id: &'static str,
    pub source_text: &'static str,
}

/// 当前序列目录；执行阶段与适用性规则由实际求解消费者扩展。
pub const ANALYSIS_SEQUENCES: &[AnalysisSequenceDefinition] = &[
    AnalysisSequenceDefinition {
        id: "fill",
        source_text: "Fill",
    },
    AnalysisSequenceDefinition {
        id: "fill-pack",
        source_text: "Fill + Pack",
    },
    AnalysisSequenceDefinition {
        id: "fast-fill",
        source_text: "Fast Fill",
    },
    AnalysisSequenceDefinition {
        id: "fill-pack-warp",
        source_text: "Fill + Pack + Warp",
    },
    AnalysisSequenceDefinition {
        id: "cool",
        source_text: "Cool",
    },
    AnalysisSequenceDefinition {
        id: "cool-fill-pack-warp",
        source_text: "Cool + Fill + Pack + Warp",
    },
    AnalysisSequenceDefinition {
        id: "molding-window",
        source_text: "Molding Window",
    },
    AnalysisSequenceDefinition {
        id: "gate-location",
        source_text: "Gate Location",
    },
    AnalysisSequenceDefinition {
        id: "cool-fem",
        source_text: "Cool (FEM)",
    },
    AnalysisSequenceDefinition {
        id: "cool-fem-fill-pack-warp",
        source_text: "Cool (FEM) + Fill + Pack + Warp",
    },
];

pub(super) fn definition(id: &str) -> Option<&'static AnalysisSequenceDefinition> {
    ANALYSIS_SEQUENCES.iter().find(|entry| entry.id == id)
}

pub(super) fn validate_sequences(manifest: &ProjectManifest) -> Result<(), ProjectError> {
    for (id, sequence) in &manifest.analysis_sequences {
        if !manifest.imports.iter().any(|record| record.id == *id) || definition(sequence).is_none()
        {
            return Err(ProjectError::ManifestInvalid(format!(
                "invalid analysis sequence for {id}"
            )));
        }
    }
    Ok(())
}
