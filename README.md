# vtk-test

[![CI](https://github.com/Yuki-Nagori/vtk-test/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Yuki-Nagori/vtk-test/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

本仓库是最小可视化渲染测试仓库，从 Panta 代码起步，应用与 crate 沿用 `panta` 名称（维护者已决定不整体改名，见 task 005）。核心是模型导入、几何 / 网格处理与 VTK 渲染；project 用于组织测试模型、资产和场景。

当前保留 Qt Quick / C++ ViewModel 宿主、Rust 工程创建 / 打开 / 保存、STL 导入与旁置网格资产、VTK WebGPU 原生视口，以及 OCCT STEP 和 Netgen 体网格适配与测试。STEP 接入工程、网格生成交互和网格修复仍需后续实现与验证，不把适配冒烟当作完整工作流。后续还需导入已求解数据，将结果场与网格、材料区域和边界标签关联后可视化，见[模型与结果数据设计](ai-docs/architecture/study-and-automation.md)。界面直接使用中文，已收敛为工程工具栏、模型列表与 VTK 视口；分析设置等旧界面已移除，工程中原有设置字段暂保留。未加载模型时视口留白，不创建默认 Welcome 场景。

VTK 与 OCCT/Netgen 继续使用已发布 Release 的 SDK，下载地址和 SHA256 由 `native/cmake/sdk-provision.cmake` 锁定。本仓库不保留 SDK 生产 workflow 或构建脚本，来源记录保留在 `tools/sdk/releases/`。

## 快速开始

安装下表所列基础工具后，在仓库根目录运行：

```sh
cargo build --locked
cargo run --locked
```

Cargo 会按锁定清单自动获取 CMake、Ninja、Qt 和 native SDK 到 `target/`。首次构建需要网络访问。Rust 版本见 [rust-toolchain.toml](rust-toolchain.toml)，依赖来源、版本和平台限制见[依赖获取规范](ai-docs/standards/dependency-acquisition.md)。

## 系统要求

| 平台 | 需要预装 |
|---|---|
| macOS（arm64；目标基线 13.0） | git、[rustup](https://rustup.rs)、Apple 命令行工具（`xcode-select --install`） |
| Linux | git、[rustup](https://rustup.rs)、C 编译器及 OpenGL/Wayland 前置；Debian/Ubuntu 需要 `pkg-config`、`libgl-dev`、`libegl1`、`libxkbcommon0`、`libxkbcommon-dev`、`libwayland-dev`、`wayland-protocols`，运行 QML 另需 `libgl1-mesa-dri`。Qt 预编译包要求 glibc ≥ 2.34。 |
| Windows | git、[rustup](https://rustup.rs)、Visual Studio Build Tools 2022（MSVC） |

## 常用命令

| 命令 | 用途 |
|---|---|
| `cargo build --locked` | 构建默认成员 `panta-launcher` 及其依赖，并生成桌面程序 `panta`（Windows 为 `panta.exe`）。 |
| `cargo build --locked --workspace` | 构建 workspace 全部成员。 |
| `cargo run --locked` | 启动桌面窗口并转发程序参数与退出码。 |
| `cargo run --locked -p panta-cli -- project inspect <工程文件.panta>` | 读取工程清单并输出名称、路径、修订号及已导入资产 ID、显示名和来源摘要；与 GUI 共用 Rust 打开工程命令。 |
| `cargo run --locked -p panta-cli -- project create <父目录> <工程名>` | 创建 `<父目录>/<工程名>/<工程名>.panta`；复用 GUI 工程服务的名称校验和冲突保护。 |
| `cargo run --locked -p panta-cli -- mesh inspect <网格文件> [--weld-tolerance <相对比例>]` | 只读预检表面网格并输出来源名、面片数、尺寸和表面网格质量诊断（近重复/未焊接顶点、退化面、孔、非流形边、法向不一致）。支持 STL、VTU、VTP、旧版 `.vtk`（POLYDATA / UNSTRUCTURED_GRID）、OBJ、PLY；体单元只保留外表面。解析规则与 GUI 导入预检共用，尺寸沿用来源坐标（来源不记录单位）。焊接容差默认为包围盒对角线 × 1e-6，可用 `--weld-tolerance` 调整。`stl inspect` 为同义别名。 |
| `cargo run --locked -p panta-cli -- mesh import <工程文件.panta> <网格文件> [--mesh-type <id>] [--units <unit>]` | 将上述格式的网格导入现有工程，默认网格类型沿用领域设置、单位为毫米；来源文件不复制进工程，展开 Mesh 估算超过 2 GiB 时提醒但仍允许导入。`stl import` 为同义别名。 |
| `cargo panta-package` | 生成当前 profile 的可搬迁安装目录，默认位于 `target/package/<profile>`。 |
| `cargo panta-package /path/to/output` | 将安装目录写到指定路径。macOS 输出 `panta.app`，Windows/Linux 输出 `bin/panta[.exe]`。 |
| `cargo test --locked --workspace` | 运行 Rust、native、QML 聚合测试。 |
| `cargo format` | 修复 Rust、C++ 和 QML 格式；`--check` 仅检查。 |
| `cargo lint` | 运行 Rust Clippy 和 QML lint；`--check` 仅检查。 |
| `cargo quality` | 依次运行格式检查、Clippy、QML lint 和完整行为测试。 |

`cargo panta-package` 通过 Qt 部署脚本收集 Qt/QML 运行时，并由 CMake 安装 app 实际链接的 native 动态依赖及许可清单；不会带上 native 开发用静态库、头文件和 CMake 导出。它生成安装目录，不生成 DMG、MSI 或 DEB 安装器，也不做 Developer ID 签名或公证。macOS arm64 的本地安装目录已做独立启动和真实窗口检查；下载的 CI 测试包会校验 app 资源签名完整性，但未公证，首次启动时 macOS 可能显示安全确认。Windows/Linux 的安装目录和目标机窗口仍待验收。

PR 与 main 推送的 GitHub Actions（`ci.yml`）只在 Linux x86_64 构建并运行完整行为测试，另有 Ubuntu quality job 检查格式、Clippy 和 qmllint；macOS arm64 与 Windows x86_64 的完整行为测试移至 `nightly.yml`，每天 18:00 UTC（北京时间 02:00）定时运行，也可在 Actions 页面手动触发（workflow_dispatch）。本仓库不自动打包或生产 SDK；测试安装目录通过本地 `cargo panta-package` 生成。

Release 安装目录可用 `cargo run --locked --release --package panta-launcher -- package` 生成到 `target/package/release`。

Cargo alias 定义在 [.cargo/config.toml](.cargo/config.toml)。Cargo 自带的 `cargo package` 保持原有 Rust crate 打包语义，因此桌面安装命令使用独立名称 `cargo panta-package`。

## 构建与测试

Cargo 负责 Rust workspace 和 native 构建调度；CMake 管理 C++、Qt/QML 和桌面程序构建图。native 诊断命令也可直接在 `native/` 下运行：

```sh
cmake --preset debug
cmake --build --preset debug
ctest --preset debug
cmake --install ../target/native/debug --prefix /path/to/output
```

日常构建可按 Cargo 常规行为解析依赖；CI 和可复现验证使用 `--locked`。跨语言测试、质量门禁和任务约束见 [AGENTS.md](AGENTS.md)、[测试规范](ai-docs/standards/testing.md)和[任务索引](ai-docs/task-index.md)。

## 项目说明

项目采用 Apache-2.0，第三方依赖按各自许可分发，见[依赖获取规范](ai-docs/standards/dependency-acquisition.md)。

- [架构总览](ai-docs/architecture/README.md)
- [构建、开发与部署](ai-docs/architecture/build-and-development.md)
- [重要模块说明](ai-docs/modules/README.md)
- [实施任务索引](ai-docs/task-index.md)
