//! 结果场 sidecar 的只读访问（task 029）：按导入记录打开 `.presult`，
//! 核对文件摘要、绑定网格摘要与节点 / 单元计数后按字段读取。

use super::{ProjectError, ProjectService};
use panta_mesh::{FieldAssociation, FieldSummary, ResultAssetReader, ResultField};

impl ProjectService {
    /// 导入记录携带的结果场摘要（只读 sidecar 头部与字段表）；无结果场返回空表。
    pub fn result_fields_for_import(
        &self,
        import_id: &str,
    ) -> Result<Vec<FieldSummary>, ProjectError> {
        Ok(self
            .open_result_asset(import_id)?
            .map(|reader| reader.summaries().to_vec())
            .unwrap_or_default())
    }

    /// 读取一个结果场的全部数值（f64，下标与网格节点 / 单元一致）。
    pub fn read_result_field(
        &self,
        import_id: &str,
        name: &str,
        association: FieldAssociation,
    ) -> Result<ResultField, ProjectError> {
        let mut reader = self.open_result_asset(import_id)?.ok_or_else(|| {
            ProjectError::CommandInvalid("import has no result fields".to_owned())
        })?;
        let index = reader
            .summaries()
            .iter()
            .position(|field| field.name == name && field.association == association)
            .ok_or_else(|| {
                ProjectError::CommandInvalid(format!(
                    "result field {name} ({}) not found",
                    association.as_str()
                ))
            })?;
        reader.read_field(index).map_err(result_error)
    }

    fn open_result_asset(
        &self,
        import_id: &str,
    ) -> Result<Option<ResultAssetReader>, ProjectError> {
        let state = self.current.as_ref().ok_or(ProjectError::NoProject)?;
        let record = state
            .imports
            .iter()
            .find(|record| record.id == import_id)
            .ok_or_else(|| ProjectError::ImportRecordMissing(import_id.to_owned()))?;
        let asset = state
            .mesh_assets
            .get(&record.mesh_asset_id)
            .ok_or_else(|| {
                ProjectError::ManifestInvalid("mesh asset index entry is missing".to_owned())
            })?;
        let Some(results) = &asset.results else {
            return Ok(None);
        };
        let root = state
            .path
            .parent()
            .ok_or_else(|| ProjectError::Io("project has no package directory".to_owned()))?;
        let path = super::service::resolve_asset_path(root, &results.relative_path)?;
        let reader = ResultAssetReader::open(&path).map_err(result_error)?;
        if reader.digest() != results.digest
            || reader.mesh_digest() != asset.digest
            || reader.summaries().len() != results.field_count as usize
        {
            return Err(ProjectError::ManifestInvalid(
                "result asset does not match its mesh asset".to_owned(),
            ));
        }
        let counts = self
            .volume_meshes
            .get(import_id)
            .map(|volume| (volume.nodes.len(), volume.tets.len()))
            .or_else(|| {
                self.streamed_volumes
                    .get(import_id)
                    .map(|counts| (counts.nodes, counts.tets))
            });
        if let Some((nodes, cells)) = counts
            && (reader.node_count() != nodes as u64 || reader.cell_count() != cells as u64)
        {
            return Err(ProjectError::ManifestInvalid(
                "result asset counts do not match the volume mesh".to_owned(),
            ));
        }
        Ok(Some(reader))
    }
}

fn result_error(error: panta_mesh::MeshAssetError) -> ProjectError {
    ProjectError::Io(format!("result asset: {error}"))
}

