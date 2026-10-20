# 目录与模块规划

[架构总览](README.md)

## 当前与目标

当前仓库包含 Rust workspace、Cargo 调度的 CMake native 图、CXX 边界、Qt/QML 窗口、VTK WebGPU 视口、工程 / STL 服务。project 是模型资产与场景载体；OCCT/Netgen 继续支持 STEP、体网格和修复验证。图中标为规划的目录仅作职责参考，不创建无用途占位模块。

```text
panta/
├── AGENTS.md                  # 任务路由与基本约束
├── README.md                  # 使用入口与当前状态
├── ai-docs/
│   ├── architecture/          # README.md 总览与分主题架构
│   ├── modules/               # 重要模块设计；实施进展见 task
│   ├── standards/             # 技术规范及官方依据
│   ├── task-index.md          # 任务队列与状态
│   └── task/                 # 模板与 NNN-name.md
├── Cargo.toml / Cargo.lock    # Rust workspace 与依赖锁
├── rust-toolchain.toml        # 固定 stable 工具链
├── crates/
│   ├── launcher/              # 统一运行入口与 native 构建调度
│   ├── panta-foundation/      # 进程级设施；当前为崩溃信号与日志
│   ├── panta-build/           # 托管构建工具与安装支持
│   ├── panta-core/            # 工程、运行时、任务、Mesh 驻留与持久化
│   ├── panta-ffi/             # CXX DTO/句柄/错误边界与 staticlib
│   ├── panta-cli/             # 工程创建/检查与 STL 检查/导入 CLI
│   ├── panta-geom/            # 规划：几何身份、修订与后端契约
│   ├── panta-import/          # 当前：格式识别、来源快照与 STL 导入准备
│   ├── panta-mesh/            # 当前：STL 网格数据、Mesh IR 与领域校验
│   ├── panta-bc/              # 规划：边界条件与目标引用
│   ├── panta-material/        # 规划：材料数据、单位与版本
│   ├── panta-visualization/   # 规划：显示配置、字段与选择语义
│   ├── project/               # 规划：工程模型与命令
│   ├── workflow/              # 规划：导入、生成与修复任务编排
│   └── storage/               # 规划：持久化与数据资产索引
├── native/
│   ├── CMakeLists.txt         # native 顶层构建（native / Cargo / Qt 演进：Qt 供给、defaults 函数）
│   ├── CMakePresets.json      # 单配置 Ninja presets：debug / release
│   ├── cmake/                 # 安装包配置模板与 Qt 供给脚本（qt-provision.cmake，Qt 入口）
│   ├── foundation/            # 基础契约与构建链验证 target
│   ├── qt-adapter/            # Qt 平台服务与系统目录/偏好适配
│   ├── bridge/                # ProjectViewModel、设置与路径 Qt 适配
│   ├── app/                   # Qt 桌面入口 panta（Qt 入口 Qt 实现）
│   ├── geometry/{include,src/occt}/       # 当前：自有摘要契约与 OCCT adapter
│   ├── mesh/{include,src/netgen}/         # 当前：native 网格 DTO、CXX 校验适配与 Netgen adapter
│   └── visualization/{include,src/vtk}/   # 当前：显示契约与 VTK 后端（含 navigation/）
├── qml/
│   ├── App.qml                # 最小工具栏、资产列表与视口工作台
│   ├── Components/
│   ├── Panels/                # ViewportPane：文档页签与 VTK 装配
│   ├── Dialogs/               # NewProjectDialog、ImportDialog
│   └── Themes/                # Theme 单例
├── schemas/                   # 规划：本地工程与结果导入格式契约
├── resources/                 # 实际资源与样例
└── tests/                     # Cargo 编排及测试；详见 tests/README.md
    ├── rust/、cpp/、qml/       # 按语言 / 模块组织功能回归
    ├── integration/           # Cargo native/QML 聚合入口
    ├── support/               # 跨功能测试共享的 Rust、Qt、QML 辅助代码
    └── fixtures/              # 固定输入与来源 / 单位 / 预期
```

