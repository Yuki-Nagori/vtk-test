//! panta 统一运行入口。
//!
//! 启动 CMake 构建的 native 产物并转发参数与退出码（任务 004）；桌面能力
//! 由任务 005 在 native/app 接入。产物路径由 build.rs 在编译期注入
//! （PANTA_NATIVE_BIN），定位不依赖启动时的当前目录。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// native 产物缺失或无法启动；取 BSD sysexits.h 的 EX_UNAVAILABLE。
const EXIT_PRODUCT_UNAVAILABLE: u8 = 69;
/// 子进程被信号终止且平台无法给出信号号时的回退退出码（对应 SIGINT 约定）。
const EXIT_SIGNAL_FALLBACK: u8 = 130;

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|argument| argument == "package")
    {
        return match install_package(&arguments[1..]) {
            Ok(code) => code,
            Err(message) => {
                eprintln!("panta-launcher: {message}");
                ExitCode::from(EXIT_PRODUCT_UNAVAILABLE)
            }
        };
    }

    let product = native_product();
    match run(product.as_deref(), &arguments) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("panta-launcher: {message}");
            ExitCode::from(EXIT_PRODUCT_UNAVAILABLE)
        }
    }
}

/// 通过当前 Cargo 构建对应的 CMake install 规则生成可搬迁安装目录。
/// 默认写入 `target/package/<profile>`；可用第一个参数指定其他前缀。
fn install_package(arguments: &[OsString]) -> Result<ExitCode, String> {
    if arguments.len() > 1 {
        return Err("用法：cargo panta-package [输出目录]".to_owned());
    }
    let cmake = option_env!("PANTA_CMAKE_EXECUTABLE")
        .ok_or("构建配置异常：未注入 PANTA_CMAKE_EXECUTABLE")?;
    let build_dir = option_env!("PANTA_NATIVE_BUILD_DIR")
        .ok_or("构建配置异常：未注入 PANTA_NATIVE_BUILD_DIR")?;
    let build_type = option_env!("PANTA_NATIVE_BUILD_TYPE")
        .ok_or("构建配置异常：未注入 PANTA_NATIVE_BUILD_TYPE")?;
    let prefix = arguments
        .first()
        .map(|argument| PathBuf::from(argument.as_os_str()))
        .or_else(|| option_env!("PANTA_PACKAGE_PREFIX").map(PathBuf::from))
        .ok_or("构建配置异常：未注入 PANTA_PACKAGE_PREFIX")?;

    let status = std::process::Command::new(cmake)
        .arg("--install")
        .arg(build_dir)
        .arg("--config")
        .arg(build_type)
        .arg("--component")
        .arg("Runtime")
        .arg("--prefix")
        .arg(&prefix)
        .status()
        .map_err(|error| format!("无法启动 CMake install：{error}"))?;
    if status.success() {
        println!("panta 安装目录：{}", prefix.display());
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(exit_code_of(&status))
    }
}

/// 构建脚本注入的 native 产物绝对路径；含空格也安全（编译期字符串，运行期
/// 以参数数组传递，不经 shell）。
fn native_product() -> Option<PathBuf> {
    option_env!("PANTA_NATIVE_BIN").map(PathBuf::from)
}

/// 启动 native 产物：stdout/stderr 直接继承，退出码原样转发。
/// product 缺失或启动失败时返回诊断，由调用方以固定退出码报告。
fn run(product: Option<&Path>, arguments: &[OsString]) -> Result<ExitCode, String> {
    let Some(product) = product else {
        return Err("构建配置异常：未注入 PANTA_NATIVE_BIN（应总是由 build.rs 提供）".to_string());
    };
    if !product.exists() {
        return Err(format!(
            "native 产物不存在：{}；请先执行 cargo build（诊断见 ai-docs/architecture/build-and-development.md）",
            product.display()
        ));
    }

    let mut command = std::process::Command::new(product);
    command.args(arguments);
    match command.status() {
        Ok(status) => Ok(exit_code_of(&status)),
        Err(error) => Err(format!("无法启动 {}：{error}", product.display())),
    }
}

/// 退出码转发规则：平台退出码取低 8 位（POSIX 语义；Windows 上超出 255 的
/// 系统码会被截断，桌面化时由 005 重新审视）；信号终止映射为 128+信号号，
/// 无信号号信息时回退到 SIGINT 约定的 130。
fn exit_code_of(status: &std::process::ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from((code & 0xFF) as u8);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ExitCode::from((128 + signal).min(u8::MAX.into()) as u8);
        }
    }
    ExitCode::from(EXIT_SIGNAL_FALLBACK)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn missing_product_reports_unavailable() {
        let missing = Path::new("/panta/definitely/missing/panta");
        let error = run(Some(missing), &[]).unwrap_err();
        assert!(error.contains("native 产物不存在"));
    }

    #[test]
    fn absent_injection_reports_configuration_error() {
        let error = run(None, &[]).unwrap_err();
        assert!(error.contains("PANTA_NATIVE_BIN"));
    }

    #[cfg(unix)]
    #[test]
    fn forwards_child_exit_code() {
        let code = run(Some(Path::new("/usr/bin/false")), &[]).unwrap();
        assert_eq!(code, ExitCode::from(1));
    }

    #[cfg(unix)]
    #[test]
    fn forwards_child_success() {
        let code = run(Some(Path::new("/usr/bin/true")), &[]).unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
    }
}
