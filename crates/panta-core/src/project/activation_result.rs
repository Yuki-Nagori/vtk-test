//! 激活终态、缓存提交和工作区收尾属于应用服务，语言边界只适配结果。

use super::{ActivationResultKind, DocumentDiagnostic, Outcome, OutcomeKind, ProjectService};
use panta_mesh::SurfaceMesh;
use std::sync::Arc;

/// 已由应用服务完成缓存与工作区收尾的终态；网格仅共享不可变所有权。
pub struct CompletedActivation {
    pub attempt: u64,
    pub generation: u64,
    pub import_id: String,
    pub kind: OutcomeKind,
    pub code: String,
    pub category: &'static str,
    pub detail: String,
    pub mesh: Option<Arc<SurfaceMesh>>,
    pub lod_source_triangles: u64,
}

impl ProjectService {
    pub(super) fn complete_asset_activation(
        &mut self,
        mut outcome: Outcome,
    ) -> CompletedActivation {
        let mut mesh = None;
        if !self.is_current_activation_attempt(&outcome.import_id, outcome.attempt) {
            outcome.kind = OutcomeKind::Expired;
            outcome.code.clear();
            outcome.category = "";
            outcome.detail.clear();
        } else if outcome.kind == OutcomeKind::Succeeded {
            if let Some(value) = outcome.mesh.take() {
                self.cache_activation_result(&outcome.import_id, outcome.attempt, value);
                self.set_mesh_lod_source(&outcome.import_id, outcome.lod_source_triangles);
                match outcome.volume.take() {
                    Some(volume) => {
                        self.volume_meshes.insert(outcome.import_id.clone(), volume);
                    }
                    None => {
                        self.volume_meshes.remove(&outcome.import_id);
                    }
                }
                mesh = self.mesh_cache.snapshot(&outcome.import_id);
            } else {
                outcome.kind = OutcomeKind::Failed;
                outcome.code = "project.mesh_missing".to_owned();
                outcome.category = "state";
                outcome.detail = outcome.import_id.clone();
            }
        }
        self.finish_asset_activation(&outcome.import_id, outcome.attempt);
        let result = match outcome.kind {
            OutcomeKind::Succeeded => ActivationResultKind::Succeeded,
            OutcomeKind::Failed => ActivationResultKind::Failed,
            OutcomeKind::Cancelled => ActivationResultKind::Cancelled,
            OutcomeKind::Expired => ActivationResultKind::Expired,
        };
        self.resolve_workspace_activation(
            &outcome.import_id,
            outcome.attempt,
            result,
            DocumentDiagnostic {
                code: outcome.code.clone(),
                category: outcome.category.to_owned(),
                detail: outcome.detail.clone(),
            },
        );
        CompletedActivation {
            attempt: outcome.attempt,
            generation: outcome.generation,
            import_id: outcome.import_id,
            kind: outcome.kind,
            code: outcome.code,
            category: outcome.category,
            detail: outcome.detail,
            mesh,
            lod_source_triangles: outcome.lod_source_triangles,
        }
    }

    /// 捕获驻留网格的不可变所有权；调用只增加引用，格式转换可在宿主后台执行。
    pub fn mesh_snapshot_for_import(&self, import_id: &str) -> Option<Arc<SurfaceMesh>> {
        self.mesh_cache.snapshot(import_id)
    }

    /// 驻留网格若为显示 LOD，返回源资产面数；完整网格或未驻留返回 0。
    pub fn mesh_lod_source_for_import(&self, import_id: &str) -> u64 {
        if self.mesh_cache.get(import_id).is_none() {
            return 0;
        }
        self.mesh_lod_sources.get(import_id).copied().unwrap_or(0)
    }

    pub(super) fn set_mesh_lod_source(&mut self, import_id: &str, source: u64) {
        if source == 0 {
            self.mesh_lod_sources.remove(import_id);
        } else {
            self.mesh_lod_sources.insert(import_id.to_owned(), source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(attempt: u64, kind: OutcomeKind) -> Outcome {
        Outcome {
            attempt,
            generation: 0,
            import_id: "part".into(),
            kind,
            code: String::new(),
            category: "",
            detail: String::new(),
            mesh: Some(SurfaceMesh {
                triangles: vec![[[0.0; 3]; 3]],
            }),
            lod_source_triangles: 0,
            volume: None,
        }
    }

    #[test]
    fn stale_outcome_cannot_publish_or_finish_a_newer_attempt() {
        let mut service = ProjectService::new();
        service.activation_attempts.insert("part".into(), 2);
        let result = service.complete_asset_activation(outcome(1, OutcomeKind::Succeeded));
        assert_eq!(result.kind, OutcomeKind::Expired);
        assert!(result.mesh.is_none());
        assert!(service.mesh_for_import("part").is_none());
        assert!(service.is_current_activation_attempt("part", 2));
    }

    #[test]
    fn service_completes_cache_and_workspace_without_a_language_adapter() {
        let mut service = ProjectService::new();
        service.workspace.open_import("part", "Part");
        service.workspace.activation_request("part");
        service.workspace.set_activation_attempt("part", 2);
        service.activation_attempts.insert("part".into(), 2);
        let result = service.complete_asset_activation(outcome(2, OutcomeKind::Succeeded));
        assert_eq!(result.kind, OutcomeKind::Succeeded);
        assert!(result.mesh.is_some());
        assert!(service.activation_attempts.is_empty());
        assert_eq!(service.document_workspace().active_document_id, "part");
        let owned = service.mesh_snapshot_for_import("part");
        service.close_workspace_document("part");
        assert!(service.mesh_for_import("part").is_none());
        assert_eq!(owned.as_ref().map(|mesh| mesh.triangles.len()), Some(1));
    }

    #[test]
    fn missing_mesh_marks_workspace_failed_and_clears_attempt() {
        let mut service = ProjectService::new();
        service.workspace.open_import("part", "Part");
        service.workspace.activation_request("part");
        service.workspace.set_activation_attempt("part", 2);
        service.activation_attempts.insert("part".into(), 2);
        let mut input = outcome(2, OutcomeKind::Succeeded);
        input.mesh = None;
        let result = service.complete_asset_activation(input);
        assert_eq!(result.kind, OutcomeKind::Failed);
        assert_eq!(result.code, "project.mesh_missing");
        assert!(service.activation_attempts.is_empty());
        assert_eq!(
            service.document_workspace().documents[0].state,
            super::super::DocumentState::Failed
        );
    }
}
