//! 大网格激活设置（task 026 / 028）：校验在 Rust，C++ 只做持久化、界面适配与可用内存查询。
use crate::{ProjectService, bridge};
use panta_core::project::{ActivationLimits, VolumeMode};

impl From<ActivationLimits> for bridge::LargeMeshLimits {
    fn from(limits: ActivationLimits) -> Self {
        Self {
            surface_triangles: limits.surface_triangles,
            surface_budget: limits.surface_budget,
            volume_cells: limits.volume_cells,
            volume_mode: limits.volume_mode.index(),
            chunk_cache_mib: limits.chunk_cache_mib,
        }
    }
}

/// 未知模式编码返回 `None`（按非法设置处理）。
fn limits_from_bridge(limits: bridge::LargeMeshLimits) -> Option<ActivationLimits> {
    Some(ActivationLimits {
        surface_triangles: limits.surface_triangles,
        surface_budget: limits.surface_budget,
        volume_cells: limits.volume_cells,
        volume_mode: VolumeMode::from_index(limits.volume_mode)?,
        chunk_cache_mib: limits.chunk_cache_mib,
    })
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
    let Some(limits) = limits_from_bridge(limits) else {
        return panta_core::project::ProjectError::LodSettingsInvalid("volume_mode".to_owned())
            .into();
    };
    match service.service.set_activation_limits(limits) {
        Ok(()) => bridge::ProjectDiagnostic::default(),
        Err(error) => error.into(),
    }
}

pub(super) fn project_service_set_available_memory(service: &mut ProjectService, bytes: u64) {
    service.service.set_available_memory(bytes);
}

pub(super) fn project_service_set_derived_chunk_dir(service: &mut ProjectService, directory: &str) {
    service
        .service
        .set_derived_chunk_dir((!directory.is_empty()).then(|| directory.into()));
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
            volume_mode: 1,
            chunk_cache_mib: 4096,
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
        for (invalid, field) in [
            (
                bridge::LargeMeshLimits {
                    volume_mode: 3,
                    ..custom
                },
                "volume_mode",
            ),
            (
                bridge::LargeMeshLimits {
                    chunk_cache_mib: 63,
                    ..custom
                },
                "chunk_cache_mib",
            ),
        ] {
            let error = project_service_set_large_mesh_limits(&mut service, invalid);
            assert_eq!(error.code, "project.lod_settings_invalid");
            assert_eq!(error.detail, field);
        }
        assert_eq!(project_service_large_mesh_limits(&service), custom);
        project_service_set_available_memory(&mut service, 8 << 30);
        project_service_set_derived_chunk_dir(&mut service, "");
    }
}
