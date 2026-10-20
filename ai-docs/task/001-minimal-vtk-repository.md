# 001 — 最小可视化渲染测试仓库整理

- 状态：implemented
- 阶段：仓库重新起步
- 依赖：无
- 优先级：P0
- 负责人：Codex
- 创建 / 更新：2026-10-20 / 2026-10-20

## 目标与范围

本仓库从 Panta 导入的代码重新起步，以模型导入、网格处理和 VTK 渲染为主链，project 用于组织测试资产与场景。
用户明确授权清空原有任务记录，旧 task 不作为本仓库实施队列。
保留 project 存储、资源缓存、Rust/CXX 桥接、Qt 宿主与 VTK 视口及必要的构建测试基础。
保留 STEP / OCCT / Netgen 适配、体网格数据契约及其回归测试，供后续 STEP 导入、体网格生成与网格修复测试使用。删除 VTK 和 OCCT/Netgen SDK 生产 workflow，应用继续消费已发布、锁定摘要的 Release。保留原 ai-docs 结构，小幅调整仓库定位；涉及旧 task 链接的段落整体改写。删除独立模拟任务演示及配套 CXX/Qt 测试接线。
本轮保留几何与网格实验基础；project 当前数据契约保持可用，QML 按更新范围简化，不在清理时迁移工程 schema。

## 实施与验收

- [x] 旧 task 清空但保留模板与使用流程；README / 架构入口反映最小可视化渲染测试定位。
- [x] 保留 STEP / OCCT / Netgen 主链；删除两条 SDK 生产 workflow，现有 Release 消费声明不变。
- [x] 保留 project / STL / VTK 主链及有意义的回归测试。
- [x] 格式与 lint 范围随删除同步，测试使用明确 package 或完整 workspace 入口。
- [x] 执行适用检查与独立评审，记录实际结果和环境限制。

## 清理与兼容例外

删除旧任务记录及模拟任务演示，保留原有技术文档并调整旧任务引用，Git 索引中已有的用户导入内容不重置、不提交。
无新增兼容实现；现存 Netgen 销毁处理重新登记为 `COMPAT(task 001; remove-task 002)`，保留与退出条件见 [002](002-netgen-cleanup-compatibility.md)。工作分支：`yuki/minimal-vtk-cleanup`。

## 验证记录

工作目录 `/Users/yuki/eit/vtk-test`，macOS arm64，Rust 1.99.0。

- `cargo build --locked`：通过。SDK 使用原锁定 Release，本地复用相同版本缓存；未运行 SDK 生产流程。
- `cargo verify-changed`：格式、Clippy、依赖、分层和 QML 阶段通过，C++ 阶段发现并修复夹具构造异常；后续按失败范围拆分复验，未将原聚合命令记为通过。
- `cargo test-scope all`：Rust 各包通过；native/QML 共 68 项中 67 项通过，剩余 SDK 文档诊断断言同步后单独复验通过。手动性能用例按声明跳过。
- `cargo test --locked -p panta-launcher --bin panta-launcher`：4 项通过；`cargo test --locked -p panta-tests --bin panta-tests`：31 项通过、3 项手动性能入口测试跳过。
- `cargo lint clippy --check`、`cargo format --check`：修复后通过。42 个自有翻译单元的 clang-tidy 扫描仅有 GPU 夹具问题，修复后该单元复验通过；include-cleaner 的 8 个失败单元及 GPU 夹具共 9 个单元修复后通过；其余单元原扫描通过。`cargo lint cppcheck --check`：通过。
- Markdown 本地链接检查与 `git diff --check`：通过；旧任务仅保留重新建立的 001/002 和模板，OCCT/Netgen 适配及现有 Release 消费保留。
- 独立评审及修复复核已完成，取消后立即重开的竞态已修复；另复核 GPU 夹具、SDK 诊断、精确生成文件误报排除和结果数据文档，未发现未解决代码问题。
- macOS 真实窗口：已检查空工作区、创建临时工程、STL 资产出现、保存重开后的按需加载、四面体 VTK 显示、关闭最后页签清空模型及重新打开同一资产；结束时退出应用。临时输入位于 `/private/tmp/vtk-test-gui-20261020/tetra.stl`，工程为该目录下 `STL渲染验收/STL渲染验收.panta`。ScreenCaptureKit 间歇报错后重新绑定窗口继续验收，最终获取包含真实模型、方向控件和文档页签的截图。
- 窗口布局修复：侧栏固定 240，视口申请剩余宽度；新增 1280×800 / 1600×1000 几何回归。`Qml.ShellModuleLoads`、该测试 clang-tidy、重建后的 `cargo lint qmllint --check` 与格式检查通过；独立评审复核无新增问题。
- 待验收：Windows / Linux 真实窗口与原生文件选择器需对应平台实机执行，因此标 implemented。Miri、sanitizer、覆盖率和性能基准本轮未运行，不计为本轮已通过项。