## 模块归属

当前 `panta-core` 已包含 project/path/task 与工程存储服务；导入格式入口在 `panta-import`，STL 网格解析与领域校验在 `panta-mesh`。`panta-foundation` 拥有进程设施，`panta-ffi` 负责跨语言契约和服务转发。规划的五个领域 crate 按实际功能建立，其职责、依赖方向和迁移门槛以 [重库适配与 Rust 领域模块](native-domain-boundaries.md) 为准；领域 crate 不反向依赖承载应用服务的 core，避免循环。

`project`、`workflow` 与 `storage` 是后续职责拆分方向，不是已存在的独立 crate。当前不再规划另一份含义重叠的 `crates/core/`；共享基础类型有真实消费者后才确定归属。C++ `include/panta/<module>/` 暴露自有 native 契约，`src/occt`、`src/netgen` 封装重库；`src/vtk` 同时承载显示后端，导航内部归 `navigation/`。业务状态逐步归 Rust；保留本地相机、命中和窗口生命周期。ViewModel 只适配 UI，QML 处理布局、展示与绑定。

`schemas/` 若随实际需求建立，负责本地工程与结果导入格式契约；应明确格式来源、版本和校验规则，并与现有 Rust 持久化定义保持单一权威。跨语言 FFI 优先验证 CXX；`crates/panta-ffi` 生成桥接头和 Rust staticlib，native CMake 通过 launcher 传入它们，不在 CMake 中回调 Cargo。QML 模块可在 qml/ 下设置自己的 CMakeLists.txt，由 native 顶层纳入构建。


## 当前依赖分组

依赖方向以 `Cargo.toml` workspace metadata、各 native target 的直接链接关系和 QML module 为准；当前分组及 adapter 边界见[分层标准](../standards/layering.md)。

| 分组 | 当前成员与职责 |
|---|---|
| Rust 基础/构建 | `panta-foundation` 进程设施；`panta-build` 工具供给与构建调度 |
| Rust 领域 | `panta-mesh` 网格 IR；`panta-import` 格式导入编排；`panta-core` 工程与服务。现有依赖是 `core → import/mesh`、`import → mesh` |
| Rust 工具入口 | `panta-launcher` 组合 Rust 服务与 native 产品；`panta-tests` 承载质量/测试调度 |
| FFI | `panta-ffi` 通过 CXX 暴露并转发 Rust 服务，不承载领域算法 |
| Native adapter | `foundation`、`geometry`、`mesh`、`qt-adapter`、`visualization` 按职责封装 OS、OCCT、Netgen、Qt、VTK；当前 `mesh → geometry` 直接链接并通过私有契约借用运行期 shape；其余 adapter 之间没有自有库直接依赖 |
| Bridge 与 UI | `native/bridge` 适配 Rust FFI、Qt 与 native 显示/平台服务；`qml/` 构成 `Panta.Shell`，通过 Bridge 暴露对象交互 |
| 组合根与测试 | `native/app` 组装 native 库、插件与 QML；`tests/` 消费被测模块，不进入生产依赖图 |

规划中的独立 `panta-geom`、材料、工作流等领域 crate 尚未建立。Rust 应用命令层的当前状态和 GUI/CLI 共享边界见[命令层设计](command-layer.md)。

## 新增文件原则

按职责放置文件，避免按语言把所有业务堆进一个桥接层。访问私有实现的单元测试贴近源码；公开 API 的 Rust 黑盒测试放根 `tests/rust/` 并由所属 crate 注册，native/QML 测试和回归夹具按[测试规范](../standards/testing.md)归档。测试资产应小且能合法分发，大型 CAD/结果数据通过受控的外部资产机制管理。

工程名与 Rust package 名已统一：工程为 `panta`，Rust package 用 `panta-` 前缀，当前 launcher 的 bin 名为 `panta-launcher`。桌面可执行文件已统一命名 `panta`；不直接沿用附件中的 `moldcae-desktop` 等示意名称。
