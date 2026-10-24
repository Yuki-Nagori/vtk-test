//! 只读文件映射（task 028）：`panta-mesh` 中唯一放开 `unsafe` 的模块，只封装 `memmap2::Mmap`。
//!
//! 使用范围：只用于派生分块文件的**按需分块读取**。“禁用分块、完整驻留”模式不经过本模块，
//! 而是用 MiB 级缓冲顺序读入进程自有内存，读完立即关闭文件、释放锁，不保留任何映射。
//!
//! # 风险
//!
//! 映射把文件内容直接暴露成 `&[u8]`。映射存活期间，只要底层文件被任何进程**修改或截断**，
//! 就违反了 Rust 对共享引用“内容不变”的假设，属于**未定义行为**（不只是读到脏数据）：
//! 修改可能让编译器基于“内容不变”所做的推断失效；截断后访问超出新文件长度的页，
//! Unix 上触发 SIGBUS、Windows 上触发访问冲突（`EXCEPTION_IN_PAGE_ERROR`），进程直接崩溃。
//!
//! # 缓解措施
//!
//! - **锁**：映射存活期间一直持有整文件共享锁（`File::try_lock_shared`）；已有排他锁时打开失败，
//!   调用方回退到定位读。
//! - **Windows 共享模式**：只给 `FILE_SHARE_READ`，不给 `FILE_SHARE_WRITE` / `FILE_SHARE_DELETE`。
//!   已有写句柄时打开失败；打开后其他进程无法以写方式打开、删除或改名。加上 `LockFileEx` 共享锁
//!   是强制锁，Windows 上这一风险在操作系统层面被排除。
//! - **长度与索引校验**：打开时校验文件长度、头部与块索引；每次读取前校验 `[offset, offset + len)` 不越过文件长度，
//!   所有切片都经过边界检查，不按未经校验的偏移访问映射。
//! - **按块校验**：每块解压时校验 zstd 帧校验和与解出的节点 / 单元计数，内容不一致返回错误而不是继续使用。
//! - **写入方约定**：本项目所有写入方都先写临时文件再 rename 覆盖，从不就地改写已发布的文件，
//!   rename 不影响已打开的映射（映射仍指向旧 inode / 旧文件对象）。
//!
//! # 残余风险（Unix）
//!
//! Unix 上 `flock` 是**建议锁**，拦不住不合作的进程：外部程序（或用户手工）就地改写或截断
//! 正在映射的派生文件时，上面的校验只能发现已读入的不一致，无法阻止并发修改本身，
//! 截断后访问相应页仍会 SIGBUS。这一风险无法在用户态消除；派生文件放在应用缓存目录、
//! 不面向用户编辑，以此把触发概率降到最低。记录在 task 028。

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
        // SAFETY: `Mmap::map` 要求映射期间底层文件不被修改或截断：只读映射在文件被修改或截断时
        // 读取映射内存是未定义行为（截断后访问越界页还会 SIGBUS / 访问冲突）。缓解：
        // 1. 文件只读打开，映射存活期间一直持有整文件共享锁（`_file` 与 `map` 同生命周期）；
        // 2. Windows 上共享模式不给 FILE_SHARE_WRITE / FILE_SHARE_DELETE，且 LockFileEx 共享锁是强制锁，
        //    其他进程无法写入、截断或删除；
        // 3. 映射长度取自刚读到的文件长度；上层打开时校验长度与索引，每块解压时校验校验和与计数，
        //    只按校验过的偏移切片；
        // 4. 返回的 `Mmap` 只提供 `&[u8]` 只读视图，本进程从不写该文件。
        // 残余风险：Unix 上 flock 是建议锁，拦不住不合作进程就地改写或截断，见模块文档。
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