## 工作记录

- 2026-10-20：创建任务；用户要求本仓库重新开始，旧任务不迁移、不保留备份目录。

- 2026-10-20：用户澄清需要 STEP 导入、体网格生成和网格修复，已撤回独立重库链路删除；配套源码与构建测试逐一恢复到原索引版本。用户明确要求删除 `sdk-vtk.yml` 与 `sdk-occt-netgen.yml`，继续使用已有 Release。任务模板及索引使用/状态说明保留。

- 2026-10-20：用户最终明确定位为最小可视化渲染测试仓库，project 为载体；原 ai-docs 保留并微调，包含旧任务链接的段落整体优化，不大规模删除技术文档。

## QML 范围更新

用户授权大幅简化 QML。收敛为测试工作台：工程创建/打开/保存、STL 导入、模型列表、文档页签和 VTK 视口。
删除 CAE 材料/工艺/分析结果、Shared Views、商业/帮助占位和多层 ribbon 等界面及专属资源；保留真实工程后端数据契约。
同步 QML 注册、调用点、组件回归和真实窗口验证。STEP/体网格/修复底层继续保留，不在本轮添加未实现 UI。

- 清理未被新工作台使用的 ShellViewModel 修订计数演示；Settings 仅保留减少动画设置及其回归。

## Welcome 清理范围

用户明确删除默认 Welcome 场景。移除欢迎字样与专用 VTK 场景/测试；工作区默认无文档，未加载模型时留白。同步页签、重置接口、缓存/激活回归，实际网格导入与相机测试改用真实小型网格。

- 根目录 `layering-lint-fixtures/` 是导入的 CMake 生成缓存，加入目录级 ignore；真正分层测试源码 `native/cmake/tests/layering/` 保留，当前检查产物在 `target/panta-layering-fixtures/`。

## 中文单语言范围更新

用户明确去掉 i18n，本仓库直接使用中文。删除语言选择/翻译字典、QM编译与资源部署、Qt翻译适配和相应测试；QML/原生交互与工程提示直接使用中文，诊断代码保留。
同步 Cargo/CMake 构建、CLI/FFI/Settings消费、测试选择器和文档。DSL/FSM/工程变量随后按用户最新授权整体移除，见下文。

## 工具链升级

用户要求首选 Rust 升至 1.99，`rust-toolchain.toml` 固定 `1.99.0`。历史 1.98.1 结果仅作为中间证据，最终验证使用 1.99.0；迁移 Rust 1.99 原子计数 API 时将 workspace MSRV 同步到 1.99，避免保留弃用警告。

## 主题简化范围

用户授权简化主题。保留现有界面实际引用的固定 QML 视觉常量，删除无运行时消费者的主题 DSL、切换与扩展方案；不添加替代主题配置系统。

## 剩余演示接口清理

用户要求继续清理。删除仅为早期双向 CXX 冒烟存在的文本重复/前缀、Session 计数句柄和 panic_probe 演示 API，连同手写 ffi_support.cc/ffi.hpp。
保留真实的工程/路径/网格 CXX 服务、退出排空、崩溃设施和网格 DTO native 回归；同步桥接构建输入与质量扫描，不用无消费者演示维持覆盖率。

## 未使用展示接口与测试收敛

