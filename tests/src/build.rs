//! 常规构建与 native 测试编排；CMake 仍拥有 native 构建图。

use std::error::Error;
use std::process::Command;

use crate::command::{cargo, run_scanner};
use crate::paths::{native_build_dir, target_root};

pub(super) fn build_launcher() -> Result<(), Box<dyn Error>> {
    cargo("build", ["--locked", "-p", "panta-launcher"])
}

pub(super) fn cmake_build_command() -> Result<Command, Box<dyn Error>> {
    let native_dir = native_build_dir();
    let mut command = Command::new(panta_build::resolve_cmake(target_root())?);
    command
        .envs(panta_build::native_test_env(
            target_root(),
            env!("PANTA_TEST_HOST"),
        )?)
        .current_dir(native_dir)
        .args(["--build"])
        .arg(native_dir)
        .args(["--config", env!("PANTA_TEST_BUILD_TYPE")]);
    Ok(command)
}

pub(super) fn run_qmllint() -> Result<(), Box<dyn Error>> {
    let mut command = cmake_build_command()?;
    command.args(["--target", "all_qmllint"]);
    // 通过时静默：Qt 生成的 no-op qmllint 目标会回显 "Nothing to do"，
    // 失败时原样转发全部输出（含 qmllint 诊断）。
    run_scanner("qmllint", command)
}
