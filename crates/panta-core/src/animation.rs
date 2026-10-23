//! 时序动画序列（task 020）：发现同名编号文件、后台解析、预取与按字节预算的 LRU 帧缓存。
//! 序列只读、不入工程；当前帧在缓存中固定，迟到或已淘汰帧的结果直接丢弃。
use panta_mesh::SurfaceMesh;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

/// 帧缓存字节预算（按每三角面 72 B 估算）。
pub const ANIMATION_CACHE_BUDGET_BYTES: usize = 512 * 1024 * 1024;
/// 当前帧之后预取的帧数。
pub const ANIMATION_PREFETCH_FRAMES: usize = 4;
/// 单个序列的帧数上限。
pub const ANIMATION_MAX_FRAMES: usize = 100_000;
const TRIANGLE_BYTES: usize = 72;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimationError {
    /// 所选文件不构成至少 2 帧的编号序列。
    NotSequence(String),
    Io(String),
}

impl AnimationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotSequence(_) => "animation.not_a_sequence",
            Self::Io(_) => "animation.io_failed",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::NotSequence(detail) | Self::Io(detail) => detail.clone(),
        }
    }
}

impl std::fmt::Display for AnimationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.detail())
    }
}

impl std::error::Error for AnimationError {}

/// 拆分 `prefix<digits>.ext` 的文件名；无数字结尾返回 None。
fn split_numbered(path: &Path) -> Option<(String, u64, String)> {
    let stem = path.file_stem()?.to_str()?;
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let digits = stem.len() - stem.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 18 {
        return None;
    }
    let (prefix, number) = stem.split_at(stem.len() - digits);
    Some((prefix.to_owned(), number.parse().ok()?, extension))
}

/// 由任一帧文件发现同目录、同前缀、同扩展名（不区分大小写）的编号序列，按编号升序。
pub fn discover_sequence(path: &Path) -> Result<Vec<PathBuf>, AnimationError> {
    let not_sequence = || AnimationError::NotSequence(path.display().to_string());
    let (prefix, _, extension) = split_numbered(path).ok_or_else(not_sequence)?;
    let directory = path.parent().ok_or_else(not_sequence)?;
    let entries = std::fs::read_dir(directory)
        .map_err(|error| AnimationError::Io(format!("{}: {error}", directory.display())))?;
    let mut frames = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| AnimationError::Io(format!("{}: {error}", directory.display())))?;
        let candidate = entry.path();
        if !candidate.is_file() {
            continue;
        }
        if let Some((other_prefix, number, other_extension)) = split_numbered(&candidate)
            && other_prefix == prefix
            && other_extension == extension
        {
            frames.push((number, candidate));
            if frames.len() > ANIMATION_MAX_FRAMES {
                return Err(AnimationError::NotSequence(format!(
                    "{}: more than {ANIMATION_MAX_FRAMES} frames",
                    path.display()
                )));
            }
        }
    }
    frames.sort();
    frames.dedup_by_key(|(number, _)| *number);
    if frames.len() < 2 {
        return Err(not_sequence());
    }
    Ok(frames.into_iter().map(|(_, path)| path).collect())
}

/// 单帧状态。
#[derive(Debug, Clone)]
pub enum FrameState {
    Pending,
    Ready(Arc<SurfaceMesh>),
    Failed(String),
}

enum Slot {
    Loading,
    Ready(Arc<SurfaceMesh>),
    Failed(String),
}

type FrameReply = (usize, Result<SurfaceMesh, String>);

