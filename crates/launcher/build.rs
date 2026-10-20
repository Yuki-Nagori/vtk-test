//! Cargo→CMake 调度（任务 004）：以与 native presets 相同的有效配置构建
//! native 树，并把可执行产物路径在编译期注入 launcher（PANTA_NATIVE_BIN）。
//!
//! 图约束：Cargo → 本脚本 → CMake/Ninja → native targets；CMake 侧不回调
//! Cargo，无递归。未来 C++ 消费 Rust 库时，接入点是 CMake 侧导入 Rust 产物
//! （任务 006 定），不得从本脚本再次触发 Cargo。
//!
//! 产物和依赖缓存位于 Cargo target；原生诊断继承到 Cargo 输出。

use panta_build as provision;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::SystemTime;

/// Cargo 递归追踪源码目录；第三方与生成文件不在监听范围内。
const RERUN_PATHS: &[&str] = &[
    "build.rs",
    "../../native",
    "../../tests/cpp",
    "../../tests/qml",
    "../panta-ffi",
    "../panta-core",
    "../panta-import",
    "../panta-mesh",
    "../panta-foundation",
    "../../qml",
];

/// 影响配置结果的环境变量，变更即重建。系统工具旁路必须显式开启。
const RERUN_ENVS: &[&str] = &[
    "CMAKE",
    "PANTA_USE_SYSTEM_TOOLS",
    "PANTA_QT_MIRROR",
    "CC",
    "CXX",
    "CLANG_FORMAT",
    "CMAKE_GENERATOR",
    "CMAKE_GENERATOR_PLATFORM",
    "CMAKE_PREFIX_PATH",
    "DEVELOPER_DIR",
    "MACOSX_DEPLOYMENT_TARGET",
];

fn main() -> ExitCode {
    for path in RERUN_PATHS {
        println!("cargo:rerun-if-changed={path}");
    }
    for key in RERUN_ENVS {
        println!("cargo:rerun-if-env-changed={key}");
    }

    match orchestrate() {
        Ok(product) => {
            println!("cargo:rustc-env=PANTA_NATIVE_BIN={}", product.display());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("panta-launcher 构建脚本失败：{message}");
            ExitCode::FAILURE
        }
    }
}

