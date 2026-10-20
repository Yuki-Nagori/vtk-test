//! 浇口定位工艺配置；目录标识、默认值和校验由领域持有。
use super::process_settings::{NumericConstraint, TEMPERATURE_CONSTRAINT};
use super::{ImportRecord, ProjectError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const NUMBER_OF_GATES_CONSTRAINT: NumericConstraint = NumericConstraint {
    minimum: 1.0,
    maximum: 10.0,
    has_minimum: true,
    has_maximum: true,
    minimum_inclusive: true,
    maximum_inclusive: true,
    integer: true,
    unit: "",
};

/// 工程清单中的算法标识；执行引擎另行接入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateLocatorAlgorithm {
    AdvancedGateLocator,
}

impl GateLocatorAlgorithm {
    pub fn id(self) -> &'static str {
        "advanced-gate-locator"
    }
    pub fn source_text(self) -> &'static str {
        "Advanced gate locator"
    }
    pub fn from_id(id: &str) -> Result<Self, ProjectError> {
        match id {
            "advanced-gate-locator" => Ok(Self::AdvancedGateLocator),
            _ => Err(ProjectError::CommandInvalid(
                "unknown gate locator algorithm".to_owned(),
            )),
        }
    }
}

/// 温度单位为摄氏度；设备引用当前只支持默认设备。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateLocationSettings {
    pub machine_id: String,
    pub mold_temperature_celsius: f64,
    pub melt_temperature_celsius: f64,
    pub algorithm: GateLocatorAlgorithm,
    pub number_of_gates: u32,
}

impl Default for GateLocationSettings {
    fn default() -> Self {
        Self {
            machine_id: "default-machine".to_owned(),
            mold_temperature_celsius: 40.0,
            melt_temperature_celsius: 230.0,
            algorithm: GateLocatorAlgorithm::AdvancedGateLocator,
            number_of_gates: 1,
        }
    }
}

impl GateLocationSettings {
    pub fn machine_source_text(&self) -> &'static str {
        "Default machine"
    }

    pub fn validation_errors(&self) -> Vec<String> {
        self.validation_errors_for_gate_count(f64::from(self.number_of_gates))
    }

    /// 接收 f64 草稿值，使小数浇口数不会在 DTO 转换时被截断。
    pub fn validation_errors_for_gate_count(&self, number_of_gates: f64) -> Vec<String> {
        let mut errors = Vec::new();
        if self.machine_id != "default-machine" {
            errors.push("machineId".to_owned());
        }
        if !TEMPERATURE_CONSTRAINT.accepts(self.mold_temperature_celsius) {
            errors.push("moldTemperature".to_owned());
        }
        if !TEMPERATURE_CONSTRAINT.accepts(self.melt_temperature_celsius) {
            errors.push("meltTemperature".to_owned());
        }
        if !NUMBER_OF_GATES_CONSTRAINT.accepts(number_of_gates) {
            errors.push("numberOfGates".to_owned());
        }
        errors
    }

    fn validate(&self) -> Result<(), ProjectError> {
        if self.validation_errors().is_empty() {
            Ok(())
        } else {
            Err(ProjectError::CommandInvalid(
                "invalid gate location settings".to_owned(),
            ))
        }
    }
}

pub(super) fn validate_settings(
    imports: &[ImportRecord],
    settings: &BTreeMap<String, GateLocationSettings>,
) -> Result<(), ProjectError> {
    for (id, setting) in settings {
        if !imports.iter().any(|record| record.id == *id) {
            return Err(ProjectError::ManifestInvalid(format!(
                "gate location settings reference missing import {id}"
            )));
        }
        setting
            .validate()
            .map_err(|error| ProjectError::ManifestInvalid(error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GateLocationSettings, NUMBER_OF_GATES_CONSTRAINT};

    #[test]
    fn gate_count_constraint_requires_an_integer_in_the_closed_range() {
        assert!(NUMBER_OF_GATES_CONSTRAINT.accepts(1.0));
        assert!(NUMBER_OF_GATES_CONSTRAINT.accepts(10.0));
        assert!(!NUMBER_OF_GATES_CONSTRAINT.accepts(0.0));
        assert!(!NUMBER_OF_GATES_CONSTRAINT.accepts(11.0));
        assert!(!NUMBER_OF_GATES_CONSTRAINT.accepts(1.5));
    }

    #[test]
    fn gate_draft_validation_preserves_fractional_input() {
        let settings = GateLocationSettings::default();
        assert_eq!(
            settings.validation_errors_for_gate_count(1.5),
            ["numberOfGates"]
        );
        assert!(settings.validation_errors_for_gate_count(10.0).is_empty());
        assert_eq!(
            settings.validation_errors_for_gate_count(11.0),
            ["numberOfGates"]
        );
    }
}
