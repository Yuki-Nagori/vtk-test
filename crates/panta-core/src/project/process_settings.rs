//! schema 4 清单的 Fill 工艺配置、默认值与读取校验。
use super::{ImportRecord, ProjectError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// QML 与领域校验共同使用的数值约束元数据。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumericConstraint {
    pub minimum: f64,
    pub maximum: f64,
    pub has_minimum: bool,
    pub has_maximum: bool,
    pub minimum_inclusive: bool,
    pub maximum_inclusive: bool,
    pub integer: bool,
    pub unit: &'static str,
}

impl NumericConstraint {
    pub fn accepts(self, value: f64) -> bool {
        value.is_finite()
            && (!self.has_minimum
                || if self.minimum_inclusive {
                    value >= self.minimum
                } else {
                    value > self.minimum
                })
            && (!self.has_maximum
                || if self.maximum_inclusive {
                    value <= self.maximum
                } else {
                    value < self.maximum
                })
            && (!self.integer || value.fract() == 0.0)
    }
}

pub const TEMPERATURE_CONSTRAINT: NumericConstraint = NumericConstraint {
    minimum: -273.15,
    maximum: 0.0,
    has_minimum: true,
    has_maximum: false,
    minimum_inclusive: true,
    maximum_inclusive: false,
    integer: false,
    unit: "°C",
};
pub const FLOW_RATE_CONSTRAINT: NumericConstraint = NumericConstraint {
    minimum: 0.0,
    maximum: 0.0,
    has_minimum: true,
    has_maximum: false,
    minimum_inclusive: false,
    maximum_inclusive: false,
    integer: false,
    unit: "cm³/s",
};
pub const SWITCH_OVER_VOLUME_CONSTRAINT: NumericConstraint = NumericConstraint {
    minimum: 0.0,
    maximum: 100.0,
    has_minimum: true,
    has_maximum: true,
    minimum_inclusive: true,
    maximum_inclusive: true,
    integer: false,
    unit: "%",
};
pub const HOLDING_DURATION_CONSTRAINT: NumericConstraint = NumericConstraint {
    minimum: 0.0,
    maximum: 0.0,
    has_minimum: true,
    has_maximum: false,
    minimum_inclusive: true,
    maximum_inclusive: false,
    integer: false,
    unit: "s",
};
pub const HOLDING_PRESSURE_CONSTRAINT: NumericConstraint = NumericConstraint {
    minimum: 0.0,
    maximum: 200.0,
    has_minimum: true,
    has_maximum: true,
    minimum_inclusive: true,
    maximum_inclusive: true,
    integer: false,
    unit: "%",
};
pub const HOLDING_PROFILE_MAX_POINTS: usize = 4096;

/// 分段持续时间（秒）和填充压力百分比；时间累计用于曲线横轴。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldingProfilePoint {
    pub duration_seconds: f64,
    pub pressure_percent: f64,
}

/// Fill 的已支持控制方式：流量填充、体积百分比切换和相对压力保压。
/// 字段名携带单位，界面无需推断工程清单里的量纲。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FillSettings {
    pub mold_temperature_celsius: f64,
    pub melt_temperature_celsius: f64,
    pub flow_rate_cm3_per_second: f64,
    pub switch_over_volume_percent: f64,
    pub fiber_orientation: bool,
    pub crystallization: bool,
    pub holding_profile: Vec<HoldingProfilePoint>,
}

impl Default for FillSettings {
    fn default() -> Self {
        Self {
            mold_temperature_celsius: 40.0,
            melt_temperature_celsius: 230.0,
            flow_rate_cm3_per_second: 94.7,
            switch_over_volume_percent: 99.0,
            fiber_orientation: true,
            crystallization: false,
            holding_profile: vec![
                HoldingProfilePoint {
                    duration_seconds: 0.0,
                    pressure_percent: 100.0,
                },
                HoldingProfilePoint {
                    duration_seconds: 10.0,
                    pressure_percent: 100.0,
                },
            ],
        }
    }
}