fn orchestrate() -> Result<PathBuf, String> {
    let manifest_dir = required_var("CARGO_MANIFEST_DIR")?;
    let out_dir = required_var("OUT_DIR")?;
    let profile = required_var("PROFILE")?;

    let build_type = match profile.as_str() {
        "debug" => "Debug",
        "release" => "Release",
        other => return Err(format!("未知 PROFILE '{other}'，无法映射 CMAKE_BUILD_TYPE")),
    };

    // 路径可能含空格，全程走参数数组；CMake 在读取 -S 时消解 `..`。
    let native_dir = PathBuf::from(&manifest_dir).join("../../native");
    if !native_dir.is_dir() {
        return Err(format!("native 目录不可达：{}", native_dir.display()));
    }
    let target_root = provision::target_root(Path::new(&out_dir))?;
    let binary_dir = target_root.join("native").join(&profile);
    let deps_root = target_root.join("panta-deps");

    // 托管引导（任务 020）：定位 → 缺失时按固定资产下载并校验。
    let cmake = provision::resolve_cmake(&target_root)?;
    let llvm = provision::resolve_llvm_compilers(&target_root)?;
    if std::env::var("CMAKE_GENERATOR").is_ok_and(|value| value != "Ninja") {
        return Err("统一构建仅支持 Ninja，以保持三平台构建入口一致".into());
    }
    let ninja = provision::resolve_ninja(&target_root, &cmake)?;
    let target_triple = required_var("TARGET")?;
    let sdk_env = provision::windows_sdk_env(&target_triple)?;

    // Cargo 的默认 feature 是唯一用户入口；把 feature 状态转换成 CMake
    // 选项，避免开发者在日常命令中重复维护两套开关。
    let bridge_module = if std::env::var_os("CARGO_FEATURE_BRIDGE_MODULE").is_some() {
        "ON"
    } else {
        "OFF"
    };
    let (ffi_include, ffi_staticlib) = ffi_artifacts(&PathBuf::from(&out_dir))?;

    // 有效配置与 native/CMakePresets.json 一致：差异项只有构建类型，
    // 其余默认值由 native/CMakeLists.txt 统一。
    let mut configure = Command::new(&cmake);
    configure
        .arg("-S")
        .arg(&native_dir)
        .arg("-B")
        .arg(&binary_dir)
        .arg("-G")
        .arg("Ninja")
        .arg(format!("-DCMAKE_BUILD_TYPE={build_type}"))
        .arg(format!("-DPANTA_ENABLE_BRIDGE_MODULE={bridge_module}"))
        .arg("-DPANTA_ENABLE_FFI_TEST=ON")
        .arg(format!("-DPANTA_FFI_INCLUDE_DIR={}", ffi_include.display()))
        .arg(format!(
            "-DPANTA_FFI_STATIC_LIB={}",
            ffi_staticlib.display()
        ))
        .arg(format!(
            "-DQT_PROVISION_DIR={}",
            deps_root.join("qt").display()
        ))
        // 预编译 SDK 缓存根（任务 031）：与 Qt 一样跨 profile 共享；presets
        // 直接 configure 时默认构建树内。
        .arg(format!(
            "-DPANTA_SDK_PROVISION_DIR={}",
            deps_root.join("sdk").display()
        ));
    let cxx_compiler = if cfg!(windows) {
        llvm.clang_cl.as_ref().ok_or("LLVM 缺少 clang-cl")?
    } else {
        &llvm.clangxx
    };
    let c_compiler = if cfg!(windows) {
        cxx_compiler
    } else {
        &llvm.clang
    };
    configure
        .envs(sdk_env.clone())
        .arg(format!("-DCMAKE_C_COMPILER={}", c_compiler.display()))
        .arg(format!("-DCMAKE_CXX_COMPILER={}", cxx_compiler.display()))
        .arg(format!("-DCMAKE_MAKE_PROGRAM={}", ninja.display()))
        .arg(format!("-DPANTA_LLVM_VERSION={}", provision::LLVM_VERSION))
        // 每次显式传 ON/OFF，避免上次系统旁路设置污染 CMakeCache。
        .arg(format!(
            "-DPANTA_USE_SYSTEM_TOOLS={}",
            if provision::use_system_tools() {
                "ON"
            } else {
                "OFF"
            }
        ));
    if let Some(sdk) = provision::macos_sdk()? {
        configure.arg(format!("-DCMAKE_OSX_SYSROOT={}", sdk.display()));
        let deployment = match std::env::var("MACOSX_DEPLOYMENT_TARGET") {
            Ok(version) => version,
            Err(_) => provision::MACOS_DEPLOYMENT_TARGET.to_owned(),
        };
        configure.arg(format!("-DCMAKE_OSX_DEPLOYMENT_TARGET={deployment}"));
    }
    run_step("configure", &mut configure)?;

    let mut build = Command::new(&cmake);
    let test_env = provision::native_test_env(&target_root, &target_triple)?;
    build
        // 构建期 gtest discovery（POST_BUILD）会在链接后立即启动测试可执行
        // 文件；Windows 子进程必须能解析托管 Qt runtime（native_test_env 注
        // 入 PATH，非 Windows 仅透传 SDK 环境）。
        .envs(test_env)
        .arg("--build")
        .arg(&binary_dir)
        .arg("--config")
        .arg(build_type);
    run_step("build", &mut build)?;

    let package_prefix = target_root.join("package").join(&profile);
    println!("cargo:rustc-env=PANTA_CMAKE_EXECUTABLE={}", cmake.display());
    println!(
        "cargo:rustc-env=PANTA_NATIVE_BUILD_DIR={}",
        binary_dir.display()
    );
    println!(
        "cargo:rustc-env=PANTA_PACKAGE_PREFIX={}",
        package_prefix.display()
    );
    println!("cargo:rustc-env=PANTA_NATIVE_BUILD_TYPE={build_type}");

    let exe_name = if cfg!(windows) { "panta.exe" } else { "panta" };
    locate_product(&binary_dir, build_type, exe_name)
}

