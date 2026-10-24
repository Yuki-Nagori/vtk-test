# 任务索引

本仓库定位为最小可视化渲染测试仓库。

| 编号 | 任务 | 状态 |
|---|---|---|
| 001 | [最小可视化渲染测试仓库整理](task/001-minimal-vtk-repository.md) | implemented |
| 002 | [Netgen 销毁兼容处理退出条件](task/002-netgen-cleanup-compatibility.md) | deferred |
| 003 | [Cargo 元数据 repository 指向本仓库](task/003-cargo-repository-metadata.md) | implemented |
| 004 | [同步文档中的 CI 平台描述](task/004-ci-platform-docs-sync.md) | implemented |
| 005 | [panta-import 改名为 panta-io，并拆分为 import / export 模块](task/005-rename-panta-identifiers.md) | done |
| 006 | [STL 修复前置：表面网格质量诊断](task/006-stl-mesh-diagnostics.md) | done |
| 007 | [STL 修复管线](task/007-stl-repair-pipeline.md) | planned |
| 008 | [STEP 接入工程工作流](task/008-step-project-workflow.md) | draft |
| 009 | [CI 编译产物缓存评估](task/009-ci-build-cache.md) | deferred |
| 010 | [多格式网格导入（VTU / VTP / VTK / OBJ / PLY）](task/010-multi-format-import.md) | done |
| 011 | [Mesh 资产分块存储（sidecar v3）](task/011-mesh-asset-chunked-storage.md) | done |
| 012 | [移除 Mesh sidecar v2 只读兼容](task/012-mesh-asset-v2-removal.md) | planned |
| 013 | [网格导出与视口截图](task/013-mesh-export.md) | done |
| 014 | [工作台 UI/UX 优化（保守版）](task/014-ui-ux.md) | done |
| 015 | [GUI 网格质量诊断面板](task/015-mesh-quality-panel.md) | implemented |
| 016 | [可取消、带百分比的后台任务进度](task/016-background-progress.md) | implemented |
| 017 | [视口工具：预设视角、复位、拾取与测距](task/017-viewport-tools.md) | implemented |
| 018 | [工作台交互：拖放导入、最近工程、菜单栏、关闭确认](task/018-workbench-interactions.md) | implemented |
| 019 | [大网格：流式资产读写、显示 LOD 与规模基准](task/019-large-mesh-lod.md) | implemented |
| 020 | [时序动画：序列加载、预取缓存与播放](task/020-time-series-animation.md) | done |
| 021 | [体网格：导入、显示/切片、导出、质量诊断、与 Netgen 体网格打通](task/021-volume-mesh.md) | done |
| 022 | [体网格质量统计、切面与 VTU 导出后台化](task/022-volume-background-tasks.md) | done |
| 023 | [工作台命令：STEP → Netgen 生成体网格](task/023-step-netgen-volume-command.md) | done |
| 024 | [任意平面切面](task/024-arbitrary-section-plane.md) | done |
| 025 | [LOD 状态下流式导出，体网格流式激活、诊断、切面与导出](task/025-lod-streaming-diagnostics-export.md) | done |
| 026 | [大网格阈值（表面 LOD / 体网格流式）设为用户设置](task/026-lod-user-settings.md) | done |
| 027 | [单机 out-of-core 分块体网格：最小原型与切面延迟实测](task/027-out-of-core-block-prototype.md) | done |
| 028 | [out-of-core 铺开：只读映射块读取、接入应用与大网格设置](task/028-out-of-core-rollout.md) | done |
| 029 | [结果场：导入、存储、分块携带、切面插值与视口上色](task/029-result-fields.md) | done |
| 030 | [原生混合单元：类型 / offset 布局、拆分、资产 v4、导入导出与质量](task/030-mixed-cells.md) | done |
| 031 | [多面体单元（VTK_POLYHEDRON 42）：面流布局、拆分、资产 v5、`.pchunk` v4、导入导出与质量](task/031-polyhedron-cells.md) | done |
| 032 | [OpenFOAM / CGNS 导入与基于面邻接的非正交性 / 偏斜度](task/032-openfoam-cgns-import.md) | in-progress |

任务先登记范围与验收，实际验证后同步状态。历史编号不再代表本仓库的待办或已交付能力。

## 目录与使用方式

```text
ai-docs/
├── architecture/                 # README.md 为总览与主题导航
├── modules/                      # 重要模块设计与任务导航
├── standards/                    # 技术规范与官方依据
├── task-index.md                 # 全部任务索引
└── task/
    ├── _template.md              # 创建任务时复制
    └── 001-minimal-vtk-repository.md # 一个任务一个 Markdown 文件
```

新增任务使用未占用的三位递增编号和 kebab-case 名称；编号稳定，不因执行顺序调整而重排。每个任务直接保存为 `task/NNN-name.md`，无需单独创建目录。设计、决策和验证记录写在任务文件内；必要时链接外部证据，不要提交临时大型日志或构建产物。

1. 从 [模板](task/_template.md) 建立任务，填写目标、边界、依赖、必读规范和可判断的验收条件。
2. 在本索引登记，核对依赖无环；信息充分且依赖完成后标 ready。日常“开始任务 001”即可指向对应任务，不需要另建应用里的 task。
3. 开始实现时标 in-progress；只按该任务范围工作。范围明显变化先更新任务，额外功能先创建新任务。
4. 每次 commit 更新 task 的实际进展、验证与清理/兼容记录；状态、依赖或标题变化时同步索引。当前可用平台验收已完成、只待明确列出的跨平台实机验收时标 implemented；此状态不阻塞依赖任务。全部验收完成才标 done，文档已写好不等于实现完成。

提交的最小完整性及消息格式见 [提交规范](standards/commits.md)，代码删除和兼容例外见 [代码生命周期](standards/code-lifecycle.md)。

## 状态约定

| 状态 | 含义 |
|---|---|
| draft | 缺少范围/验收，不能开始 |
| planned | 内容已编排，依赖尚未满足 |
| ready | 依赖完成且可开始，尚无实现 |
| in-progress | 正在实施 |
| implemented | 实现和当前可用平台验收已完成；仅待任务列出的 Windows / Wayland 等跨平台实机验收，不阻塞依赖任务 |
| blocked | 有具体阻塞，记录解除条件 |
| done | 验收完成且有证据 |
| deferred | 暂不排入当前交付 |
| cancelled | 已取消，保留编号和原因 |
