//! 结果场 sidecar 的只读访问（task 029）：按导入记录打开 `.presult`，
//! 核对文件摘要、绑定网格摘要与节点 / 单元计数后按字段读取。

use super::{ProjectError, ProjectService};
use panta_mesh::{FieldAssociation, FieldSummary, ResultAssetReader, ResultField};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
        let index = field_index(&reader, name, association)?;
        reader.read_field(index).map_err(result_error)
    }

    fn open_result_asset(
        &self,
        import_id: &str,
    ) -> Result<Option<ResultAssetReader>, ProjectError> {
        self.result_field_source(import_id)?
            .map(|source| source.open())
            .transpose()
    }

    /// 结果 sidecar 的只读句柄（不做文件 IO）：显示适配在后台线程打开、核对并读字段。
    /// 句柄共享服务的单字段缓存，拖动切面时同一字段不重复整字段读取。
    /// 多时间步导入时为最新一步（与 [`ProjectService::result_time_steps`] 的最后一项相同）。
    pub fn result_field_source(
        &self,
        import_id: &str,
    ) -> Result<Option<ResultFieldSource>, ProjectError> {
        self.result_source_with(import_id, |asset| asset.results.as_ref())
    }

    /// 导入记录的结果场时间步（task 033），时间不减；单步或无结果场时为空。
    pub fn result_time_steps(&self, import_id: &str) -> Result<Vec<ResultTimeStep>, ProjectError> {
        let (_, asset) = self.import_asset(import_id)?;
        Ok(asset
            .result_steps
            .iter()
            .map(|step| ResultTimeStep {
                time: step.time,
                label: step.label.clone(),
                field_count: step.results.field_count as usize,
            })
            .collect())
    }

    /// 第 `step` 个时间步（[`ProjectService::result_time_steps`] 的下标）的结果 sidecar 句柄。
    pub fn result_field_source_at(
        &self,
        import_id: &str,
        step: usize,
    ) -> Result<ResultFieldSource, ProjectError> {
        self.result_source_with(import_id, |asset| {
            asset.result_steps.get(step).map(|step| &step.results)
        })?
        .ok_or_else(|| {
            ProjectError::CommandInvalid(format!("result time step {step} does not exist"))
        })
    }

    fn import_asset(
        &self,
        import_id: &str,
    ) -> Result<(&super::ProjectState, &super::model::MeshAssetRef), ProjectError> {
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
        Ok((state, asset))
    }

    fn result_source_with(
        &self,
        import_id: &str,
        select: impl FnOnce(&super::model::MeshAssetRef) -> Option<&super::model::ResultAssetRef>,
    ) -> Result<Option<ResultFieldSource>, ProjectError> {
        let (state, asset) = self.import_asset(import_id)?;
        let Some(results) = select(asset) else {
            return Ok(None);
        };
        let root = state
            .path
            .parent()
            .ok_or_else(|| ProjectError::Io("project has no package directory".to_owned()))?;
        let path = super::service::resolve_asset_path(root, &results.relative_path)?;
        let counts = self
            .volume_meshes
            .get(import_id)
            // 节点场长度是来源节点数，不含多面体拆分追加的中心节点（task 031）。
            .map(|volume| (volume.source_node_count(), volume.cell_count()))
            .or_else(|| {
                self.streamed_volumes
                    .get(import_id)
                    .map(|counts| (counts.source_nodes(), counts.cells))
            });
        Ok(Some(ResultFieldSource {
            path,
            digest: results.digest.clone(),
            mesh_digest: asset.digest.clone(),
            field_count: results.field_count as usize,
            counts,
            cache: self.result_field_cache.clone(),
        }))
    }
}

/// 一个结果场时间步的摘要（task 033）。
#[derive(Clone, Debug, PartialEq)]
pub struct ResultTimeStep {
    pub time: f64,
    pub label: String,
    pub field_count: usize,
}

/// 服务级单字段缓存：键为（结果摘要、字段名、关联）。
pub(super) type ResultFieldCache = Arc<Mutex<Option<CachedField>>>;
type CachedField = (String, String, FieldAssociation, Arc<ResultField>);

/// 结果 sidecar 的定位与核对参数；`open` / `read_field` 做文件 IO，可在任意线程调用。
#[derive(Clone, Debug)]
pub struct ResultFieldSource {
    path: PathBuf,
    digest: String,
    mesh_digest: String,
    field_count: usize,
    counts: Option<(usize, usize)>,
    cache: ResultFieldCache,
}

