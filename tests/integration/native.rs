use std::error::Error;
use std::path::Path;
use std::process::Command;

#[test]
fn native_and_qml_suite_passes() -> Result<(), Box<dyn Error>> {
    let target_root = Path::new(env!("PANTA_TEST_TARGET_DIR"));
    let cmake = panta_build::resolve_cmake(target_root)?;
    let native_dir = Path::new(env!("PANTA_TEST_NATIVE_DIR"));
    let build_type = env!("PANTA_TEST_BUILD_TYPE");
    let ctest = cmake.with_file_name(panta_build::exe_name("ctest"));
    if !native_dir.is_dir() {
        return Err(format!(
            "native 构建树不存在：{}；请先 cargo build --locked",
            native_dir.display()
        )
        .into());
    }
    if !ctest.is_file() {
        return Err(format!("ctest 不存在：{}", ctest.display()).into());
    }

    // qmllint 可能重新配置 CMake 并清除 GTest discovery 输出；随后完整构建
    // 测试目标，确保 CTest 消费实际生成的用例清单。
    for target in ["all_qmllint", "all"] {
        let status = Command::new(&cmake)
            .envs(panta_build::native_test_env(
                target_root,
                env!("PANTA_TEST_HOST"),
            )?)
            .args(["--build"])
            .arg(native_dir)
            .args(["--config", build_type, "--target", target])
            .status()?;
        if !status.success() {
            return Err(format!("native {target} 失败（退出码 {:?}）", status.code()).into());
        }
    }

    let status = Command::new(ctest)
        .envs(panta_build::native_test_env(
            target_root,
            env!("PANTA_TEST_HOST"),
        )?)
        .args(["--output-on-failure", "--no-tests=error", "-C", build_type])
        .current_dir(native_dir)
        .status()?;
    if !status.success() {
        return Err(format!("CTest 失败（退出码 {:?}）", status.code()).into());
    }
    Ok(())
}
