# 依赖获取与主平台环境

更新日期：2026-10-20。Cargo 调度 CMake 并供给锁定工具与预编译 Qt / VTK / OCCT / Netgen SDK。应用消费已经发布的 Release；VTK、OCCT/Netgen 与 GoogleTest SDK 由本仓库 `sdk-*.yml` workflow 手动生产发布（任务 041），普通开发与 CI 不编译重库。版本与兼容性维护见 [技术基线](baseline.md)，下载 URL、SHA256 与平台映射以 `crates/panta-build`、`native/cmake/qt-provision.cmake` 和 `native/cmake/sdk-provision.cmake` 为准。本文记录获取、重建、升级与回退规则，不继承旧任务的验收状态。

## 托管原则（2026-09-16 决策）

- Cargo 是唯一依赖入口：`cargo build` 触发的构建引导负责把固定的预编译 CMake、Ninja、Qt、VTK、OCCT、Netgen 包缓存到工作区构建树（根 `target/` 下托管目录），不写入源码树。正常开发构建不得把第三方源码加入本地构建图。
- 预编译优先：优先使用上游或项目发布的、与目标平台、架构、编译器 ABI、Qt 版本和所需模块匹配的 SDK/二进制包。只有不存在合适的预编译包时，才可另立任务评估由 CI/维护者生成可复用制品；本地构建源码不是默认回退，也不能静默触发。
- 包不可用或匹配检查失败时立即给出版本、平台、架构、ABI、缺失 target 和获取入口诊断；不能退回系统库或偷偷开始长时间源码编译。
- 不把系统包管理器（Homebrew/MacPorts/apt 等）作为项目基线；个人机器上已安装的同名包只是开发便利，不能当作兼容性证据，也不得进入提交的构建配置。
- 开发者前置：git、rustup（工具链由根 [rust-toolchain.toml](../../rust-toolchain.toml) 固定，缺失时 `rustup toolchain install`）与所在平台的 SDK/运行库（macOS：Apple 命令行工具和 SDK；Linux：glibc/sysroot、C 编译器（Rust 链接驱动）与 OpenGL 前置——供给 Qt 的 `find_package(Qt6 Gui)` 经 `WrapOpenGL` 强依赖宿主 GL 开发文件，运行 QML 另需 libEGL/libxkbcommon 运行库与软件渲染驱动，Debian/Ubuntu 对应 `libgl-dev`、`libegl1`、`libxkbcommon0`、`libgl1-mesa-dri`，CI 由 ci.yml 平台前置步骤安装；Windows：MSVC Build Tools、Windows SDK 和 CRT）。自有 C++ 编译器由 Cargo 托管的 LLVM 22.1.7 供给，不依赖 PATH 中的 Apple Clang、GCC 或 cl.exe。首次构建需要联网，此后可离线重复构建；跨机器缓存只复用经过版本与摘要校验的依赖资产。
- 版本固定：源码 tag 附 commit SHA 校验；下载的预编译包附 SHA256，并记录目标平台、架构、编译器/运行库 ABI、Qt 兼容范围和模块清单。升级必须修改固定清单、完成对应集成验证后同一 commit 提交。
- 选版原则（2026-09-16 维护者决策）：尽量采用各上游最新稳定版，同时核对依赖间版本关系；受关系约束不能升级时，记录原因与重评条件。

## 历史环境参考

| 项目 | 值 |
|---|---|
| 操作系统 | macOS 26.3.1，arm64（Apple Silicon） |
| 编译器 | Apple clang 17.0.0（/Library/Developer/CommandLineTools，含 macOS 26.2 SDK） |
| C++ 标准库 | libc++（随工具链） |
| Perl | /usr/bin/perl 5.34.1（Qt syncqt 需要） |
| CMake / Ninja | 不要求预装；由引导按下方清单供给 |

上表是既有 Panta 的环境记录，不作为本仓库已经验收的证据。实际 C++ 编译器由 Cargo 托管 LLVM 供给；Apple CLT 提供平台 SDK 与运行库。当前平台的系统、图形后端和验收结果需另外记录，CI 配置见下文。

