//! schema 4 材料引用使用的内置资源身份与读取校验。
use super::{ProjectError, ProjectManifest};
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct MaterialIdentity {
    schema: String,
    id: String,
}

static MATERIAL_ID: LazyLock<Result<String, String>> = LazyLock::new(|| {
    parse_material(include_str!(
        "../../../../resources/materials/pp-mineral-25.json"
    ))
});

fn parse_material(source: &str) -> Result<String, String> {
    let data: MaterialIdentity = serde_json::from_str(source)
        .map_err(|error| format!("invalid built-in material: {error}"))?;
    if data.schema != "panta.material-preview/v1" {
        return Err("unsupported built-in material schema".to_owned());
    }
    Ok(data.id)
}

/// 返回内置材料的稳定引用 ID，资源解析只发生一次。
pub fn default_material_id() -> Result<&'static str, ProjectError> {
    MATERIAL_ID
        .as_ref()
        .map(String::as_str)
        .map_err(|error| ProjectError::ManifestInvalid(error.clone()))
}

pub(super) fn validate_materials(manifest: &ProjectManifest) -> Result<(), ProjectError> {
    for (id, material) in &manifest.materials {
        if !manifest.imports.iter().any(|record| record.id == *id)
            || material != default_material_id()?
        {
            return Err(ProjectError::ManifestInvalid(format!(
                "invalid material for {id}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_material;

    #[test]
    fn malformed_material_reports_a_parse_error() -> Result<(), Box<dyn std::error::Error>> {
        let error = parse_material("{")
            .err()
            .ok_or("malformed material should be rejected")?;
        assert!(error.starts_with("invalid built-in material:"));
        Ok(())
    }

    #[test]
    fn unsupported_material_schema_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let mut source: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../resources/materials/pp-mineral-25.json"
        ))?;
        source["schema"] = serde_json::Value::String("panta.material-preview/v2".to_owned());

        assert_eq!(
            parse_material(&source.to_string()).err().as_deref(),
            Some("unsupported built-in material schema")
        );
        Ok(())
    }
}