impl FillSettings {
    /// 按稳定字段键报告草稿错误；提交校验复用同一结果。
    pub fn validation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !TEMPERATURE_CONSTRAINT.accepts(self.mold_temperature_celsius) {
            errors.push("moldTemperature".to_owned());
        }
        if !TEMPERATURE_CONSTRAINT.accepts(self.melt_temperature_celsius) {
            errors.push("meltTemperature".to_owned());
        }
        if !FLOW_RATE_CONSTRAINT.accepts(self.flow_rate_cm3_per_second) {
            errors.push("flowRate".to_owned());
        }
        if !SWITCH_OVER_VOLUME_CONSTRAINT.accepts(self.switch_over_volume_percent) {
            errors.push("switchVolume".to_owned());
        }
        // 曲线有明确上限，避免元数据和 UI 快照随输入无限增长。
        if self.holding_profile.is_empty()
            || self.holding_profile.len() > HOLDING_PROFILE_MAX_POINTS
        {
            errors.push("holdingProfile".to_owned());
        }
        for (index, point) in self.holding_profile.iter().enumerate() {
            if !HOLDING_DURATION_CONSTRAINT.accepts(point.duration_seconds) {
                errors.push(format!("holdingProfile.{index}.duration"));
            }
            if !HOLDING_PRESSURE_CONSTRAINT.accepts(point.pressure_percent) {
                errors.push(format!("holdingProfile.{index}.pressure"));
            }
        }
        if !self
            .holding_profile
            .iter()
            .map(|point| point.duration_seconds)
            .sum::<f64>()
            .is_finite()
        {
            errors.push("holdingProfile".to_owned());
        }
        errors
    }

    fn validate(&self) -> Result<(), ProjectError> {
        if self.validation_errors().is_empty() {
            Ok(())
        } else {
            Err(ProjectError::CommandInvalid(
                "invalid fill process settings".to_owned(),
            ))
        }
    }
}

pub(super) fn validate_settings(
    imports: &[ImportRecord],
    settings: &BTreeMap<String, FillSettings>,
) -> Result<(), ProjectError> {
    for (id, setting) in settings {
        if !imports.iter().any(|record| record.id == *id) {
            return Err(ProjectError::ManifestInvalid(format!(
                "fill settings reference missing import {id}"
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
    use super::{
        FLOW_RATE_CONSTRAINT, FillSettings, HOLDING_DURATION_CONSTRAINT,
        HOLDING_PRESSURE_CONSTRAINT, HoldingProfilePoint, SWITCH_OVER_VOLUME_CONSTRAINT,
        TEMPERATURE_CONSTRAINT,
    };

    #[test]
    fn process_numeric_constraints_accept_closed_edges_and_reject_open_flow_edge() {
        assert!(TEMPERATURE_CONSTRAINT.accepts(-273.15));
        assert!(!TEMPERATURE_CONSTRAINT.accepts(-273.150_001));
        assert!(!FLOW_RATE_CONSTRAINT.accepts(0.0));
        assert!(FLOW_RATE_CONSTRAINT.accepts(f64::MIN_POSITIVE));
        assert!(SWITCH_OVER_VOLUME_CONSTRAINT.accepts(0.0));
        assert!(SWITCH_OVER_VOLUME_CONSTRAINT.accepts(100.0));
        assert!(!SWITCH_OVER_VOLUME_CONSTRAINT.accepts(100.000_001));
        assert!(HOLDING_DURATION_CONSTRAINT.accepts(0.0));
        assert!(HOLDING_PRESSURE_CONSTRAINT.accepts(200.0));
        assert!(!HOLDING_PRESSURE_CONSTRAINT.accepts(200.000_001));
    }

    #[test]
    fn fill_draft_errors_are_per_field_and_match_domain_validation() {
        let mut profile = FillSettings::default().holding_profile;
        profile[0].pressure_percent = 201.0;
        let settings = FillSettings {
            flow_rate_cm3_per_second: 0.0,
            holding_profile: profile,
            ..FillSettings::default()
        };

        assert_eq!(
            settings.validation_errors(),
            ["flowRate", "holdingProfile.0.pressure"]
        );
        assert!(settings.validate().is_err());

        let valid = FillSettings {
            flow_rate_cm3_per_second: 1.0,
            holding_profile: vec![
                HoldingProfilePoint {
                    duration_seconds: 0.0,
                    pressure_percent: 200.0,
                },
                HoldingProfilePoint {
                    duration_seconds: 10.0,
                    pressure_percent: 100.0,
                },
            ],
            ..FillSettings::default()
        };
        assert!(valid.validation_errors().is_empty());
        assert!(valid.validate().is_ok());
    }
}
