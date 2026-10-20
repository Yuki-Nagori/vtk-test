//! 工具安装布局、可执行文件查找及可执行权限。

use std::fs;
use std::path::{Path, PathBuf};

/// 平台可执行名；测试运行器与本 crate 的工具定位共用同一后缀规则。
pub fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

pub(super) fn tools_root(target_root: &Path) -> PathBuf {
    target_root.join("panta-tools")
}

/// 深度受限地查找 `bin/cmake`；返回第一个命中的路径。
pub(super) fn find_cmake_binary(dir: &Path, exe: &str, depth: usize) -> Option<PathBuf> {
    if depth > 4 {
        return None;
    }
    let bin_dir = dir.join("bin");
    let candidate = bin_dir.join(exe);
    if candidate.is_file() {
        return Some(candidate);
    }
    let entries = fs::read_dir(dir).ok()?;
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            paths.push(path);
        }
    }
    paths.sort();
    paths
        .iter()
        .find_map(|path| find_cmake_binary(path, exe, depth + 1))
}

/// 在 LLVM 解包目录中查找工具；官方压缩包在顶层带有版本目录，Windows
/// 归档和 Unix 归档的布局因此统一按相对路径处理。
pub(super) fn find_binary(dir: &Path, name: &str) -> Option<PathBuf> {
    let exe = exe_name(name);
    let mut directories = vec![(dir.to_owned(), 0usize)];
    while let Some((directory, depth)) = directories.pop() {
        if depth > 6 {
            continue;
        }
        for candidate in [directory.join(&exe), directory.join("bin").join(&exe)] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut children = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        children.sort();
        for path in children {
            directories.push((path, depth + 1));
        }
    }
    None
}

pub(super) fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exe = exe_name(name);
    std::env::split_paths(&path)
        .map(|directory| directory.join(&exe))
        .find(|candidate| candidate.is_file())
}

pub(super) fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .map_err(|error| format!("设置 {} 可执行位失败：{error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        // Windows zip 内的 .exe 自带可执行语义，无需额外处理。
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_cmake_binary_in_nested_layout() {
        let dir = std::env::temp_dir().join(format!("panta-provision-tree-{}", std::process::id()));
        let nested = dir
            .join("cmake-4.4.3-macos-universal")
            .join("CMake.app")
            .join("Contents")
            .join("bin");
        fs::create_dir_all(&nested).unwrap_or_else(|error| panic!("mkdir failed: {error}"));
        let binary = nested.join(exe_name("cmake"));
        fs::write(&binary, b"MZ").unwrap_or_else(|error| panic!("write failed: {error}"));

        let found = find_cmake_binary(&dir, &exe_name("cmake"), 0);
        assert_eq!(found, Some(binary));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_binary_returns_none() {
        let dir =
            std::env::temp_dir().join(format!("panta-provision-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("mkdir failed: {error}"));
        assert_eq!(find_cmake_binary(&dir, exe_name("cmake").as_str(), 0), None);
        assert_eq!(
            find_cmake_binary(
                &PathBuf::from("/nonexistent-panta"),
                exe_name("cmake").as_str(),
                0
            ),
            None
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
