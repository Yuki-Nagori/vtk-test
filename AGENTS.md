# 仓库协作指南

本仓库是最小可视化渲染测试仓库，应用与 crate 暂沿用 `panta` 名称；保留模型导入、几何 / 网格处理与 VTK 渲染，project 用于组织测试资产与场景。当前能力见 [README](README.md)，实施进度见[任务索引](ai-docs/task-index.md)；规划路径和接口不代表已经实现。

## 工作入口

| 工作内容 | 入口 |
|---|---|
| 开始任务、查依赖与状态 | [任务索引](ai-docs/task-index.md) / [任务模板](ai-docs/task/_template.md) |
| 查中文界面、路径、后台任务与模块设计 | [重要模块说明](ai-docs/modules/README.md) |
| 查模块职责、数据流与里程碑 | [架构总览及主题导航](ai-docs/architecture/README.md) |
| 查版本基线、编码及依赖规则 | [技术规范索引](ai-docs/standards/README.md) |

只阅读当前任务相关的架构与规范。任务范围变化时先更新所属任务；长期设计决策同步到对应说明。

## 任务与代码约束

- 实施前创建或更新 `ai-docs/task/NNN-name.md` 并登记 `ai-docs/task-index.md`。任务元数据使用无序列表，每个任务保持一个文件。CI 修复等小范围工作更新所属任务的范围与验证记录，不另建任务。验收后记录实际验证结果并同步任务状态和索引。
- 代码注释遵循[注释规范](ai-docs/standards/comments.md)，新增文件遵循[仓库文件规范](ai-docs/standards/repository-hygiene.md)，验证要求见[验证与评审](ai-docs/standards/validation-and-review.md)。
- 不保留死代码或废弃实现；替换时同步清理，默认不写前向或后向兼容代码。确需例外时用 `COMPAT(...)` 标记并登记清理任务，见[代码生命周期](ai-docs/standards/code-lifecycle.md)。
- 每次提交需让功能、测试、配置、文档和任务记录保持一致，并遵循[提交规范](ai-docs/standards/commits.md)。不把已知损坏状态留给后续提交修复。

## 架构边界

- Rust 承担领域数据、业务规则与编排。CXX 只声明映射和签名；C++ 适配层调用重型库并封装类型、异常与所有权。
- VTK 窗口、输入和逐帧显示由 C++ 管理。QML 经 C++ ViewModel 调用服务，不直接操作 OCCT、Netgen 或 VTK。自有 C++ 使用 C++20。详细边界见[分层规则](ai-docs/standards/layering.md)和[职责审计](ai-docs/architecture/native-domain-boundaries.md)。
- Cargo 提供统一入口，CMake 拥有 native 构建图；Rust/C++ 桥接优先验证 CXX。
- 未落地的接口、目录和命令要标为规划，只运行仓库中实际存在的检查。

## 构建、测试与窗口验收

- 本地日常开发按改动选择 `cargo test --locked -p <package>`；涉及跨语言、共享构建或影响无法安全缩小时，运行 `cargo test --locked --workspace`，聚合完整 Rust/native/QML 行为测试。修改后和评审修复后先重跑直接受影响的测试，再完成适用验收。
- 常规构建和质量检查使用 `cargo build --locked`、`cargo format --check`、`cargo lint <tool> --check`；`cargo quality` 提供手动完整质量入口。格式入口仅覆盖 Rust、C++ 和 QML；CMake 语法及行为由实际 configure/build/CTest 验证。PR 与 main 推送的 CI 只在 Linux 构建并运行完整行为测试，macOS 与 Windows 完整行为测试由 nightly 定时或手动触发运行；单一 Ubuntu quality job 检查格式、Clippy 和 qmllint。
- 新增或迁移 Cargo package、CMake target、QML 模块或测试支持目录时，同一改动更新实际 Cargo/CMake 注册、依赖与格式/lint 输入，并验证新测试确实被发现和执行；必要时同步任务与架构文档。测试范围依据真实消费关系显式选择，不维护自动变更范围选择器。
- 当前不保留性能、覆盖率、Miri 或 sanitizer 的专属入口和门禁。确有诊断需求时另建任务，明确问题、工具、输入及验收后再接入。
- 需要真实窗口验收时先运行 `cargo build --locked`。macOS 绑定 `target/native/debug/app/panta.app`，或启动其 `Contents/MacOS/panta`；Windows/Linux 启动 `target/native/debug/app/panta.exe` 或 `target/native/debug/app/panta`。macOS 用 `cua.getApp` 绑定 `.app` 并调用 `getAXStateAndScreenshot()` 检查真实窗口。每次 UI 操作后刷新 `getAXState()`，按最新状态定位控件。
- 验收结束时关闭应用。不要为验收先连接 VS Code，也不要把无头 QML/CTest 或仅启动进程当作真实 GPU 窗口通过。Linux / Windows 使用窗口清单中的实际窗口 ID 绑定。图形会话不可用时记录具体限制；用户人工验收可作为独立证据。

