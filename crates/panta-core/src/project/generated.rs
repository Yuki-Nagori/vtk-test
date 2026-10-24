//! STEP → Netgen 体网格生成结果的工程事务（task 023）。
//!
//! 网格由 native 适配器生成；本模块只负责来源 STEP 摘要、工程记录（来源种类“生成”）
//! 与 Mesh 资产提交。提交复用导入的待处理槽位、取消闸门与 `finish_stl_import`，
//! 因此进度、取消和发布语义与文件导入一致。
use super::import::{
    IMPORT_RUNNING, ImportCommit, PendingImport, commit_mesh_record, import_cancelled,
};
use super::model::next_revision;
use super::{ImportRecord, ProjectError, ProjectService, VolumeGeneration};
use panta_mesh::{Progress, TetMesh};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, mpsc};

/// 生成器稳定 ID。
pub const NETGEN_GENERATOR: &str = "netgen";
/// 生成记录的格式标记；来源文件不复制，只保存名称与摘要。
pub(super) const GENERATED_FORMAT: &str = "step";
const GENERATED_MESH_TYPE: &str = "solid-3d";
/// 最大单元尺寸上限（毫米）；超过时视为参数错误。
const MAX_LENGTH_LIMIT_MM: f64 = 1.0e6;

impl ProjectService {
    /// 启动生成体网格的提交事务。`volume` 已由 native 生成并通过 Mesh IR 校验；
    /// 来源 STEP 摘要、资产写出与清单提交在写入 worker 执行，结果由
    /// `finish_generated_volume` 发布。
    pub fn begin_generated_volume(
        &mut self,
        source_step: &Path,
        max_length_mm: f64,
        volume: TetMesh,
    ) -> Result<u64, ProjectError> {
        self.ensure_project_writable()?;
        if self.pending_import.is_some() || self.pending_preview.is_some() {
            return Err(ProjectError::CommandInvalid(
                "another import is pending".to_owned(),
            ));
        }
        if !max_length_mm.is_finite() || !(0.0..=MAX_LENGTH_LIMIT_MM).contains(&max_length_mm) {
            return Err(ProjectError::VolumeGenerationFailed(format!(
                "max length {max_length_mm} mm outside 0..={MAX_LENGTH_LIMIT_MM}"
            )));
        }
        validate_volume(&volume)?;
        let current = self.current.as_ref().ok_or(ProjectError::NoProject)?;
        let import_number = current.imports.len() + 1;
        let revision = next_revision(current.revision)?;
        let request = self
            .import_request
            .checked_add(1)
            .ok_or_else(|| ProjectError::CommandInvalid("import request exhausted".to_owned()))?;
        let mut candidate = current.clone();
        candidate.revision = revision;
        candidate.dirty = false;

        let phase = Arc::new(AtomicU8::new(IMPORT_RUNNING));
        let worker_phase = Arc::clone(&phase);
        let progress = Arc::new(Progress::new());
        let worker_progress = Arc::clone(&progress);
        let source = source_step.to_path_buf();
        let (sender, receiver) = mpsc::channel();
        self.execution
            .writes
            .submit(move || {
                let result = commit_generated(
                    &mut candidate,
                    &source,
                    max_length_mm,
                    volume,
                    import_number,
                    &worker_phase,
                    &worker_progress,
                );
                if let Err(mpsc::SendError(Ok(_))) = sender.send(result) {
                    eprintln!("panta: generated volume committed after service teardown");
                }
            })
            .map_err(|error| {
                ProjectError::Io(format!("start volume generation commit: {error}"))
            })?;
        self.import_request = request;
        self.pending_import = Some(PendingImport {
            request,
            phase,
            progress,
            receiver,
        });
        Ok(request)
    }

    /// `None` 表示仍在提交；与 `finish_stl_import` 相同的发布语义。
    pub fn finish_generated_volume(
        &mut self,
        request: u64,
    ) -> Result<Option<ImportRecord>, ProjectError> {
        self.finish_stl_import(request)
    }