#[cfg(test)]
mod tests {
    use crate::project::{ProjectError, ProjectService, temp_directory};
    use panta_mesh::FieldAssociation;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mesh/formats")
            .join(name)
    }

    fn write_grid_with_fields(path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(
            path,
            "# vtk DataFile Version 4.2\nresults\nASCII\nDATASET UNSTRUCTURED_GRID\n\
             POINTS 5 double\n0 0 0 1 0 0 0 1 0 0 0 1 1 1 1\n\
             CELLS 2 10\n4 0 1 2 3\n4 1 2 3 4\nCELL_TYPES 2\n10 10\n\
             POINT_DATA 5\nSCALARS temperature double\nLOOKUP_TABLE default\n300 nan 310 320 330\n\
             VECTORS velocity float\n1 0 0 0 2 0 0 0 3 0 0 0 1 1 1\n\
             TENSORS stress double\n1 0 0 0 1 0 0 0 1 1 0 0 0 1 0 0 0 1 1 0 0 0 1 0 0 0 1 \
             1 0 0 0 1 0 0 0 1 1 0 0 0 1 0 0 0 1\n\
             CELL_DATA 2\nSCALARS material int\nLOOKUP_TABLE default\n1 2\n",
        )
    }

    #[test]
    fn volume_import_keeps_result_fields_across_reopen() -> Result<(), Box<dyn std::error::Error>> {
        let fixture_dir = temp_directory::Fixture::new()?;
        let source = fixture_dir.root.join("results.vtk");
        write_grid_with_fields(&source)?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture_dir.root, "Results")?;
        let record = service.import_stl(&source, "solid-3d", "millimeters", false)?;
        assert_eq!(record.skipped_result_fields.len(), 1);
        assert!(record.skipped_result_fields[0].starts_with("stress (node): TENSORS"));
        let summaries = service.result_fields_for_import(&record.id)?;
        let names: Vec<_> = summaries
            .iter()
            .map(|field| (field.name.as_str(), field.association, field.missing))
            .collect();
        assert_eq!(
            names,
            [
                ("temperature", FieldAssociation::Node, 1),
                ("velocity", FieldAssociation::Node, 0),
                ("material", FieldAssociation::Cell, 0),
            ]
        );
        assert_eq!(summaries[0].range, Some([300.0, 330.0]));
        assert_eq!(summaries[1].range, Some([0.0, 3.0]));

        let mut reopened = ProjectService::new();
        reopened.open(&project.path)?;
        assert_eq!(reopened.imports()?, vec![record.clone()]);
        let material =
            reopened.read_result_field(&record.id, "material", FieldAssociation::Cell)?;
        assert_eq!(material.values, [1.0, 2.0]);
        let temperature =
            reopened.read_result_field(&record.id, "temperature", FieldAssociation::Node)?;
        assert!(temperature.values[1].is_nan());
        assert!(matches!(
            reopened.read_result_field(&record.id, "material", FieldAssociation::Node),
            Err(ProjectError::CommandInvalid(_))
        ));
        // 激活请求带结果场引用（派生分块带场构建）；驻留切面可按字段着色。
        let request = reopened.activation_request(&record.id)?;
        assert!(
            request
                .results
                .is_some_and(|results| results.relative_path.ends_with(".presult"))
        );
        let volume = service
            .volume_for_import(&record.id)
            .ok_or("volume missing")?;
        let section = crate::project::section_plane_field(
            &volume,
            &material,
            panta_mesh::FieldComponent::Magnitude,
            [1.0, 1.0, 1.0],
            0.5,
            &panta_mesh::Progress::new(),
        )?
        .ok_or("cancelled")?;
        assert!(!section.values.is_empty());
        assert!(
            section
                .values
                .iter()
                .flatten()
                .all(|value| *value == 1.0 || *value == 2.0)
        );

        // sidecar 被改写（摘要不符）时拒绝读取，网格本身不受影响。
        let results = service
            .current
            .as_ref()
            .and_then(|state| state.mesh_assets.get(&record.mesh_asset_id))
            .and_then(|asset| asset.results.clone())
            .ok_or("result asset reference missing")?;
        assert_eq!(results.field_count, 3);
        let path = project
            .path
            .parent()
            .ok_or("project root")?
            .join(&results.relative_path);
        let range = panta_mesh::ResultAssetReader::open(&path)?
            .field_byte_range(2)
            .ok_or("material byte range")?;
        let mut bytes = std::fs::read(&path)?;
        bytes[usize::try_from(range.start)?] ^= 0xff;
        std::fs::write(&path, bytes)?;
        // 打开只读头部与字段表；损坏的块在读取该字段时才被发现，其他字段不受影响。
        assert_eq!(reopened.result_fields_for_import(&record.id)?.len(), 3);
        assert!(matches!(
            reopened.read_result_field(&record.id, "material", FieldAssociation::Cell),
            Err(ProjectError::Io(_))
        ));
        reopened.read_result_field(&record.id, "velocity", FieldAssociation::Node)?;
        Ok(())
    }

    #[test]
    fn surface_imports_have_no_results_and_fixture_normals_are_kept()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture_dir = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        service.create(&fixture_dir.root, "No results")?;
        let surface = service.import_stl(
            &fixture("tet_ball_ascii.vtu"),
            "dual-domain",
            "millimeters",
            false,
        )?;
        assert!(service.result_fields_for_import(&surface.id)?.is_empty());
        let volume = service.import_stl(
            &fixture("tet_ball_ascii.vtu"),
            "solid-3d",
            "millimeters",
            false,
        )?;
        let fields = service.result_fields_for_import(&volume.id)?;
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "Normals");
        assert!(
            !fixture_dir
                .root
                .join("No results/assets/results")
                .read_dir()?
                .any(|entry| {
                    entry.is_ok_and(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                })
        );
        Ok(())
    }
}
