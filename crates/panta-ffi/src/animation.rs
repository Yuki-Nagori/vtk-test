//! 时序动画序列的 FFI 适配（task 020）：opaque 句柄，GUI 线程持有并轮询帧状态。
use crate::bridge;
use crate::project_response::diagnostic;
use panta_core::animation::{AnimationSequence as CoreSequence, FrameState};

pub struct AnimationSequence(Option<CoreSequence>);

/// 打开失败时返回空句柄并写入诊断；空句柄的帧数为 0。
pub(super) fn animation_open(
    path: &str,
    error: &mut bridge::ProjectDiagnostic,
) -> Box<AnimationSequence> {
    match CoreSequence::open(std::path::Path::new(path)) {
        Ok(sequence) => Box::new(AnimationSequence(Some(sequence))),
        Err(failure) => {
            *error = diagnostic(failure.code(), "input", failure.detail());
            Box::new(AnimationSequence(None))
        }
    }
}

pub(super) fn animation_frame_count(sequence: &AnimationSequence) -> u64 {
    sequence.0.as_ref().map_or(0, |s| s.frame_count() as u64)
}

pub(super) fn animation_cached_frames(sequence: &AnimationSequence) -> u64 {
    sequence.0.as_ref().map_or(0, |s| s.cached_frames() as u64)
}

pub(super) fn animation_frame_name(sequence: &AnimationSequence, index: u64) -> String {
    sequence
        .0
        .as_ref()
        .and_then(|s| s.frame_path(usize::try_from(index).ok()?))
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(super) fn animation_request(sequence: &mut AnimationSequence, index: u64) {
    if let (Some(s), Ok(index)) = (sequence.0.as_mut(), usize::try_from(index)) {
        s.request(index);
    }
}

/// 帧就绪时 `ready` 为真并带坐标；未就绪两者皆空；失败写入 `animation.frame_failed`。
pub(super) fn animation_frame(
    sequence: &mut AnimationSequence,
    index: u64,
) -> bridge::AnimationFrameResult {
    let mut result = bridge::AnimationFrameResult::default();
    let (Some(s), Ok(index)) = (sequence.0.as_mut(), usize::try_from(index)) else {
        result.error = diagnostic(
            "animation.not_a_sequence",
            "input",
            "no sequence".to_owned(),
        );
        return result;
    };
    match s.frame(index) {
        FrameState::Pending => {}
        FrameState::Failed(detail) => {
            result.error = diagnostic("animation.frame_failed", "input", detail);
        }
        FrameState::Ready(mesh) => match crate::mesh_coordinates(&mesh) {
            Ok(coordinates) => {
                result.ready = true;
                result.coordinates = coordinates;
            }
            Err(detail) => {
                result.error = diagnostic("animation.frame_failed", "resource", detail);
            }
        },
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn sequence_handle_reports_open_errors_and_ready_frames() -> Result<(), Box<dyn Error>> {
        let root = std::env::temp_dir().join(format!("panta-ffi-anim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        for index in 0..2 {
            std::fs::write(
                root.join(format!("f_{index}.stl")),
                "solid t\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid\n",
            )?;
        }
        let mut error = bridge::ProjectDiagnostic::default();
        let missing = animation_open(&root.join("x.stl").display().to_string(), &mut error);
        assert_eq!(error.code, "animation.not_a_sequence");
        assert_eq!(animation_frame_count(&missing), 0);

        let mut error = bridge::ProjectDiagnostic::default();
        let mut sequence = animation_open(&root.join("f_1.stl").display().to_string(), &mut error);
        assert!(error.code.is_empty());
        assert_eq!(animation_frame_count(&sequence), 2);
        assert_eq!(animation_frame_name(&sequence, 1), "f_1.stl");
        animation_request(&mut sequence, 1);
        let mut frame = animation_frame(&mut sequence, 1);
        for _ in 0..500 {
            if frame.ready || !frame.error.code.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            frame = animation_frame(&mut sequence, 1);
        }
        assert!(frame.ready);
        assert_eq!(frame.coordinates.len(), 9);
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
