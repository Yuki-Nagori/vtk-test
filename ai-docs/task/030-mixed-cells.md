# 030 — 原生混合单元：类型 / offset 布局、拆分、资产 v4、导入导出与质量

- 状态：in-progress
- 阶段：PR 1（Mesh IR、拆分、资产 v4）实施中；PR 2（VTU / 旧版 VTK 导入导出、单元场、诊断与质量）、PR 3（`.pchunk` v3、流式路径、混合基准）待开始
- 依赖：[029](029-result-fields.md)、[028](028-out-of-core-rollout.md)、[021](021-volume-mesh.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

体网格 IR 目前只有线性四面体；来源 VTU / 旧版 VTK 中的 hex、wedge、pyramid 无法导入。用户 2026-10-24 15:00 UTC+8 确认方案 A（原生混合单元）：Mesh IR 增加 VTK 风格的单元类型数组与 offset 布局，支持 tet / hex / prism(wedge) / pyramid，为 polyhedron 预留；局部节点顺序写进格式说明；资产格式与 `.pchunk` 升版本、旧派生缓存自动重建；切面、体积、质量计算时临时拆成四面体但保持原单元 ID；质量统计按单元类型（参考 Verdict / ParaView）；基准用棱柱边界层 + 四面体或六面体核心的合成混合网格。按 3–4 个 PR 拆分。

## 必读

- [网格架构](../architecture/mesh.md)（「原生混合单元」一节为格式说明）
- [资产激活](../modules/asset-activation.md)、[性能](../modules/performance.md)
- [提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)
- VTK 9.7 头文件 `vtkCellType.h`、`vtkWedge.h`、`vtkPyramid.h`、`vtkHexahedron.h`（本地 SDK `target/panta-deps/sdk`，2026-10-24 查阅）
- Verdict 库手册（scaled Jacobian、aspect ratio 等指标定义；PR 2 引用）

## 范围与非目标

- PR 1：`panta-mesh` 的 `CellType` / `CellLayout`、共形拆分表、`volume_mesh_from_cells`、`cell_volumes`、校验、资产 v4 读写与分块流式读取（`visit_cell_tets`）；结果场单元数改按原始单元计。
- PR 2：VTU / 旧版 VTK 读取 10/12/13/14（42 明确报错），VTU 原生单元导出，单元场按原单元对应，按类型的诊断与质量统计，FFI / QML 展示类型计数。
- PR 3：`.pchunk` v3 带原单元 ID（旧 v2 派生缓存因版本不符自动重建），分块质量与切面，混合流式写入，合成混合网格基准（读次数、字节、打开文件数）。
- 非目标：polyhedron 实际支持（需面流布局，另立任务）、二次单元、非共形（悬挂节点）网格、Netgen 输出混合单元。

## 涉及分组与跨组依赖

`panta-mesh`（IR、资产）、`panta-io`（导入导出，PR 2）、`panta-core`（工程 / 激活）、`panta-ffi` 与 QML（PR 2）。不新增跨组依赖。

## 前置条件与待决策

- 已决定：拆分不落盘、加载时确定性重算；混合资产用固定 40 B 记录（便于分块与流式），polyhedron 将来用独立流；`.presult` 容器保持版本 3，已有结果 sidecar 不失效；v3 资产继续可读，新写一律 v4。
- 待决定（PR 2 / 3）：质量指标集合（拟：全部类型给 scaled Jacobian，四面体保留最小二面角与长宽比）；`.pchunk` v3 中单元 ID 的存储方式。

## 实施步骤

1. PR 1：IR + 拆分表 + 资产 v4 + 测试 + 格式文档。
2. PR 2：导入导出、单元场、诊断与质量、FFI / QML。
3. PR 3：`.pchunk` v3、流式路径、混合基准与 nightly。

## 预计改动

- PR 1：新增 `crates/panta-mesh/src/cells.rs`、`mixed_tests.rs`；修改 `lib.rs`、`volume.rs`、`asset/{mod,stream,tests}.rs`、`synthetic.rs`；`panta-core` 结果场单元计数；所有 `TetMesh` 字面量增加 `cells: None`。
- 持久化：Mesh 资产头部版本 3 → 4；纯四面体资产目录与摘要不变。

## 清理与兼容例外

- COMPAT：v3 Mesh 资产只读（`is_readable_mesh_asset_version` 与 `open_layout`），新写入一律 v4；两者块布局一致，读取开销为零，暂不设删除任务（v3 为现存工程的主要格式）。
- 无废弃项。

## 验收标准

- [x] PR 1：hex 全部 40320 种节点标号、wedge / pyramid 全部标号拆分为正向四面体且体积正确；镜像输入自动修正；节点重复、polyhedron、offset 不符被拒绝。
- [x] PR 1：混合网格（hex + pyramid + wedge + tet）构建、校验、按单元体积、外表面正确；篡改区域或拆分顺序时校验失败。
- [x] PR 1：v4 混合资产完整读与流式读往返；混合流在 v3 头部下被拒绝；v3 纯四面体资产仍可读；`VolumeAssetChunks` 给出与驻留一致的拆分四面体与单元 ID。
- [ ] PR 2：VTU / 旧版 VTK 混合导入导出往返，单元场长度按原单元，诊断按类型统计。
- [ ] PR 3：`.pchunk` v3、混合基准（棱柱边界层 + 核心）与 Windows nightly。
- [ ] GUI / GPU 实机验收（混合网格切面显示与类型统计）待人工。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | `/workspace/vtk-test`，`cargo test -p panta-mesh --locked` | 全部通过 | 75 passed（含 6 个混合单元新测试与拆分表穷举测试） |
| 2026-10-24 | `cargo quality`（fmt + clippy + qmllint） | 通过 | 通过 |
| 2026-10-24 | `cargo test --locked --workspace`（Ninja，含 CTest） | 全部通过 | 全部通过，CTest 69/69 |
| 2026-10-24 | `QT_QPA_PLATFORM=offscreen timeout 8 target/native/debug/app/panta` | 启动后被 timeout 结束（124） | 124 |

## 风险与回退

- 拆分表错误会导致非共形或负体积：穷举测试覆盖全部节点标号；校验在加载时重算拆分并比对。
- v4 头部旧版本程序无法读取：只影响降级使用；纯四面体资产可把头部版本改回 3 恢复（目录摘要不含版本）。

## 决策与工作记录

- 2026-10-24：创建任务（用户 15:00 UTC+8 确认方案 A）。PR 1 实现 IR、拆分表（脚本生成后内嵌常量，单元测试穷举验证）、资产 v4 与流式单元读取；`VolumeStreamCounts` 新增 `cells`，`tets` 为拆分四面体数。导入器尚未产生混合网格，导出 / 质量 / `.pchunk` 留给 PR 2 / 3。

## 完成摘要

未完成。
