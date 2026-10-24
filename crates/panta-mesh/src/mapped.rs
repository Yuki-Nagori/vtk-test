//! 只读文件映射（task 028）：`panta-mesh` 中唯一放开 `unsafe` 的模块，只封装 `memmap2::Mmap`。
//!
//! 映射期间文件被他人改写或截断是映射 IO 的固有风险，这里按平台尽量排除：
//!
//! - Windows：打开时共享模式只给 `FILE_SHARE_READ`（不给写和删除共享），已有写句柄时打开失败，
//!   打开后他人也无法以写方式打开、删除或改名；另加整文件共享锁（`LockFileEx`，强制锁）。
//! - Unix：加整文件共享 `flock`（建议锁）。本项目写入方都先写临时文件再 rename，不就地改写已发布文件；
//!   不遵守建议锁的外部进程截断文件时，访问越界页会触发 SIGBUS，这一残余风险无法在用户态消除，
//!   记录在 task 028。
//!
//! 内容一致性由上层保证：打开时校验文件长度与索引，每块解压时校验 zstd 校验和与计数。

use std::fs::File;
use std::path::Path;

/// 只读映射的整个文件；持有文件句柄与共享锁直到释放。
pub struct MappedFile {
    map: memmap2::Mmap,
    /// 保持句柄（及其共享锁）与映射同生命周期。
    _file: File,
}

impl MappedFile {
    /// 只读打开并整文件映射。已有他人持有排他锁、或（Windows）已有写句柄时返回错误，调用方应回退到定位读。
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = open_read_only(path)?;
        file.try_lock_shared().map_err(|error| match error {
            std::fs::TryLockError::Error(error) => error,
            std::fs::TryLockError::WouldBlock => {
                std::io::Error::new(std::io::ErrorKind::WouldBlock, "file is locked for writing")
            }
        })?;
        let len = usize::try_from(file.metadata()?.len())
            .map_err(|_| std::io::Error::other("file too large to map"))?;
        if len == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "cannot map an empty file",
            ));
        }
        // SAFETY: `Mmap::map` 要求映射期间底层文件不被修改或截断，否则读取映射内存是未定义行为。
        // 文件只读打开，并在映射存活期间一直持有整文件共享锁；Windows 上共享模式拒绝写 / 删除，
        // 且共享锁是强制锁，其他进程无法写入。Unix 上锁是建议锁，残余风险见模块文档。
        // 映射长度取自刚读到的文件长度，返回的 `Mmap` 只提供 `&[u8]` 只读视图。
        let map = unsafe { memmap2::MmapOptions::new().len(len).map(&file)? };
        Ok(Self { map, _file: file })
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.map
    }

    /// 提示内核预读 `[offset, offset + len)`（Unix `madvise(WILLNEED)`）；Windows 无对应安全接口，不做任何事。
    pub fn will_need(&self, offset: usize, len: usize) {
        #[cfg(unix)]
        {
            if offset
                .checked_add(len)
                .is_some_and(|end| end <= self.map.len())
            {
                let _ = self
                    .map
                    .advise_range(memmap2::Advice::WillNeed, offset, len);
            }
        }
        #[cfg(not(unix))]
        let _ = (offset, len);
    }
}

#[cfg(windows)]
fn open_read_only(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ`：不给 `FILE_SHARE_WRITE` / `FILE_SHARE_DELETE`。
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
}

#[cfg(not(windows))]
fn open_read_only(path: &Path) -> std::io::Result<File> {
    File::open(path)
}
