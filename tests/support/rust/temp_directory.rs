//! 工程功能 / 性能测试共享的临时目录；仅清理本实例创建的目录，不删除已有路径。
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Fixture {
    pub root: PathBuf,
}
impl Fixture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "panta-test-{}-{timestamp}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        Ok(Self { root })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
