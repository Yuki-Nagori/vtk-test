//! 大网格激活设置（task 026）：校验在 Rust，C++ 只做持久化与界面适配。
use crate::{ProjectService, bridge};
use panta_core::project::ActivationLimits;

impl From<ActivationLimits> for bridge::LargeMeshLimits {
    fn from(limits: ActivationLimits) -> Self {
        Self {
            surface_triangles: limits.surface_triangles,
            surface_budget: limits.surface_budget,
            volume_cells: limits.volume_cells,
        }
    }
}

impl From<bridge::LargeMeshLimits> for ActivationLimits {
    fn from(limits: bridge::LargeMeshLimits) -> Self {
        Self {
            surface_triangles: limits.surface_triangles,
            surface_budget: limits.surface_budget,
            volume_cells: limits.volume_cells,
        }
    }
}

pub(super) fn default_large_mesh_limits() -> bridge::LargeMeshLimits {
    ActivationLimits::DEFAULT.into()
}

pub(super) fn project_service_large_mesh_limits(
    service: &ProjectService,
) -> bridge::LargeMeshLimits {
    service.service.activation_limits().into()
}

/// 非法设置返回 `project.lod_settings_invalid`，已生效的设置保持不变。
pub(super) fn project_service_set_large_mesh_limits(
    service: &mut ProjectService,
    limits: bridge::LargeMeshLimits,
) -> bridge::ProjectDiagnostic {
    match service.service.set_activation_limits(limits.into()) {
        Ok(()) => bridge::ProjectDiagnostic::default(),
        Err(error) => error.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_round_trip_and_invalid_values_keep_the_previous_setting() {
        let mut service = crate::project::project_service_new();
        let defaults = default_large_mesh_limits();
        assert_eq!(project_service_large_mesh_limits(&service), defaults);
        let custom = bridge::LargeMeshLimits {
            surface_triangles: 30_000_000,
            surface_budget: 3_000_000,
            volume_cells: 50_000_000,
        };
        assert!(
            project_service_set_large_mesh_limits(&mut service, custom)
                .code
                .is_empty()
        );
        assert_eq!(project_service_large_mesh_limits(&service), custom);
        let error = project_service_set_large_mesh_limits(
            &mut service,
            bridge::LargeMeshLimits {
                volume_cells: 0,
                ..custom
            },
        );
        assert_eq!(error.code, "project.lod_settings_invalid");
        assert_eq!(error.detail, "volume_cells");
        assert_eq!(project_service_large_mesh_limits(&service), custom);
    }
}