/// 帧序列加载器；由单个 GUI 线程持有，解析在专用后台线程执行。
pub struct AnimationSequence {
    frames: Vec<PathBuf>,
    slots: HashMap<usize, Slot>,
    ready_order: VecDeque<usize>,
    cached_bytes: usize,
    budget_bytes: usize,
    current: usize,
    jobs: Option<Sender<(usize, PathBuf)>>,
    replies: Receiver<FrameReply>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for AnimationSequence {
    fn drop(&mut self) {
        // 断开任务通道；工作线程完成当前帧后退出，迟到结果随接收端丢弃。
        self.cancelled.store(true, Ordering::Release);
        self.jobs = None;
    }
}

fn parse_frame(path: &Path) -> Result<SurfaceMesh, String> {
    let snapshot = panta_import::read_source(path).map_err(|error| error.to_string())?;
    if !snapshot.format.is_surface_mesh() {
        return Err(format!("{}: not a surface mesh format", path.display()));
    }
    panta_import::parse_mesh_snapshot(&snapshot).map_err(|error| error.to_string())
}

impl AnimationSequence {
    pub fn open(path: &Path) -> Result<Self, AnimationError> {
        Self::with_frames(discover_sequence(path)?, ANIMATION_CACHE_BUDGET_BYTES)
    }

