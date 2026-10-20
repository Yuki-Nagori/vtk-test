use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use panta_build as provision;

fn main() -> Result<(), io::Error> {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-env-changed=PANTA_USE_SYSTEM_TOOLS");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-env-changed=CLANG_FORMAT");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Cargo 必须设置 OUT_DIR"))?,
    );
    let target_root = provision::target_root(&out_dir).map_err(io::Error::other)?;
    let llvm = provision::resolve_llvm_compilers(&target_root).map_err(io::Error::other)?;
    let compiler = match (
        env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc"),
        llvm.clang_cl.as_ref(),
    ) {
        (true, Some(path)) => path,
        _ => &llvm.clangxx,
    };

    let mut builder = cxx_build::bridge("src/lib.rs");
    let cxx_standard_flag = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        "/std:c++20"
    } else {
        "-std=c++20"
    };
    let cxx_exception_flag =
        (env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")).then_some("/EHsc");
    let sdk_env = provision::windows_sdk_env(&env::var("TARGET").map_err(io::Error::other)?)
        .map_err(io::Error::other)?;
    for (key, value) in sdk_env {
        builder.env(key, value);
    }
    if let Some(sdk) = provision::macos_sdk().map_err(io::Error::other)? {
        let deployment = env::var("MACOSX_DEPLOYMENT_TARGET")
            .unwrap_or_else(|_| provision::MACOS_DEPLOYMENT_TARGET.to_owned());
        builder
            .flag("-isysroot")
            .flag(sdk.as_os_str())
            .flag(format!("-mmacosx-version-min={deployment}"))
            .flag("-nostdinc++")
            .flag("-isystem")
            .flag(sdk.join("usr/include/c++/v1").as_os_str());
    }
    builder
        .compiler(compiler)
        .flag_if_supported(cxx_standard_flag);
    if let Some(flag) = cxx_exception_flag {
        // cxx 生成的错误边界通过 C++ exception 抛出 rust::Error；clang-cl
        // 默认关闭异常，必须显式启用与 MSVC ABI 一致的同步展开语义。
        builder.flag(flag);
    }
    builder.compile("panta_ffi_bridge");

    let generated_header = find_generated_header(&out_dir).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "cxx-build did not generate panta FFI header in {}",
                out_dir.display()
            ),
        )
    })?;
    let cxx_include = out_dir.join("cxxbridge").join("include");
    let public_header = cxx_include.join("panta_ffi.h");
    fs::copy(&generated_header, &public_header).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "copy generated CXX header {} to {}: {error}",
                generated_header.display(),
                public_header.display()
            ),
        )
    })?;

    // launcher/build.rs 将该头文件目录与 staticlib 一起传给 native CMake。
    println!("cargo:include={}", cxx_include.display());
    Ok(())
}

fn find_generated_header(root: &Path) -> Option<PathBuf> {
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
            } else if path.file_name().is_some_and(|name| name == "lib.rs.h") {
                return Some(path);
            }
        }
    }
    None
}