用户要求进一步简化测试和未使用功能。删除已无QML消费者的材料/Fill/Gate/分析序列ProjectViewModel属性、目录投影、草稿校验、后台确认与对应CXX入口/DTO；保留ImportDialog实际使用的mesh类型目录。
删除仅验证这些展示接口的测试；复合测试保留工程/资产/导入失败/修订和取消不变量，仅移除CAE投影断言。不迁移现有Rust工程清单schema，领域持久化回归保留。

## DSL 与状态机整体移除

用户明确授权整体移除 DSL 模块。删除 panta-dsl-core/panta-dslc、.pa编译生成/变量文档接口与生成状态机，Qt状态机文档收敛为当前交互边界。
STL资产激活改为直接后台读取/校验/结果交付，保留取消、工程代次/attempt过期、容量限制、失败不替换有效快照和退出生命周期；不把通用状态机简化为缺少错误边界的同步读取。
同步 Cargo/CMake/quality/selector/CLI/FFI、测试与文档；删除仅服务DSL或变量配置的测试和性能入口，工程主清单与真实网格链继续保留。

- DSL 删除同步修正测试选择器：无法识别的 Rust/Cargo package 路径同时扩大测试与质量检查，防止已删除模块或未知板块静默跳过；新增回归通过。Miri 仅运行不触发 zstd FFI 的用例。
- 独立评审发现取消后的在飞激活请求被重新打开的页签复用，已排除已取消请求的去重，并以单 worker 阻塞队列确定性验证关闭后立即重开：旧结果过期，新请求成功且网格驻留。109 项核心单测、8 项激活集成、39 项工程集成和核心 Clippy 通过。评审者只读复核修复及回归，原问题闭环，无新增发现；其余代次隔离、队列容量与跨工程显示隔离未发现新增问题。

## 已求解结果可视化范围澄清

用户明确需要导入已经求解完成的数据并渲染。保留网格、区域材料 / 边界标签和结果场组合设计，明确网格版本、节点 / 单元 / 区域关联、单位、时间步与来源；这属于后续结果导入 / 可视化方向，不是外部求解器执行接入，也不恢复 DSL 或 Study 配置编辑框架。
本轮只同步文档范围；当前已实现能力仍为工程 / STL / VTK 主链，不宣称结果导入、标签展示或时间步功能已经完成。

- 最终静态检查修复：GPU 夹具的网格分配移至可报告异常的初始化阶段；清理直接 include，VTK 转换显式保留完整数据类型；Cppcheck 对固定 CXX 生成迭代器的误报精确登记排除，未关闭自有源码检查。SDK 缺资产诊断测试同步到现存文档入口。
- macOS 真实窗口发现 STL nameFilters 使用全角括号导致选择器拒绝 `.stl`，已修为 Qt 过滤器语法的半角括号，重建后复验。

- 收尾状态：代码与当前平台验收完成；未提交、推送或创建 PR。任务 002 的 Netgen 兼容清理仍为 deferred，不影响保留现有适配。

## 剩余文件整体独立评审

2026-10-20：用户要求对剩余 314 个变更文件逐项 subagent review。只读审核全部受版本控制的文件（含原有 LICENSE），按运行时代码、构建/测试/CI、文档三组逐项记录用途、实际消费者、保留理由和删除候选；主代理交叉核验，不将“能够编译”视为每个文件都必要。保留已明确需要的 STEP/Netgen、模型/材料/边界及已求解结果设计、任务模板与文档体系。评审已完成：3 位 gpt-6.1-sol / medium 只读评审者覆盖运行时代码 142、构建测试设施 115、文档 58，共 315 个跟踪文件（314 个变更及原有 LICENSE）；主代理验证清单无遗漏、无重复，并交叉核验主要消费者。314 个变更中，235 个支撑当前功能/验证，33 个保留契约/参考，45 个有效但可选，1 个可移除候选。

可移除候选为 `crates/panta-ffi/src/path.rs`，必须同步清理未被生产使用的 PathHost 实例路径服务、CXX 接线和专属测试；Rust 内部资产路径校验保留。其他文件内收缩项为旧分析/Fill/Gate 编辑 API 与对应测试、低层 CXX 重复导出、按钮三个无调用扩展属性。保留 schema 4 数据校验与网格/材料/边界/结果关联设计，不直接删除持久化字段。