    pub(crate) fn with_frames(
        frames: Vec<PathBuf>,
        budget_bytes: usize,
    ) -> Result<Self, AnimationError> {
        let (jobs, job_receiver) = mpsc::channel::<(usize, PathBuf)>();
        let (reply_sender, replies) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        std::thread::Builder::new()
            .name("panta-animation".to_owned())
            .spawn(move || {
                for (index, path) in job_receiver {
                    if worker_cancelled.load(Ordering::Acquire) {
                        return;
                    }
                    if reply_sender.send((index, parse_frame(&path))).is_err() {
                        return;
                    }
                }
            })
            .map_err(|error| AnimationError::Io(format!("start animation worker: {error}")))?;
        Ok(Self {
            frames,
            slots: HashMap::new(),
            ready_order: VecDeque::new(),
            cached_bytes: 0,
            budget_bytes,
            current: 0,
            jobs: Some(jobs),
            replies,
            cancelled,
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn frame_path(&self, index: usize) -> Option<&Path> {
        self.frames.get(index).map(PathBuf::as_path)
    }

    pub fn cached_frames(&self) -> usize {
        self.ready_order.len()
    }

    pub fn cached_bytes(&self) -> usize {
        self.cached_bytes
    }

    /// 设为当前帧并排队当前帧与后续预取帧（循环回绕）；进行中的任务数受预取窗口约束。
    pub fn request(&mut self, index: usize) {
        if self.frames.is_empty() {
            return;
        }
        self.current = index % self.frames.len();
        let window = (ANIMATION_PREFETCH_FRAMES + 1).min(self.frames.len());
        for offset in 0..window {
            let frame = (self.current + offset) % self.frames.len();
            if self.slots.contains_key(&frame) {
                continue;
            }
            let loading = self
                .slots
                .values()
                .filter(|slot| matches!(slot, Slot::Loading))
                .count();
            if loading >= window {
                break;
            }
            let Some(jobs) = &self.jobs else { return };
            if jobs.send((frame, self.frames[frame].clone())).is_err() {
                return;
            }
            self.slots.insert(frame, Slot::Loading);
        }
    }

    /// 收取后台结果并返回指定帧状态。
    pub fn frame(&mut self, index: usize) -> FrameState {
        self.drain();
        match self.slots.get(&index) {
            Some(Slot::Ready(mesh)) => FrameState::Ready(Arc::clone(mesh)),
            Some(Slot::Failed(message)) => FrameState::Failed(message.clone()),
            Some(Slot::Loading) | None => FrameState::Pending,
        }
    }

    fn drain(&mut self) {
        while let Ok((index, result)) = self.replies.try_recv() {
            if !matches!(self.slots.get(&index), Some(Slot::Loading)) {
                continue;
            }
            match result {
                Ok(mesh) => {
                    self.cached_bytes += mesh.triangles.len() * TRIANGLE_BYTES;
                    self.slots.insert(index, Slot::Ready(Arc::new(mesh)));
                    self.ready_order.push_back(index);
                    self.evict();
                }
                Err(message) => {
                    self.slots.insert(index, Slot::Failed(message));
                }
            }
        }
    }

    /// 超预算时按就绪先后淘汰；当前帧固定，因此单帧超预算时仍保留当前帧。
    fn evict(&mut self) {
        let mut kept = VecDeque::new();
        while self.cached_bytes > self.budget_bytes {
            let Some(index) = self.ready_order.pop_front() else {
                break;
            };
            if index == self.current {
                kept.push_back(index);
                continue;
            }
            if let Some(Slot::Ready(mesh)) = self.slots.remove(&index) {
                self.cached_bytes -= mesh.triangles.len() * TRIANGLE_BYTES;
            }
        }
        while let Some(index) = kept.pop_back() {
            self.ready_order.push_front(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn triangle_stl(scale: f64, triangles: usize) -> String {
        let mut text = String::from("solid t\n");
        for index in 0..triangles {
            let z = index as f64;
            text.push_str(&format!(
                "vertex 0 0 {z}\nvertex {scale} 0 {z}\nvertex 0 {scale} {z}\n"
            ));
        }
        text.push_str("endsolid\n");
        text
    }

    fn wait_ready(sequence: &mut AnimationSequence, index: usize) -> FrameState {
        for _ in 0..500 {
            let state = sequence.frame(index);
            if !matches!(state, FrameState::Pending) {
                return state;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        FrameState::Pending
    }

    #[test]
    fn discovery_sorts_numerically_and_filters_prefix_and_extension() -> Result<(), Box<dyn Error>>
    {
        let fixture = crate::project::temp_directory::Fixture::new()?;
        for name in [
            "flow_10.stl",
            "flow_2.STL",
            "flow_1.stl",
            "flow_3.obj",
            "other_4.stl",
        ] {
            std::fs::write(fixture.root.join(name), triangle_stl(1.0, 1))?;
        }
        let frames = discover_sequence(&fixture.root.join("flow_2.STL"))?;
        let names: Vec<_> = frames
            .iter()
            .filter_map(|path| path.file_name()?.to_str())
            .collect();
        assert_eq!(names, ["flow_1.stl", "flow_2.STL", "flow_10.stl"]);
        assert_eq!(
            discover_sequence(&fixture.root.join("other_4.stl")).map_err(|e| e.code()),
            Err("animation.not_a_sequence")
        );
        assert!(discover_sequence(&fixture.root.join("plain.stl")).is_err());
        Ok(())
    }

    #[test]
    fn loader_prefetches_evicts_within_budget_and_isolates_bad_frames() -> Result<(), Box<dyn Error>>
    {
        let fixture = crate::project::temp_directory::Fixture::new()?;
        let mut frames = Vec::new();
        for index in 0..8 {
            let path = fixture.root.join(format!("step_{index:03}.stl"));
            let body = if index == 5 {
                "not a mesh".to_owned()
            } else {
                triangle_stl(index as f64 + 1.0, 10)
            };
            std::fs::write(&path, body)?;
            frames.push(path);
        }
        // 每帧 10 面 = 720 B；预算容纳 3 帧。
        let mut sequence = AnimationSequence::with_frames(frames, 3 * 720)?;
        assert_eq!(sequence.frame_count(), 8);
        sequence.request(0);
        let FrameState::Ready(first) = wait_ready(&mut sequence, 0) else {
            return Err("frame 0 not ready".into());
        };
        assert_eq!(first.triangles.len(), 10);
        // 预取的 1..=4 帧随后就绪；缓存不超过预算且保留当前帧。
        assert!(matches!(wait_ready(&mut sequence, 4), FrameState::Ready(_)));
        assert!(sequence.cached_bytes() <= 3 * 720);
        assert!(matches!(sequence.frame(0), FrameState::Ready(_)));
        sequence.request(5);
        assert!(matches!(
            wait_ready(&mut sequence, 5),
            FrameState::Failed(_)
        ));
        assert!(matches!(wait_ready(&mut sequence, 6), FrameState::Ready(_)));
        // 回绕预取到第 0 帧之后的帧。
        sequence.request(7);
        assert!(matches!(wait_ready(&mut sequence, 7), FrameState::Ready(_)));
        assert!(sequence.cached_frames() <= 3);
        Ok(())
    }
}