## Subagent 使用规则

### 何时调用

- 每个非平凡任务在交付前都要安排一次独立评审。评审者根据需求、风险和完整 diff 检查，不复述实现者自检。简单的拼写、格式或单行修复不因此强制启动评审，用户明确要求时除外。
- 关键行为或失败路径存在测试缺口时，按[测试规范](ai-docs/standards/testing.md)拆分给 subagent 调查或修复；主代理负责整合并运行适用验证，不添加只复述实现的断言。
- 复杂问题能拆成相互独立的调查（例如跨平台 CI 根因、模块边界或文档一致性）时，可以并行委派。先确认每个子任务有独立产出，避免多个代理同时修改同一处。
- 常规小改动、主线工作可直接完成且没有独立审查要求时，不要为形式而启动 subagent。

### 默认模型

- 代码与架构的独立评审默认使用 `gpt-6.1-sol`，reasoning effort 为 `medium`。用户对某次工作明确指定其他模型或强度时，以该指示为准。
- 调查与实现子任务沿用当前代理可用的默认模型；只有用户或任务要求指定时才覆盖。工具支持时，委派前明确传入模型和 reasoning effort。

### 委派与评审边界

- 委派说明列明目标、范围、相关路径、验收条件、需要运行的检查，以及只读评审或获准修改。独立评审默认只读，不让评审者顺手重写被评代码。
- 评审报告应提供可操作的问题、文件与行号、触发条件和影响。没有发现问题时也要说明检查范围与限制。主代理判断发现是否成立，负责修复、验证和更新文档，并将评审结果记录在所属任务中；不把评审结论直接当作通过证明。
- 等待评审时，主代理继续处理不依赖评审结论的工作。修复评审发现后，重跑受影响测试及适用的交付门禁；只有影响无法安全缩小或明确要求完整验收时才运行全量测试，必要时请评审者复核。
- Subagent 不自行提交、推送、创建或合并 PR，也不代表用户对外沟通，除非用户明确授权。

## 分支、GitHub 与提交

- GitHub issue、PR、Actions 与仓库状态的查询或操作统一使用已认证的 `gh` CLI（如 `gh issue`、`gh pr`、`gh run`、必要时 `gh api`），不要用网页手工操作或自写 HTTP 请求。写操作前核对仓库、目标编号、当前分支和对话中的授权范围。
- 涉及代码、测试、依赖、构建或 CI 配置的工作，以及同时包含实现与配套文档的工作，都在专用工作分支完成。Task 实现与 issue 修复默认创建或继续对应分支，验证后通过 PR 合并到 `main`。
- 只有纯文档 issue 修复，且用户明确要求直接提交到 `main`（或本会话已有同等明确授权）时，才可在 `main` 上修改并提交。提交前确认工作区没有混入其他改动。纯文档例外不适用于实现任务中的文档同步。
- 不确定是否属于纯文档时按需要分支处理；不要为了省略 PR，把测试、脚本、配置或生成逻辑归为文档改动。
- 提交、推送、合并 PR 或改写共享分支历史，均须获得当前会话的明确授权；完成评审本身不构成授权。获准推送实现前，先完成独立评审。
- CI 修复等小改动复用所属任务。本地验证和评审后，按当前授权提交或推送，并等待对应 CI；CI 失败时继续定位、修复和复验，全部通过后结束。