    /// 同步入口，供无 GUI 的调用方与测试使用。
    pub fn generate_volume_record(
        &mut self,
        source_step: &Path,
        max_length_mm: f64,
        volume: TetMesh,
    ) -> Result<ImportRecord, ProjectError> {
        let request = self.begin_generated_volume(source_step, max_length_mm, volume)?;
        loop {
            if let Some(record) = self.finish_generated_volume(request)? {
                return Ok(record);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

fn validate_volume(volume: &TetMesh) -> Result<(), ProjectError> {
    if volume.tets.is_empty() || volume.boundary.is_empty() {
        return Err(ProjectError::VolumeGenerationFailed(
            "generated mesh has no tetrahedra or boundary".to_owned(),
        ));
    }
    let report = panta_mesh::validate_tet_mesh(volume);
    if report.issues.is_empty() {
        Ok(())
    } else {
        Err(ProjectError::VolumeGenerationFailed(
            report.issues.join("; "),
        ))
    }
}

fn commit_generated(
    candidate: &mut super::ProjectState,
    source: &Path,
    max_length_mm: f64,
    volume: TetMesh,
    import_number: usize,
    phase: &AtomicU8,
    progress: &Progress,
) -> Result<ImportCommit, ProjectError> {
    progress.set_fraction(0.0);
    let (digest, size, modified) = step_digest(source, phase, progress)?;
    let source_name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mesh = panta_mesh::boundary_surface(&volume);
    let summary = mesh.summary();
    let record = ImportRecord {
        record_version: super::IMPORT_RECORD_VERSION,
        parser_version: 0,
        id: format!("import-{import_number}"),
        display_name: step_display_name(&source_name),
        source_name: source_name.clone(),
        source_path: source.to_string_lossy().into_owned(),
        source_digest: Some(digest.clone()),
        source_size_bytes: size,
        source_modified_unix_ms: modified,
        mesh_asset_id: format!("mesh-import-{import_number}"),
        format: GENERATED_FORMAT.to_owned(),
        mesh_type: GENERATED_MESH_TYPE.to_owned(),
        units: "millimeters".to_owned(),
        show_import_log: false,
        triangle_count: summary.triangle_count,
        dimensions: summary.dimensions,
        generation: Some(VolumeGeneration {
            generator: NETGEN_GENERATOR.to_owned(),
            source_step_name: source_name,
            source_step_digest: digest,
            max_length_mm,
        }),
        skipped_result_fields: Vec::new(),
        region_names: Vec::new(),
        boundary_group_names: Vec::new(),
    };
    if phase.load(Ordering::Acquire) != IMPORT_RUNNING {
        return Err(import_cancelled());
    }
    progress.set_fraction(0.2);
    commit_mesh_record(
        candidate,
        record,
        mesh,
        super::import::MeshPayload {
            volume: Some(volume),
            fields: Vec::new(),
        },
        phase,
        progress,
        0.2,
    )
}

/// 默认资产名去掉 `.step` / `.stp` 扩展名（大小写不敏感）。
fn step_display_name(source_name: &str) -> String {
    source_name
        .rsplit_once('.')
        .filter(|(_, suffix)| {
            suffix.eq_ignore_ascii_case("step") || suffix.eq_ignore_ascii_case("stp")
        })
        .map_or_else(|| source_name.to_owned(), |(stem, _)| stem.to_owned())
}

/// 流式计算来源 STEP 的 SHA-256（0–20% 进度）；来源只读取、不复制。
fn step_digest(
    source: &Path,
    phase: &AtomicU8,
    progress: &Progress,
) -> Result<(String, u64, Option<u64>), ProjectError> {
    use sha2::{Digest, Sha256};
    let unreadable =
        |error: std::io::Error| ProjectError::VolumeGenerationFailed(format!("read STEP: {error}"));
    let mut file = std::fs::File::open(source).map_err(unreadable)?;
    let metadata = file.metadata().map_err(unreadable)?;
    let total = metadata.len();
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok());
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 20];
    let mut done = 0_u64;
    loop {
        if phase.load(Ordering::Acquire) != IMPORT_RUNNING {
            return Err(import_cancelled());
        }
        let read = file.read(&mut buffer).map_err(unreadable)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        done += read as u64;
        progress.report(0.0, 0.2, done, total.max(1));
    }
    Ok((format!("sha256:{:x}", hasher.finalize()), done, modified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::temp_directory;
    use panta_mesh::{SurfaceTriangle, Tetrahedron};

    fn unit_tet() -> TetMesh {
        TetMesh {
            nodes: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            tets: vec![Tetrahedron {
                nodes: [0, 1, 2, 3],
                region: 0,
            }],
            boundary: vec![
                SurfaceTriangle {
                    nodes: [0, 2, 1],
                    group: 0,
                },
                SurfaceTriangle {
                    nodes: [0, 1, 3],
                    group: 0,
                },
                SurfaceTriangle {
                    nodes: [1, 2, 3],
                    group: 0,
                },
                SurfaceTriangle {
                    nodes: [0, 3, 2],
                    group: 0,
                },
            ],
            region_count: 1,
            boundary_group_count: 1,
            cells: None,
        }
    }

    #[test]
    fn generated_volume_records_source_step_and_survives_reopen()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let step = fixture.root.join("Part.step");
        std::fs::write(&step, b"ISO-10303-21;\nEND-ISO-10303-21;\n")?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Generated")?;
        let record = service.generate_volume_record(&step, 2.5, unit_tet())?;
        let generation = record.generation.clone().ok_or("generation missing")?;
        assert_eq!(generation.generator, NETGEN_GENERATOR);
        assert_eq!(generation.source_step_name, "Part.step");
        assert!(generation.source_step_digest.starts_with("sha256:"));
        assert_eq!(generation.max_length_mm, 2.5);
        assert_eq!(record.display_name, "Part");
        assert_eq!(record.triangle_count, 4);
        assert!(service.volume_for_import(&record.id).is_some());

        // 来源 STEP 删除后工程仍可重开：资产自包含。
        std::fs::remove_file(&step)?;
        let mut reopened = ProjectService::new();
        reopened.open(&project.path)?;
        let reopened_record = reopened
            .imports()?
            .into_iter()
            .find(|item| item.id == record.id)
            .ok_or("record missing")?;
        assert_eq!(reopened_record.generation, record.generation);
        Ok(())
    }

    #[test]
    fn generated_volume_rejects_invalid_parameters_and_meshes()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let step = fixture.root.join("Part.step");
        std::fs::write(&step, b"ISO-10303-21;\n")?;
        let mut service = ProjectService::new();
        service.create(&fixture.root, "Generated")?;
        let error = service
            .begin_generated_volume(&step, -1.0, unit_tet())
            .err()
            .ok_or("negative length accepted")?;
        assert_eq!(error.code(), "project.volume_generation_failed");
        let mut empty = unit_tet();
        empty.tets.clear();
        let error = service
            .begin_generated_volume(&step, 0.0, empty)
            .err()
            .ok_or("empty mesh accepted")?;
        assert_eq!(error.code(), "project.volume_generation_failed");
        let missing = service
            .generate_volume_record(&fixture.root.join("missing.step"), 0.0, unit_tet())
            .err()
            .ok_or("missing STEP accepted")?;
        assert_eq!(missing.code(), "project.volume_generation_failed");
        assert!(service.imports()?.is_empty());
        Ok(())
    }

    #[test]
    fn generated_volume_commit_can_be_cancelled_before_the_manifest_gate()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let step = fixture.root.join("Part.step");
        std::fs::write(&step, vec![b'x'; 4 << 20])?;
        let mut service = ProjectService::new();
        service.create(&fixture.root, "Generated")?;
        let request = service.begin_generated_volume(&step, 0.0, unit_tet())?;
        if service.cancel_stl_import(request) {
            assert!(service.imports()?.is_empty());
        }
        Ok(())
    }
}