## 固定清单

| 依赖 | 固定版本（tag） | 来源 | 校验 | 获取形态 | 关键选项 | CMake package / targets | 许可证入口 | 消费验证要求 |
|---|---|---|---|---|---|---|---|---|
| CMake | 4.4.3 | `github.com/Kitware/CMake` release 资产：macOS `cmake-4.4.3-macos-universal.tar.gz`（universal，SHA256 `0c5d65251c14cc884bfa16bdbed3c263ce5bffe2e21c0d0d00962cb0610464fa`）、Linux x86_64 `cmake-4.4.3-linux-x86_64.tar.gz`（`d6c83076c575bc00b823522ac974bda66d0af05d6ddc30e739c12385cf32c6cc`）、Windows x86_64 `cmake-4.4.3-windows-x86_64.zip`（`4d52ebab7193a698651639ed80d8d04fd903358843572cf44c7fd234cb7c26ab`）；SHA256 为 2026-09-17 下载实测 | 官方资产 SHA256（见来源及 `panta-build` 固定清单） | 官方二进制解包：默认托管缓存/下载并校验；不支持固定资产的平台才允许显式旁路；解包用平台自带 `tar` | — | 可执行工具（非 CMake 包） | BSD-3（发布包 Copyright.txt） | 首次 / 增量 configure、配置切换与失败诊断 |
| Ninja | 1.13.2 | `github.com/ninja-build/ninja` release 资产：`ninja-mac.zip`（SHA256 `c99048673aa765960a99cf10c6ddb9f1fad506099ff0a0e137ad8960a88f321b`）、`ninja-linux.zip`（`5749cbc4e668273514150a80e387a957f933c6ed3f5f11e03fb30955e2bbead6`）、`ninja-win.zip`（`07fc8261b42b20e71d1720b39068c2e14ffcee6396b76fb7a795fb460b78dc65`）；SHA256 为 2026-09-17 下载实测 | 官方资产 SHA256（见来源及 `panta-build` 固定清单） | 预编译 release 资产；解包用 `cmake -E tar`（libarchive，三平台零额外工具） | — | 可执行工具 | Apache-2.0（仓库 COPYING） | 增量构建与失败码传播 |
| LLVM/Clang | 22.1.7 | `github.com/llvm/llvm-project` release 资产：macOS ARM64 `LLVM-22.1.7-macOS-ARM64.tar.xz`（SHA256 `4177245188b0a30a6539c96b361dea56f253485756bfd8927a6a59e7301e7806`）、Linux x86_64 `LLVM-22.1.7-Linux-X64.tar.xz`（`edb0522b41e261819c06ea437d249f9b8acfa413d3805bc9920eec6fb76ff830`）、Windows x64 `LLVM-22.1.7-win64.exe`（`e091fcf965ce589c83c0f7c5356b2fcf3e658a8ec990bfcf79cce4389a0d1eb3`） | 归档 SHA256（固定于 `panta-build` 资产清单） | Cargo build.rs 下载到 `target/panta-tools/llvm` 并校验；Unix 解包归档，Windows 通过官方 NSIS 安装包静默安装；Unix 使用 clang/clang++，Windows 使用 clang-cl；clang-format 与编译器来自同一目录 | clang / clang++ / clang-cl；格式与编译工具同源 | 可执行工具（非 CMake 包） | Apache-2.0 with LLVM exceptions（发布包 LICENSE） | 实际编译器路径、ABI 与质量工具同源 |
| Qt | 6.11.2（qtbase + qtdeclarative + qtsvg 预编译包，含 Qml/Quick/QuickControls2/Test 与 qtsvg 的 qsvg 图像格式插件（SVG 图标解码）；qt5compat 等未取，需要时按 task 扩展） | `download.qt.io/online/qtsdkrepository/{mac_x64,linux_x64,windows_x86}/desktop/qt6_6112/`（与在线安装器同源、免账号；三端 URL 与归档名见 `native/cmake/qt-provision.cmake`，以脚本为准；官方源下载失败或哈希不符时自动回退清华 TUNA 镜像，`PANTA_QT_MIRROR` 环境变量（镜像站点根或 qtsdkrepository 仓库根）可把其他同构镜像插到最前） | SHA256（2026-09-16/17 下载实测，硬编码于供给脚本并强校验；镜像来源同样强制校验） | 预编译 7z 解包（`cmake -E tar`，三平台零额外工具）；维护者决策（2026-09-16）：不源码构建。qtsvg 归档（SHA256 于 2026-09-22 下载实测，并经官方 .meta4 交叉核对）提供 `plugins/imageformats/qsvg` 供 Image 渲染 SVG 图标资源 | 供给缓存于 `target/panta-deps/qt/staging`（跨 profile、Cargo 与 presets 共享；每归档解包指纹在 `qt/extracted/`，升级只补新归档）；安装版布局为平铺 bin/lib | `find_package(Qt6 6.11 COMPONENTS Core Gui Qml Quick QuickControls2 Test)` | LGPL-3.0（发布包 LICENSES/） | 资源、中文控件 / SVG 加载与安装部署 |
| Qt Linux ICU runtime | 73.2（与 Qt 6.11.2 Linux 工具 ABI 匹配） | Qt 官方 qtsdkrepository `6.11.2-0-202608131018icu-linux-Rhel8.6-x86_64.7z` | SHA256 `111bdae30a66fff6ef65620e95766170fa5a6f425c360ea33b79cb2ec7e2fd86` | 预编译 7z 解包至 `qt/staging/lib`；仅 Linux 使用，不使用系统 ICU 或源码构建 | — | `rcc`、`qtpaths`、`qmlimportscanner` 运行时依赖 | ICU 许可证随归档 | Linux Qt 工具可运行且不依赖系统 ICU ABI |
| VTK | 9.7.0（commit `23f0a095621e91bbdbeace8451e22b950c8e5f46`，tag `v9.7.0` 解引用，unmodified 上游源码） | 本仓库自产制品（Release `sdk-vtk-9.7.0-webgpu`，任务 041 迁入生产管线后 2026-10-24 workflow run [40099745820](https://github.com/Yuki-Nagori/vtk-test/actions/runs/40099745820) 三平台生产/自检/发布，macOS 13.0 最低部署目标，新增 RenderingAnnotation）：macOS `vtk-9.7.0-macos-arm64.tar.gz`（SHA256 `515c61a363d921e4159ab31c660d15ff36c662945ec794a417984200880cf815`）、Linux `vtk-9.7.0-linux-x86_64.tar.gz`（`8fc89f1633b30be69ea218dae8c6eda652112122e14ab497ea2674509107c35c`）、Windows `vtk-9.7.0-windows-x86_64.tar.gz`（`5a088f5bed4fc02cf8687991be830c72cbe40da2a52674021f9eedbc27267032`）；SHA 已按 sidecar 和 Release API digest 更新；完整 URL 见 `native/cmake/sdk-provision.cmake` 清单，合规说明见 `tools/sdk/releases/vtk-9.7.0.md` | 预编译归档 SHA256（2026-10-24 Release sidecar 与 API digest 核对，manifest 强校验）；源码 tag/commit 作为来源依据 | 预编译制品消费（`panta_require_sdk(vtk)` 三平台登记；历史制品的 macOS consumer 烟测、native configure/build 与离线复用通过；新摘要消费端 CI 待复验）。WebGPU/共享库；Linux 由 ubuntu-24.04 gcc13 生产，Wayland-only，glibc 和窗口运行兼容性需在目标平台验证 | `VTK::RenderingWebGPU`、`VTK::RenderingUI`、`VTK::RenderingAnnotation`、`dawn::webgpu_dawn`；原生窗口分别为 Cocoa、Wayland、Win32 | `find_package(VTK CONFIG)` 与 `find_package(Dawn CONFIG)`；`VTK::`/`dawn::` targets | BSD-3（VTK）与 Dawn 上游许可证，均随包 | 原生窗口、输入、图形后端与运行时加载；真实窗口验收 |
| OCCT | 8.0.1（tag `V8.0.1`，commit `b8f597c677811d1f9f4d8a97f5ae2825c0353a42`，2026-07-30，当前最新 release） | 本仓库自产制品（Release `sdk-occt-netgen-8.0.1-6.2.2604`，2026-10-24 workflow run [40099749860](https://github.com/Yuki-Nagori/vtk-test/actions/runs/40099749860) 三平台生产/自检/发布，配置与原 Panta 一致，macOS 13.0 最低部署目标；维护者决策：官方仅 Windows 有归档、跨平台工具链不一致，三平台统一自托管，官方归档不再消费）：macOS `…/occt-8.0.1-macos-arm64.tar.gz`（SHA256 `6ee6314b739dbfc4d3aaa4ca3a69f610405f28d388e9862fc6e5320b5c7eee25`）、Linux `…/occt-8.0.1-linux-x86_64.tar.gz`（`2f0077b82fc0e3ccdf85665be9dd0c3fdefc9009eb90ef684db14491d9a5ee92`）、Windows `…/occt-8.0.1-windows-x86_64.tar.gz`（`b197cf4e4b66f56d30d16d0abcbe8896773aaffab69ef95cad51ac7b06feab81`）；SHA 已按 sidecar 和 Release API digest 更新，完整 URL 见 `native/cmake/sdk-provision.cmake` 清单，配对约束见"依赖间版本关系" | 预编译归档 SHA256（2026-10-24 Release sidecar 与 API digest 核对，manifest 强校验）；tag SHA 作为来源依据 | 预编译制品消费（`native/cmake/sdk-provision.cmake` 清单 三平台登记；历史 macOS 生产 consumer 烟测通过；新摘要消费端 CI 待复验）。Release/Shared；模块 Draw/Visualization/DETools 与 USE_FREETYPE/USE_XLIB 关闭（渲染归 VTK、零系统第三方依赖），其余默认启用（含 STEP 所需 DataExchange）；模块裁剪须以 STEP 实际消费验证为依据 | SDK 必须提供 `find_package(OpenCASCADE)`（已验证：targets 无命名空间，`TKernel`/`TKDESTEP` 等） | `find_package(OpenCASCADE CONFIG)` | LGPL-2.1 + OCCT 例外（随包 `share/licenses/OCCT/`） | STEP 导入、单位 / 摘要与异常；Windows 嵌套 DLL 路径由 `native_test_env` 递归解析 |
| Netgen | v6.2.2604（commit `3ee489c7d58fdbc2a6708cca3cbaefaae506dc17`，当前最新 release） | 本仓库自产制品（与 OCCT 同 Release `sdk-occt-netgen-8.0.1-6.2.2604` 成对发布；2026-10-24 workflow run [40099749860](https://github.com/Yuki-Nagori/vtk-test/actions/runs/40099749860) 三平台生产/自检/发布）：macOS `…/netgen-6.2.2604-macos-arm64.tar.gz`（SHA256 `2c8ef67a6101a63f3e24b8cbf6a328433bd66d0fc959337dd1cadedc943c24f7`）、Linux `…/netgen-6.2.2604-linux-x86_64.tar.gz`（`20a2f3cd3b6c475d5efdfdaf1539038e9433ca23d565e7e679b59b18da0a36d5`）、Windows `…/netgen-6.2.2604-windows-x86_64.tar.gz`（`8b2db13e9b122a43086a42a471140c2319a3ae3e92408ea077c8276bb9184183`）；SHA 已按 sidecar 和 Release API digest 更新，完整 URL 见 `native/cmake/sdk-provision.cmake` 清单，配对约束见"依赖间版本关系" | 预编译归档 SHA256（2026-10-24 Release sidecar 与 API digest 核对，manifest 强校验）；tag SHA 作为来源依据 | 预编译制品消费（`native/cmake/sdk-provision.cmake` 清单 三平台登记；历史 macOS 生产 consumer 烟测通过；新摘要消费端 CI 待复验）。`USE_OCC=ON` 链接同 Release 的 OCCT（成对升级）；GUI/Python/MPI/JPEG/MPEG 关闭；**包配置文件为 `NetgenConfig.cmake`（大写 N）——`find_package` 必须用 `Netgen`**（Linux ext4 大小写敏感）；运行期依赖同平台 OCCT 资产（消费侧负责处理加载路径） | `ngcore`/`nglib` imported targets（无命名空间）；与 OCCT ABI 匹配由成对发布保证 | `find_package(Netgen CONFIG)` | LGPL-2.1（随包 `share/licenses/Netgen/LICENSE`） | 真实几何到体网格、结构 / 方向及销毁验证 |
| GoogleTest（仅测试） | v1.18.0（上游 commit `063de7e9578f82b369302001269680b4b1553359`） | 上游 `github.com/google/googletest`；仓库 Release `sdk-googletest-1.18.0` | 三平台归档 SHA256（2026-10-24 本仓库重产，workflow run [40099754184](https://github.com/Yuki-Nagori/vtk-test/actions/runs/40099754184)，sidecar 与 Release API digest 核对）：macOS arm64 `58996fdc1b268dfd186a546a66a7824b0eb34d2914fbbafbf9689197f1823d83`；Linux x86_64 `7bdf2a49af0cf7ca99bad99c8a42a1a5f5612284da1e8d120dc76b8d39294c56`；Windows x86_64 `82f38c72edec8737abed9ca5ab29ac9241d45fef9746686554a7119b199727b8` | 三平台自建静态 SDK（本仓库 `sdk-googletest.yml`）；CMake `panta_require_sdk(googletest)` 仅在 `BUILD_TESTING=ON` 时下载/校验/解包到 `target/panta-deps/sdk/googletest/1.18.0/<triple>`，可由 Cargo 缓存目录共享 | 上游 CMake 安装；C++17、static、`BUILD_GMOCK=OFF`、Windows MSVC 2022 x64 Release `/MD` | `find_package(GTest CONFIG)`；`GTest::gtest` / `GTest::gtest_main` | BSD-3-Clause：归档内 `share/licenses/GoogleTest/LICENSE` | CTest 发现、断言失败传播与测试运行时依赖 |

固定版本、摘要与构建参数以源码清单为准；本表保留来源与消费边界。修改依赖后在本仓库执行消费验证，不能将制品生产自检等同于当前应用已通过。预编译 SDK 自任务 041 起由本仓库 `sdk-*.yml` 生产与发布（`gh workflow run sdk-vtk.yml -f publish=true` 等），流程见 `tools/sdk/` 与各发布说明。

## 依赖间版本关系（2026-09-16 核实）

- **Netgen ↔ OCCT**：Netgen v6.2.2604 通过 `find_package(OpenCASCADE)` 接入 OCCT 8.0.1。源码版本守卫采用 `NETGEN_OCC_VERSION_AT_LEAST` 风格，但守卫本身不能证明网格正确性，仍须验证真实 STEP 到体网格。OCCT 不承诺跨版本 C++ ABI 稳定；Netgen 制品只与构建时所链接的 OCCT 兼容，因此消费同一 Release 成对发布的 SDK（`sdk-occt-netgen-<occt>-<netgen>`），升级必须成对验证。历史候选 OCCT `V7_9_3`（commit `a016080bf673`）只是来源记录，不能在失败时静默切换到未验证的另一组合。
- **VTK ↔ Qt**：VTK 9.7.0 WebGPU SDK 不依赖 Qt VTK GUI 模块；Qt 6.11.2 提供 QML/Quick 宿主，原生 view/layer/surface 由 VTK 平台桥接接入。SDK 必须提供 `VTK::RenderingWebGPU`、`VTK::RenderingUI`、`dawn::webgpu_dawn` 和对应 hardware window；实际窗口、输入和运行时加载需独立验证。没有匹配 SDK 时明确报告缺失资产，不在普通开发构建中转为 VTK 源码生产。
- **全体 ↔ CMake 4.4**：清单内项目声明的 CMake 下限均 ≥3.10，高于 CMake 4 移除的 <3.5 兼容线；修改后需验证首次 configure。
- Rust 侧无 native 版本耦合；CXX 自身的 MSRV（1.88）低于固定工具链 1.99.0，但本仓库源码与 workspace `rust-version` 要求 Rust 1.99，约束见 [Rust 规范](rust.md)。

## CI

- GitHub Actions workflow [ci.yml](../../.github/workflows/ci.yml)：push 到 main 与 pull request 触发，仅 `ubuntu-latest`；[nightly.yml](../../.github/workflows/nightly.yml) 每天 18:00 UTC 定时及手动触发，矩阵 `macos-latest` / `windows-2022`。三平台统一 Ninja；Windows 使用托管 clang-cl、MSVC Build Tools/Windows SDK 环境与 Qt MSVC2022 预编译包；CI 不自行安装项目工具。CMake/Ninja/Qt 由 Cargo 构建或质量入口按需供给；GoogleTest 在 `BUILD_TESTING=ON` 时由 `sdk-provision` 下载并校验对应平台固定 SDK。Linux 上 Qt 预编译包的 configure 与 QML 运行依赖宿主 OpenGL 前置（见"开发者前置"），由 workflow 平台前置步骤以 apt 安装，属宿主能力而非项目依赖。构建引导的 curl 下载带 connect/speed/max 上限，任务级 `timeout-minutes` 兜底传输停滞。
- Qt 6.11.2 Linux 预编译归档面向 RHEL9，Qt 工具（包括 `rcc`、`qtpaths`、`qmlimportscanner`）需要 ICU 73。`native/cmake/qt-provision.cmake` 同步下载并校验 Qt 官方的 ICU 73 预编译归档，解包到 `qt/staging/lib`，让 Ubuntu 使用与 Qt 工具匹配的 ABI；不使用系统 ICU、不伪造 SONAME，也不源码编译 ICU。Linux 仍跳过仅供 IDE 使用的 `.qmlls.build.ini` 和当前 app 的空 import scan，保留 QML typeinfo、cachegen、资源和运行时验证。
- PR 与 main push 在 Linux、nightly 在 macOS / Windows 安装 rust-toolchain.toml 中的固定 Rust 工具链并执行 `cargo test --locked --workspace`，其中包含 native/QML 构建和完整行为测试；单一 Ubuntu quality job 运行 `cargo format --check`、`cargo lint clippy --check` 和 `cargo lint qmllint --check`。基础检查与验收边界见[质量工具链](../modules/quality-tooling.md)。
- runner 需要镜像自带的平台编译器与 rustup；平台编译器和标准库属于 Cargo 无法替代的宿主能力，项目依赖仍由 Cargo 驱动的构建引导供给。
- 边界：CI 覆盖以 workflow 和实际测试清单为准；VTK、OCCT、Netgen 的 SDK 生产由本仓库手动 `sdk-*.yml` workflow 完成并自检，PR/nightly 验证消费、链接与已有测试，真实 GPU 窗口另行验收。

## 干净重建步骤

当前流程由 launcher、`panta-build` 和 native 供给脚本共同实现：

1. 按 [README 系统要求](../../README.md#系统要求) 安装平台前置（macOS：Apple CLT；Linux：C 编译器与 OpenGL 前置；Windows：MSVC）与 rustup；CMake/Ninja/Qt/VTK/OCCT/Netgen 由固定预编译供给处理。
2. 在仓库内执行 `rustup toolchain install 1.99.0`（或首次 cargo 命令时按 rustup 提示安装）。
3. `cargo build --locked`：launcher 的 build.rs 调度 CMake/Ninja，消费构建树内固定的预编译依赖（缺失或 ABI 不匹配时立即失败并给出诊断）；`cargo run` 启动 `panta`（Windows 为 `panta.exe`）并转发参数与退出码。VTK/OCCT/Netgen SDK 由 `native/cmake/sdk-provision.cmake` 清单 供应用例引擎消费；GoogleTest v1.18.0 也已在 `native/cmake/sdk-provision.cmake` 登记，仅构建测试时从 Release 下载并强校验，统一缓存于 `target/panta-deps/sdk/`。

验证入口见 [构建与开发](../architecture/build-and-development.md)；各依赖的消费验证要求见固定清单最后一列。

## 升级与回退

- 单个依赖升级：修改本文固定 tag/SHA → 核对与其它依赖的版本关系（尤其 Netgen ↔ OCCT）→ 按受影响范围执行最小验证集（几何/网格/视口冒烟）→ 回填 SHA256 与实测结果 → 清单、task 与受影响规范同一 commit 提交。
- 回退：恢复上一版固定清单并按干净重建重建；构建树可整目录删除，不影响源码树。OCCT / Netgen 回退仍需恢复已验证的完整配对组合。
- Rust 工具链升级规则见 [Rust 规范](rust.md)。

## 已知风险与显式未验证项

- SDK 是否可获取、摘要是否匹配、目标架构和 ABI 是否兼容均由消费端检查；资产缺失不得触发隐式源码构建。
- Netgen v6.2.2604 与 OCCT 8.0.1 的运行期对象生命周期和网格正确性需持续回归；适配器兼容处理见 [Netgen](netgen.md)。
- Qt Quick 与 VTK 原生 WebGPU surface 的叠加、缩放和输入须以真实窗口验证，不能用 offscreen QML 测试替代。
- 下载摘要必须与实际资产核对，依赖升级同时更新源码清单、本文和对应验证记录。

## 质量工具供给

`panta-build` 是 launcher、FFI 与根质量 runner 共享的支持 crate。LLVM/CMake/Ninja 固定资产集中登记在 `src/assets.rs`，由 `src/install.rs` 按版本和 SHA256 隔离并加锁安装；根公开 API 保持。Rust、C++ 和 QML 保留基础格式检查，Rust 使用 Clippy，QML 使用 qmllint；CMake 通过真实 configure/build/CTest 验证，不再引入单独的格式或 lint 运行时。详细边界与入口见[质量工具链](../modules/quality-tooling.md)。

CI 缓存只保存 Cargo registry/git 和 `target/panta-tools`、`target/panta-deps` 这类可验证的依赖资产；`target/native` 与 Cargo 编译产物每次在当前 checkout 重新生成，避免旧 compile database、CMakeCache 或增量对象从 restore key 泄漏。缓存键包含平台、架构、LLVM 版本、Cargo/原生供给清单；回退命中只能复用目录内带版本和摘要校验的资产。Windows 的 LLVM 官方安装包及其已校验安装目录因此会随 `target/panta-tools` 缓存复用。

托管工具归档下载以父进程限制含重试的总耗时为 30 分钟，解包或 Windows 安装上限为 20 分钟；超时终止并回收直接子进程，保留临时目录供下次持锁清理，不发布 `.complete`。安装锁、归档检查、摘要校验、解包和发布均有阶段日志；长时间运行的下载、解包或安装每 30 秒报告耗时。

整体上限不能仅用 curl `--max-time` 替代：它在每次重试时重置，`--retry-max-time` 也不截断已经开始的传输。供给实现须同时限制传输和外层进程等待，超时保留可定位诊断。机制依据：[curl 手册](https://curl.se/docs/manpage.html#--max-time)、[Cargo build script 输出](https://doc.rust-lang.org/cargo/reference/build-scripts.html#life-cycle-of-a-build-script)。