fn locate_product(binary_dir: &Path, build_type: &str, exe_name: &str) -> Result<PathBuf, String> {
    let app_dir = binary_dir.join("app");
    // Ninja 等单配置生成器通常把产物放在 app/；多配置生成器可能使用
    // app/<Config>/ 或 <Config>/；部分 clang-cl/Ninja 组合直接写入构建树根部。
    // 所有候选都在 target/native/<profile> 下，不能把系统目录当作回退位置。
    let candidates = [
        app_dir
            .join("panta.app")
            .join("Contents")
            .join("MacOS")
            .join(exe_name),
        app_dir.join(exe_name),
        app_dir.join(build_type).join(exe_name),
        binary_dir.join(build_type).join(exe_name),
        binary_dir.join(exe_name),
    ];
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            let expected = candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" 或 ");
            format!(
                "CMake 构建成功但未找到产物（尝试：{expected}）；检查 native/app 的 OUTPUT_NAME 与生成器配置"
            )
        })
}

fn ffi_artifacts(out_dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    let include = std::env::var_os("DEP_PANTA_FFI_INCLUDE")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo 未提供 DEP_PANTA_FFI_INCLUDE，panta-ffi 桥接头不可用".to_owned())?;
    if !include.join("panta_ffi.h").is_file() {
        return Err(format!(
            "panta-ffi 生成头不存在：{}",
            include.join("panta_ffi.h").display()
        ));
    }

    // build.rs 的 OUT_DIR 位于 target/<profile>/build/<pkg-hash>/out；Cargo
    // staticlib 既可能在 profile 根，也可能在 deps/。两处均可
    // 留有旧制品；按修改时间取最新，避免新桥接头配上旧 staticlib。
    let profile_dir = out_dir.ancestors().nth(3).ok_or_else(|| {
        format!(
            "无法从 launcher OUT_DIR 推导 profile：{}",
            out_dir.display()
        )
    })?;
    let deps_dir = profile_dir.join("deps");
    let mut candidates = Vec::new();
    for directory in [profile_dir, deps_dir.as_path()] {
        let entries = fs::read_dir(directory)
            .map_err(|error| format!("读取 panta-ffi 产物目录 {}：{error}", directory.display()))?;
        candidates.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                        return false;
                    };
                    let is_library = path
                        .extension()
                        .is_some_and(|extension| extension == "a" || extension == "lib");
                    is_library
                        && (name.starts_with("libpanta_ffi") || name.starts_with("panta_ffi"))
                }),
        );
    }
    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    let staticlib = candidates.into_iter().next_back().ok_or_else(|| {
        format!(
            "未找到 panta-ffi staticlib（检查 {} 与 {}）",
            profile_dir.display(),
            deps_dir.display()
        )
    })?;
    Ok((include, staticlib))
}

fn run_step(name: &str, command: &mut Command) -> Result<(), String> {
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(match status.code() {
            Some(code) => format!("CMake {name} 失败（退出码 {code}），诊断见上方输出"),
            None => format!("CMake {name} 被信号终止，诊断见上方输出"),
        }),
        // program not found 等启动错误在此浮出，并给出可定位的处置指引。
        Err(error) => Err(format!(
            "无法执行 {:?}：{error}。请检查托管工具缓存与平台 SDK；参见 ai-docs/standards/dependency-acquisition.md",
            command.get_program()
        )),
    }
}

fn required_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("Cargo 未提供环境变量 {name}，构建脚本运行环境异常"))
}
