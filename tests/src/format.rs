//! Rust / C++ / QML 格式入口及源码清单。

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::command::{cargo, run};
use crate::paths::{repository_root, target_root};

pub(super) fn format_all(check: bool, rest: &[String]) -> Result<(), Box<dyn Error>> {
    if let Some(unexpected) = rest.first() {
        return Err(format!(
            "format 不接受位置参数 '{unexpected}'；修复为缺省行为，\
                            --check 只验证不改动"
        )
        .into());
    }
    if check {
        cargo("fmt", ["--all", "--", "--check"])?;
    } else {
        cargo("fmt", ["--all"])?;
    }
    check_cpp_format(check)?;
    run_qml_format(check)?;
    if check {
        println!("format 检查通过");
    } else {
        println!("format 完成");
    }
    Ok(())
}

fn format_qml_files(check: bool, files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let target_root = target_root();
    let qmlformat = qmlformat_path(target_root);
    if !qmlformat.is_file() {
        provision_qml_format(root, target_root)?;
    }
    for file in files {
        let mut command = Command::new(&qmlformat);
        if check {
            let output = command
                .arg(file)
                .output()
                .map_err(|error| format!("qmlformat 启动失败（{}）：{error}", file.display()))?;
            if !output.status.success() {
                return Err(format!(
                    "qmlformat 失败：{}\n{}",
                    file.display(),
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }
            let formatted = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
            let current = fs::read_to_string(file)?.replace("\r\n", "\n");
            if formatted != current {
                return Err(format!("QML 文件需要格式化：{}", file.display()).into());
            }
        } else {
            command.arg("-i").arg(file);
            run(&format!("qmlformat {}", file.display()), command)?;
        }
    }
    Ok(())
}

fn run_qml_format(check: bool) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let mut files = Vec::new();
    for directory in ["qml", "tests/qml"] {
        files.extend(qml_sources(&root.join(directory))?);
    }
    files.sort();
    if files.is_empty() {
        return Err("QML 源文件清单为空".into());
    }
    format_qml_files(check, &files)
}

fn qml_sources(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            files.extend(qml_sources(&path)?);
        } else if path.extension().is_some_and(|extension| extension == "qml") {
            files.push(path);
        }
    }
    Ok(files)
}

fn qmlformat_path(target_root: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "qmlformat.exe"
    } else {
        "qmlformat"
    };
    target_root.join("panta-deps/qt/staging/bin").join(name)
}

fn provision_qml_format(root: &Path, target_root: &Path) -> Result<(), Box<dyn Error>> {
    let cmake = panta_build::resolve_cmake(target_root)?;
    let mut command = Command::new(cmake);
    command
        .arg(format!(
            "-DQT_PROVISION_DIR={}",
            target_root.join("panta-deps/qt").display()
        ))
        .arg("-P")
        .arg(root.join("native/cmake/qt-provision.cmake"));
    run("准备 qmlformat", command)
}

fn check_cpp_format(check: bool) -> Result<(), Box<dyn Error>> {
    let llvm = panta_build::resolve_llvm_compilers(target_root())?;
    let clang_format = &llvm.clang_format;
    if !clang_format.is_file() {
        return Err(format!("clang-format 不存在：{}", clang_format.display()).into());
    }
    let root = repository_root()?;
    let mut files = cpp_sources(&root.join("native"))?;
    files.extend(cpp_sources(&root.join("tests/cpp"))?);
    files.extend(cpp_sources(&root.join("tests/qml"))?);
    files.extend(cpp_sources(&root.join("tests/support"))?);
    if files.is_empty() {
        return Err("C++ 源文件清单为空".into());
    }
    files.sort();
    let mut command = Command::new(clang_format);
    if check {
        command.args(["--dry-run", "-Werror"]);
    } else {
        command.arg("-i");
    }
    command.args(files.iter());
    run(&format!("clang-format（{} 个文件）", files.len()), command)
}

fn cpp_sources(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            files.extend(cpp_sources(&path)?);
        } else if is_cpp_file(&path) {
            files.push(path);
        }
    }
    Ok(files)
}

fn is_cpp_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension,
                "cc" | "cpp" | "cxx" | "mm" | "h" | "hh" | "hpp" | "hxx"
            )
        })
}