文档发现 8 处局部过时说明（外部 solver-client、安装目录、Study/Job 职责、Welcome、mesh→geometry 依赖、窗口 Chrome 例外、Windows sanitizer 范围、PR 测试范围），保留文件并待局部修正。SDK 来源构建脚本存在手工维护入口；性能/覆盖率/Miri/sanitizer/nightly 等有真实用途，列为可选，不当作孤儿删除。

完整逐文件评审输出到本地 `/private/tmp/vtk-test-file-review.md` 和同名 JSON，记录每文件用途、证据及 13 条具体发现（旧工艺 API 与专属测试为同组收缩）。本轮只读评审实现，未重跑构建/测试、未删除代码、未提交或推送。

## 整体评审后的清理实施

2026-10-20：用户授权开始落实评审结论。移除无消费者的路径 CXX/PathHost 实例服务、旧工艺编辑与 PlanSettings 展示 API、冗余 CXX 导出及按钮扩展属性；同步专属测试、构建和选择器，修正文档 8 处不一致。保留 schema 4 读取/校验、材料/边界/结果关联、工程/STL/VTK、OCCT/Netgen，以及已有实际入口的可选工具。本轮不迁移工程 schema，不删除保留的质量和性能设施。完成后重新独立评审并执行受影响验证。


### 评审结论落实记录

- 删除旧路径 CXX 模块、PathHost 两个文件与专属测试，新增 `utf8_boundary` 窄 helper 及 Unicode/孤立代理项回归；CMake 和测试范围同步。
- 删除 PlanSettings、旧工艺编辑/确认与对应后台 metadata 管线；清单写入委托并入 repository，移除仅转发的 storage 模块。保留 schema 4 所有字段/验证，新增打开→重命名→后台保存的既存字段保持回归，通用修订耗尽、失败、并发和退出写回改用保留操作验证。
- 收缩 CXX 重复导出及无消费者激活 DTO；workspace drain 仍执行核心状态/诊断发布。FFI 新增成功显示及加载失败诊断回归。
- 材料模块只保留内置资源 schema/ID 及工程引用校验，移除旧展示摘要；材料 JSON 原始资源保留。移除按钮的 showCaret/dimText/contentAlignLeft 属性和对应无用分支。
- 文档 8 处不一致已局部修正，并同步实际路径边界与材料资源用途。315 文件审阅是本轮清理前的快照；当前净减少 3 个文件，跟踪文件 312 个（含 LICENSE）。
- 独立只读评审已完成：覆盖 UTF-8、FFI/工作区诊断、schema 4 完整写回、取消提交闸门、租约原子替换、材料 ID 和替代测试，未发现可操作问题。未将评审代替运行验证。
- 针对性结果：core 104 单测、8 激活测试、35 工程测试及全目标 Clippy 通过；FFI 13 测试与 Clippy 通过；runner 31 通过、3 手动性能入口测试跳过；Markdown 本地链接检查通过。最终验证完成：
  - `cargo verify-changed` 的格式、workspace Clippy、依赖、CMake/分层、构建、QML lint、42 个自有单元 clang-tidy 通过；include-cleaner 仅报新 helper 缺少直接 `<string>`，补齐后该单元针对性复验通过，其余单元原扫描通过。原聚合命令因该中途失败不记为整体通过。
  - `cargo lint cppcheck --check`、`cargo format --check`、`cargo lint qmllint --check`、`cargo build --locked` 均通过。
  - `cargo test-scope all` 最终退出 0：全部 Rust 包及 60 项 native/QML 通过，手动性能用例按声明跳过；包含 9 项 STEP 与 14 项网格适配回归。因整个导入相对 main 的公共构建边界，选择器保守扩大为完整范围。
  - macOS 真实窗口重开已有临时工程，点击 tetra 完成按需加载，截图确认真实模型、方向控件及页签；关闭最后页签后截图确认模型清空，再次打开正常。结束时退出应用，返回 procNotFound 确认进程已结束。
  - Markdown 本地链接与最终空白检查通过。Windows/Linux 实机仍待对应环境验收，保持 implemented；Miri、sanitizer、覆盖率与性能基准未在本轮重跑。
  - 所有实现工作在 `yuki/minimal-vtk-cleanup`；未提交、推送或创建 PR。暂存状态由外部操作发生变化，代理未操作索引。


