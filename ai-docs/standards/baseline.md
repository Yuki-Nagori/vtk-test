# 技术基线与版本决策

更新日期：2026-10-20。本文记录最小可视化渲染测试仓库的技术选择；实现和版本以根 manifest、工具供给代码及 native SDK 清单为准。既有 Panta 的 Release 来源与历史验证只说明依赖背景，本仓库的构建、几何 / 网格和真实窗口验收须单独记录。

## 当前基线

| 项目 | 项目选择 | 验证重点 |
|---|---|---|
| 平台 | macOS / arm64 为本地窗口验收平台；PR CI 覆盖 Linux，nightly 覆盖 macOS/Windows | 记录实际系统、SDK、编译器及图形后端，不把 CI 构建当成真实窗口验收 |
| C++ | 自有代码 C++20；Cargo 托管 LLVM 22.1.7；平台 SDK / 标准库由宿主提供 | 部署目标、编译器选项、ABI 与平台运行库 |
| C++ 测试 | GoogleTest v1.18.0（三平台固定 SDK，仅 `BUILD_TESTING`；规则见 [gtest](gtest.md)） | native 测试聚合、错误传播与真实覆盖范围 |
| Rust | stable 1.99.0，edition 2024；`rust-version` 下限 1.99 | 锁定工具链与依赖的 MSRV |
| Cargo | 主开发入口，launcher 调度 native 构建 | 首次 / 增量构建与配置切换 |
| CMake + Ninja | CMake 4.4.3 + Ninja 1.13.2，由 Cargo 引导供给 | 配置、最终链接、安装与失败诊断 |
| Qt | 6.11.2 预编译包，包含 Qt Quick 与所需工具 / 插件 | 轻量 QML 测试界面、资源加载与部署 |
| VTK | 9.7.0 WebGPU，三平台预编译 hardware-window SDK | 原生 surface/view 与 Qt Quick 叠加、输入、显示缩放与实际帧呈现 |
| OCCT | 8.0.1，消费与目标 ABI 匹配的预编译 SDK | STEP 导入、单位与元数据、几何生命周期 |
| Netgen | v6.2.2604，与 OCCT 成对发布的预编译 SDK | 真实几何到体网格、结构 / 方向校验及销毁 |
| Rust/C++ FFI | CXX 1.0.202，CMake 最终链接 | DTO、错误、所有权及生命周期 |

精确 tag、commit SHA、来源 URL、构建选项、依赖间版本关系和许可证入口见 [依赖获取与主平台环境](dependency-acquisition.md)。

## 依赖获取与版本固定

Cargo 统一托管：开发者安装 git、rustup 与平台 SDK / 运行库后，`cargo build` 触发的引导按固定清单获取预编译依赖到构建树。不以 Homebrew 等系统包管理器作为项目基线；本机同名包只是个人便利，不构成兼容性证据。git tag 以 commit SHA 校验，预编译包以 SHA256 及平台 / 架构 / ABI 元数据校验。

VTK、OpenCASCADE、Netgen 和 Qt 默认使用匹配的预编译 SDK。这些 SDK 由本仓库 `sdk-*.yml` workflow 手动生产并发布（任务 041 自原 Panta 迁入）。缺少匹配包时，先明确制品来源与验证方案；普通 `cargo build` 不隐式编译这些重库。OCCT 与 Netgen 必须成对升级，不能独立替换其中一个二进制。

## 决策记录要求

当前工作目录所在机器不自动构成所有发布平台的承诺。依赖升级或新增时记录精确版本 / tag、来源与校验信息、链接方式、启用模块、CMake package / targets、许可证入口和对应测试。

安装完成或 configure 成功不能单独证明 ABI 与运行时兼容。Qt / VTK 需实际窗口验证；OCCT / Netgen 需实际几何到网格验证。网格修复的新能力按真实实现与案例验收，不由依赖存在推断已经具备。

优先稳定 release，不以 nightly/latest 作为锁定标识。升级需说明触发原因、接口变化、回退路径和最小验证集。第三方许可说明来自其发布包，不将本仓库 Apache-2.0 套用到依赖。

## 事实、约定与验证的边界

C++20、模块隔离、Cargo 主入口是项目选择；各工具实际支持的 API 由锁定版本决定，组合可用性由本仓库实际验证决定。历史任务完成状态不能替代当前范围的验证。

固定清单维护在依赖获取文档；实施与验证记录写入当前任务。各技术依据见 [规范索引](README.md)，开始工作见 [任务索引](../task-index.md)。