impl ResultFieldSource {
    /// 打开 sidecar 并核对文件摘要、绑定网格摘要、字段数与节点 / 单元计数。
    pub fn open(&self) -> Result<ResultAssetReader, ProjectError> {
        let reader = ResultAssetReader::open(&self.path).map_err(result_error)?;
        if reader.digest() != self.digest
            || reader.mesh_digest() != self.mesh_digest
            || reader.summaries().len() != self.field_count
        {
            return Err(ProjectError::ManifestInvalid(
                "result asset does not match its mesh asset".to_owned(),
            ));
        }
        if let Some((nodes, cells)) = self.counts
            && (reader.node_count() != nodes as u64 || reader.cell_count() != cells as u64)
        {
            return Err(ProjectError::ManifestInvalid(
                "result asset counts do not match the volume mesh".to_owned(),
            ));
        }
        Ok(reader)
    }

    /// 读取一个字段（整字段顺序读）；与上次读取的字段相同时直接返回缓存。
    pub fn read_field(
        &self,
        name: &str,
        association: FieldAssociation,
    ) -> Result<Arc<ResultField>, ProjectError> {
        if let Some(field) = self.cached(name, association) {
            return Ok(field);
        }
        // 先放下旧字段，避免新旧两份同时驻留。
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
        let mut reader = self.open()?;
        let index = field_index(&reader, name, association)?;
        let field = Arc::new(reader.read_field(index).map_err(result_error)?);
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some((
                self.digest.clone(),
                name.to_owned(),
                association,
                field.clone(),
            ));
        }
        Ok(field)
    }

    fn cached(&self, name: &str, association: FieldAssociation) -> Option<Arc<ResultField>> {
        let cache = self.cache.lock().ok()?;
        let (digest, cached_name, cached_association, field) = cache.as_ref()?;
        (*digest == self.digest && cached_name == name && *cached_association == association)
            .then(|| field.clone())
    }
}