## 可选测试与 CI 设施收敛

2026-10-20：用户授权继续审查此前 45 个可选文件，目标只保留必要构建、导入和渲染验证。删除独立性能、coverage、Miri、sanitizer 及其专属脚本/抑制/依赖，删除 nightly、打包和 SDK 生产 workflow 与无运行消费者的生产脚本；已发布 Release 消费继续保留。CI 收敛为三平台构建/完整行为测试与单一质量 job，取消 L1/L2 变更选择分发及对应复杂本地选择器，日常改动用明确 package/test 选择运行。保留关键工程持久化/导入/失败取消/网格/窗口生命周期回归、CLI 导入检查入口、实际可访问性和轻量编辑器配置。原技术文档保留并与当前入口同步，不将已删除诊断设施保留为活跃规划。完成独立评审与实际验证后更新状态。


### 可选设施收敛结果

- 原 45 文件快照中现有 28 个已删除、17 个保留（快照包含前轮已删除的 PlanSettings）。本轮从 312 个文件净减 37 个至 275 个，额外删除 SDK 生产脚本、变更分类与工具缓存 action 等支撑设施。
- 保留的可选项有明确消费者：CLI 工程/导入入口、CMake presets、编辑器配置、减少动画的系统适配、无 Bridge 资源构建诊断、三份 SDK 来源记录；增量更新文档压缩为非当前范围说明。未恢复任何性能/覆盖率/Miri/sanitizer活跃设施。
- CI 仅 `ci.yml`：三平台矩阵统一 `cargo test --locked --workspace`，另一个 Ubuntu job 运行 format、Clippy、qmllint。删除重复 check/build/分层测试分发，保留下载缓存与 Linux 图形前置。
- 删除 Criterion/Hyperfine 及专属依赖，离线更新 Cargo.lock；删除失效 syn 多版本豁免，保留 LLVM 编译/格式/静态检查及必要 AR 工具；工具链自检同步实际裁剪结果。
- 独立只读评审覆盖冷启动 Cargo→CMake、三平台运行环境、SDK 消费、本地打包、质量命令与文档，未发现行为回归。评审发现的 Miri cfg/忽略残留已移除，并经复核确认；正常平台输入与等待行为保留。
- 实际验证：`cargo test --locked --workspace` 退出 0，完整 Rust 与 60 项 native/QML 通过；后续 Miri 配置残留清理后重跑 core/foundation 通过；runner 9 项、panta-build 20 项针对性测试及 Clippy 通过。
- 与 CI 对应的 `cargo format --check`、`cargo lint clippy --check`、`cargo lint qmllint --check` 通过；额外 CMake lint、Rust/CMake 分层及 `cargo run --locked -p panta-tests -- toolchain` 通过。剩余 workflow YAML 与本地 action 引用已静态核验，GitHub Actions 未远程运行。
- 当前 UI / 视口运行代码未变化，本轮未重复真实窗口操作；上一轮 macOS 真实渲染记录作为独立历史证据。Windows/Linux 实机仍待对应环境验收，任务保持 implemented。没有提交、推送或创建 PR。


## Python 开发工具链移除

2026-10-20：用户要求移除不再需要的 cppcheck / Python 设施。实际消费者仍包括手动 cppcheck 与 cmakelang 格式/lint；本轮一并删除二者入口、配置、Python/uv 供给和锁文件，以及专用数据库生成。保留 Rust/C++/QML 格式、Clippy、clang-tidy/includes、分层和 QML 检查，CMake 语法/target 接线继续由真实 configure/build/CTest 验证，不另引入替代语言工具链。同步文档与忽略规则；第三方 SDK 中 Python 关闭选项和来源说明不作为本仓库 Python 实现删除。完成独立评审及适用测试后同步状态。


### Python 工具链清理结果