fn field_index(
    reader: &ResultAssetReader,
    name: &str,
    association: FieldAssociation,
) -> Result<usize, ProjectError> {
    reader
        .summaries()
        .iter()
        .position(|field| field.name == name && field.association == association)
        .ok_or_else(|| {
            ProjectError::CommandInvalid(format!(
                "result field {name} ({}) not found",
                association.as_str()
            ))
        })
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
        // 显示句柄共享单字段缓存：同字段复用，换字段后旧字段被替换。
        let source = service
            .result_field_source(&record.id)?
            .ok_or("result source")?;
        let velocity = source.read_field("velocity", FieldAssociation::Node)?;
        let again = source.read_field("velocity", FieldAssociation::Node)?;
        assert!(std::sync::Arc::ptr_eq(&velocity, &again));
        assert_eq!(
            source
                .read_field("material", FieldAssociation::Cell)?
                .values,
            [1.0, 2.0]
        );
        let reread = source.read_field("velocity", FieldAssociation::Node)?;
        assert!(!std::sync::Arc::ptr_eq(&velocity, &reread));
        assert_eq!(velocity.values, reread.values);
        assert!(matches!(
            source.read_field("missing", FieldAssociation::Node),
            Err(ProjectError::CommandInvalid(_))
        ));

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

    /// OpenFOAM 多区域算例（task 032）：按米导入、区域与边界组名写入记录并在重开后保留。
    #[test]
    fn openfoam_case_imports_regions_patches_and_fields() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture_dir = temp_directory::Fixture::new()?;
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mesh/openfoam/multi/multi.foam");
        let mut service = ProjectService::new();
        let project = service.create(&fixture_dir.root, "Foam")?;
        let record = service.import_stl(&source, "solid-3d", "meters", false)?;
        assert_eq!(record.format, "openfoam");
        assert_eq!(record.display_name, "multi");
        assert_eq!(record.units, "meters");
        assert_eq!(record.region_names, ["fluid", "solid"]);
        // 正交 blockMesh：两区域合并后无非正交（task 032 PR 2）。
        let face = record.face_quality.ok_or("face quality")?;
        assert!(face.internal_faces > 0 && face.max_non_orthogonality_degrees < 1e-6);
        assert_eq!(face.severe_non_orthogonal_faces + face.high_skew_faces, 0);
        assert!(
            record
                .boundary_group_names
                .iter()
                .any(|name| name == "solid/solid_to_fluid")
        );
        // 记录尺寸沿用来源单位（与其他格式一致），界面按 `units` 显示为 m。
        assert_eq!(record.dimensions, [0.1, 0.1, 0.01]);
        let summaries = service.result_fields_for_import(&record.id)?;
        let temperature = summaries
            .iter()
            .find(|field| field.name == "T")
            .ok_or("missing T")?;
        assert_eq!(temperature.association, FieldAssociation::Cell);
        assert_eq!(temperature.units, "K");
        assert_eq!(temperature.range, Some([300.0, 400.0]));
        let mut reopened = ProjectService::new();
        reopened.open(&project.path)?;
        assert_eq!(reopened.imports()?, vec![record]);
        Ok(())
    }

    /// 多时间步 OpenFOAM 算例（task 033）：每步一个 sidecar，最新步与 `results` 共用；
    /// 重开后时间步保留，单个时间步文件损坏只影响该步。
    #[test]
    fn transient_openfoam_case_keeps_every_time_step() -> Result<(), Box<dyn std::error::Error>> {
        let fixture_dir = temp_directory::Fixture::new()?;
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mesh/openfoam/transient/transient.foam");
        let mut service = ProjectService::new();
        let project = service.create(&fixture_dir.root, "Transient")?;
        let record = service.import_stl(&source, "solid-3d", "meters", false)?;
        assert!(
            record.skipped_result_fields.is_empty(),
            "{:?}",
            record.skipped_result_fields
        );
        let steps = service.result_time_steps(&record.id)?;
        let labels: Vec<(f64, &str, usize)> = steps
            .iter()
            .map(|step| (step.time, step.label.as_str(), step.field_count))
            .collect();
        assert_eq!(
            labels,
            [
                (0.0, "0", 2),
                (0.005, "0.005", 2),
                (0.01, "0.01", 2),
                (0.015, "0.015", 2)
            ]
        );
        let first = service.result_field_source_at(&record.id, 0)?;
        let velocity = first.read_field("U", FieldAssociation::Cell)?;
        assert!(velocity.values.iter().all(|value| *value == 0.0));
        let latest = service
            .result_field_source(&record.id)?
            .ok_or("latest source")?
            .read_field("U", FieldAssociation::Cell)?;
        let last = service
            .result_field_source_at(&record.id, 3)?
            .read_field("U", FieldAssociation::Cell)?;
        assert_eq!(latest.values, last.values);
        assert!(service.result_field_source_at(&record.id, 4).is_err());
        let root = project.path.parent().ok_or("project root")?;
        let asset = service
            .current
            .as_ref()
            .and_then(|state| state.mesh_assets.get(&record.mesh_asset_id))
            .cloned()
            .ok_or("asset")?;
        let paths: Vec<&str> = asset
            .result_steps
            .iter()
            .map(|step| step.results.relative_path.as_str())
            .collect();
        let id = &record.mesh_asset_id;
        assert_eq!(
            paths,
            [
                format!("assets/results/{id}-r1-s00000.presult"),
                format!("assets/results/{id}-r1-s00001.presult"),
                format!("assets/results/{id}-r1-s00002.presult"),
                format!("assets/results/{id}-r1.presult"),
            ]
        );

        let mut reopened = ProjectService::new();
        reopened.open(&project.path)?;
        assert_eq!(reopened.result_time_steps(&record.id)?, steps);
        let middle = reopened
            .result_field_source_at(&record.id, 1)?
            .read_field("p", FieldAssociation::Cell)?;
        assert!(middle.values.iter().any(|value| *value != 0.0));

        // 第 2 步 sidecar 被改写：该步无法打开，其余步不受影响。
        let tampered = root.join(paths[1]);
        let reader = panta_mesh::ResultAssetReader::open(&tampered)?;
        let index = reader
            .summaries()
            .iter()
            .position(|field| field.name == "U")
            .ok_or("U summary")?;
        let range = reader.field_byte_range(index).ok_or("U byte range")?;
        drop(reader);
        let mut bytes = std::fs::read(&tampered)?;
        bytes[usize::try_from(range.start)?] ^= 0xff;
        std::fs::write(&tampered, bytes)?;
        assert!(
            reopened
                .result_field_source_at(&record.id, 1)?
                .read_field("U", FieldAssociation::Cell)
                .is_err()
        );
        reopened
            .result_field_source_at(&record.id, 2)?
            .read_field("U", FieldAssociation::Cell)?;
        Ok(())
    }

    type StepEdit = Box<dyn Fn(&mut serde_json::Value)>;

    /// 清单中的时间步必须时间不减、最后一步与 `results` 一致；否则整个清单被拒绝。
    #[test]
    fn invalid_result_steps_reject_the_manifest() -> Result<(), Box<dyn std::error::Error>> {
        let fixture_dir = temp_directory::Fixture::new()?;
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mesh/openfoam/transient/transient.foam");
        let mut service = ProjectService::new();
        let project = service.create(&fixture_dir.root, "Steps")?;
        let record = service.import_stl(&source, "solid-3d", "meters", false)?;
        let original = std::fs::read(&project.path)?;
        let manifest: serde_json::Value = serde_json::from_slice(&original)?;
        let id = record.mesh_asset_id.clone();
        let edits: [(&str, StepEdit); 4] = [
            (
                "decreasing time",
                Box::new(|steps| steps[0]["time"] = serde_json::Value::from(1.0)),
            ),
            (
                "last step differs from results",
                Box::new(|steps| {
                    let first = steps[0]["results"].clone();
                    steps[3]["results"] = first;
                }),
            ),
            (
                "duplicate label",
                Box::new(|steps| steps[1]["label"] = steps[0]["label"].clone()),
            ),
            (
                "single step",
                Box::new(|steps| {
                    let last = steps[3].clone();
                    *steps = serde_json::Value::Array(vec![last]);
                }),
            ),
        ];
        for (name, edit) in edits {
            let mut changed = manifest.clone();
            edit(&mut changed["mesh_assets"][&id]["result_steps"]);
            std::fs::write(&project.path, serde_json::to_vec_pretty(&changed)?)?;
            let mut reopened = ProjectService::new();
            assert!(
                matches!(
                    reopened.open(&project.path),
                    Err(ProjectError::ManifestInvalid(_))
                ),
                "{name}"
            );
        }
        std::fs::write(&project.path, &original)?;
        ProjectService::new().open(&project.path)?;
        Ok(())
    }

    /// 单个时间目录的算例不写 `result_steps`，清单与 032 相同。
    #[test]
    fn single_step_imports_omit_result_steps() -> Result<(), Box<dyn std::error::Error>> {
        let fixture_dir = temp_directory::Fixture::new()?;
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mesh/openfoam/multi/multi.foam");
        let mut service = ProjectService::new();
        let project = service.create(&fixture_dir.root, "Single")?;
        let record = service.import_stl(&source, "solid-3d", "meters", false)?;
        assert!(service.result_time_steps(&record.id)?.is_empty());
        assert!(service.result_field_source_at(&record.id, 0).is_err());
        let manifest = std::fs::read_to_string(&project.path)?;
        assert!(!manifest.contains("result_steps"));
        let results =
            std::fs::read_dir(project.path.parent().ok_or("root")?.join("assets/results"))?
                .filter_map(Result::ok)
                .count();
        assert_eq!(results, 1);
        Ok(())
    }

    /// CGNS 时间步（task 033）经导入后成为 3 个 sidecar；缺解 Zone 的值保留为 NaN。
    #[test]
    fn transient_cgns_import_keeps_time_steps() -> Result<(), Box<dyn std::error::Error>> {
        let fixture_dir = temp_directory::Fixture::new()?;
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mesh/cgns/transient.cgns");
        let mut service = ProjectService::new();
        service.create(&fixture_dir.root, "Cgns")?;
        let record = service.import_stl(&source, "solid-3d", "meters", false)?;
        let steps = service.result_time_steps(&record.id)?;
        let times: Vec<f64> = steps.iter().map(|step| step.time).collect();
        assert_eq!(times, [0.0, 0.5, 1.0]);
        let middle = service
            .result_field_source_at(&record.id, 1)?
            .read_field("T", FieldAssociation::Cell)?;
        assert_eq!(middle.values[0], 100.0);
        assert!(middle.values[1].is_nan());
        Ok(())
    }
}