- 删除 pyproject.toml、uv.lock、.cmake-format.py、cppcheck 的模型/抑制配置、panta-build Python 模块和 Python 专用规范，共净减 7 个文件；当前 268 个跟踪文件。
- 删除 uv 版本/三平台资产/安装解包分支；删除 cppcheck 解析所需的 include 过滤、直接 shlex 依赖和专用数据库。cc 仍需要的间接 shlex 由 Cargo.lock 正常保留。
- 格式入口仅 Rust/C++/QML，lint 7 阶段不再含 cmakelang/cppcheck；CMake 使用真实配置、编译、CTest 与分层图验证。没有新建替代语言工具链。
- 文档、导航、忽略规则同步；保留第三方 SDK 真实 Python 关闭选项/上游教程来源说明。源码内无 Python/uv 调用，cppcheck 名称仅保留在已移除入口应返回错误的回归中。
- 独立只读评审确认冷启动格式入口、LLVM 编译数据库、实际 CMake 验证与本地打包链路完整，无可操作问题。
- 实际验证：panta-build 19 项、runner 9 项针对性测试/Clippy 通过；离线 metadata 更新成功；`cargo test --locked --workspace` 退出 0，完整 Rust 和 60 项 native/QML 全部通过；格式、workspace Clippy、Markdown 链接和空白检查通过。`cargo lint qmllint --check` 通过。UI 运行代码未变，本轮未重复真实窗口或跨平台实机验收。
- 未提交、推送或创建 PR。


## 静态分析与分层门禁移除

2026-10-20：用户要求测试仓库只保留必要构建、导入和渲染验证。移除 clang-tidy、include-cleaner、Rust/CMake 自动分层规则及专属配置/fixture/编译数据库合并；保留 CMake 原生 compile_commands 供轻量 IDE 使用。进一步收敛手动质量设施，去掉无必要的 Cargo 扩展安装/审计和独立工具链自检，只保留格式、Rust Clippy、QML lint 与实际 workspace/native 行为测试。LLVM 编译器、clang-format、AR 和原生运行库供给继续服务真实构建；模块职责文档作为人读边界保留，不再维护自动允许边清单。同步源码注释、编辑器设置和文档，执行独立评审与实际构建测试。

### 静态分析清理结果

- 净减 10 个文件，当前 258 个文件。删除分析配置、分层实现与夹具、合并编译数据库模块和独立工具链自检；移除 Cargo 扩展安装、审计、豁免和无消费者依赖。保留用户已有 `layering-lint-fixtures/` 缓存的忽略项。
- `cargo lint` 仅 Clippy 与 qmllint；`cargo quality` 聚合格式、两项 lint 和 workspace 行为测试。编译、导入、网格、持久化与渲染生命周期验证保留，不添加仅证明旧命令已删除的机械测试。
- LLVM 裁剪支持 Unix 归档和 Windows 安装布局，保留编译器、clang-format、AR/链接工具、Windows DLL 与编译资源；CXX 实际编译及头文件/staticlib 元数据保留。IDE 使用原生 CMake 编译数据库，不再生成合并数据库。
- 独立只读评审覆盖质量入口、Cargo/CMake、LLVM 裁剪、FFI、CI、编辑器与文档；发现的旧 cargo-machete 豁免已删除，无阻塞缺陷。文档代理核验 53 份文档的 246 个本地链接。
- 实际 `cargo quality` 退出 0：格式、workspace Clippy、qmllint、完整 Rust 和 60 项 native/QML 全部通过，包含 STEP 与 Netgen 体网格回归。panta-build 16 项与 runner 4 项测试通过；原生编译数据库存在。差异空白检查通过，初始导入 STEP 夹具原有空白保持不改。
- 本轮未修改 UI 行为，未重复真实窗口验收；Windows/Linux 实机仍待对应环境，保持 implemented。未提交、推送或创建 PR。

## 提交与合并授权

2026-10-20：用户授权提交整理结果、创建 PR，并不等待 CI 直接合并到 main。本次提交包含清理后导入的完整最小仓库；本地 `cargo quality` 与独立评审结果见上。远程 CI 不作为此次合并的等待条件，跨平台实机验收仍未完成。
