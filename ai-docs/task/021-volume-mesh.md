# 021 — 体网格：导入、显示/切片、导出、质量诊断、与 Netgen 体网格打通

- 状态：done
- 阶段：网格
- 依赖：[019](019-large-mesh-lod.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

当前工作台以表面网格为主；`panta-mesh` 已有四面体体网格的资产表示与基本校验（有符号体积），但没有面向用户的体网格导入、显示、切片、导出与诊断，Netgen 生成的体网格也未接入同一链路。本任务把体网格做成一等公民。OCCT STEP 与 Netgen 代码保留。

## 必读

`crates/panta-mesh/src/lib.rs`（体网格校验）、`crates/panta-mesh/src/asset/`、`native/mesh/include/panta/mesh/mesh_ir.hpp`、[UI 与桥接架构](../architecture/ui-and-bridge.md)。

## 范围与非目标（五个子项，可分 PR）

1. **Volume 导入**：VTU / 传统 VTK（UNSTRUCTURED_GRID）四面体单元导入为体网格资产；非四面体单元首期拒绝并给出明确错误（保守决策）。
2. **显示/切片**：显示体网格外表面（边界面提取），支持一个轴向/任意平面裁切并显示切面；大体网格沿用 019 的 LOD 思路仅简化显示表面。
3. **导出**：体网格导出 VTU（含原始节点与单元），外表面可另存 STL。
4. **质量诊断**：在现有有符号体积检查基础上增加纵横比 / 最小二面角等统计，与表面诊断共用结果面板与进度/取消（016）。
5. **与 Netgen 体网格打通**：Netgen 生成的体网格转换为同一体网格资产，走相同显示、诊断、导出路径。
- 非目标：六面体/棱柱/金字塔等混合单元、高阶单元、体网格编辑与重划分、多切面。

## 保守决策

- 首期仅支持线性四面体；遇到其他单元类型整体拒绝，不做部分导入。
- 切片为显示功能，不生成新的网格资产。
- 体网格质量指标阈值只做报告不做自动修复。

## 验收标准

- [x] 各子项单元测试（导入/导出往返、边界面提取、切面、诊断指标、Netgen 转换）。
- [x] format、clippy、qmllint、全量测试通过；offscreen 启动无 QML 警告。
- [ ] 实机：切片交互与大体网格显示流畅度（需 GPU）。

## 验证计划与结果

| 验证 | 结果 |
| --- | --- |
| 单元测试 | 通过：panta-mesh volume 4 项、panta-import 夹具/往返、panta-core project::volume、panta-ffi DTO 质量/导出、NetgenMesher.BoxYields 扩展断言；63 CTest 全过 |
| 实机 GUI | 未做（需 GPU）：切面交互、大体网格显示流畅度 |

## 清理与兼容例外

无。

## 风险与回退

体网格规模大时边界面提取与切面需流式/并行；回退为仅显示外表面。

## 决策与工作记录

- 2026-10-24：按用户要求先登记为正式任务（五个子项），实施分子项 PR 进行。

- 2026-10-24：PR A（领域层）：`panta-mesh::volume`（由单元构建 TetMesh、外表面提取、单平面切面、纵横比/最小二面角统计）；`panta-import` 体网格解析（VTU / 旧版 VTK，仅线性四面体，其他单元整体拒绝）与 VTU 体网格导出。负有向单元交换节点 2/3 修正朝向（不改几何）。
- 2026-10-24：PR B（工程/桥接/UI 集成）：网格类型 `solid-3d` + VTU/旧版 VTK 导入为体网格资产（kind `volume`），显示网格为外表面（大网格沿用 019 LOD）；含非四面体单元时整体拒绝 `project.import_volume_unsupported`。重新打开工程激活时恢复四面体数据。侧栏“体网格”区：节点/单元数、最小二面角、长宽比、劣质单元数（同步计算，阈值常量只报告）；X/Y/Z 单切面（仅替换视口显示，切面期间禁用导出/诊断）。导出 `.vtu` 写四面体单元（同步），其他格式导出外表面。偏离：质量统计未接入 016 诊断面板的进度/取消，待用户决定。
- 2026-10-24：PR C（Netgen 打通）：FFI `mesh_tet_quality` / `mesh_tet_export_vtu` 与 C++ `volume_quality_with_rust` / `export_native_mesh_vtu_with_rust`，Netgen 体网格与导入体网格共用 Rust 质量统计和 VTU 导出；Netgen 测试补充断言。OCCT STEP / Netgen 代码保持不动。

## 完成摘要

首期（线性四面体、单轴向切面）完成，共三个 PR：#20 领域层、#21 工程/桥接/UI 集成、PR C Netgen 路径打通。Netgen 生成的 `NativeTetMeshDto` 经 `TetMeshData` 转为同一 `TetMesh`，复用质量统计与 VTU 导出（`volume_quality_with_rust`、`export_native_mesh_vtu_with_rust`）。

遗留，需另立任务并经批准：
- 工作台中“STEP → Netgen 生成体网格”的用户命令和工程记录类型（ImportRecord 目前以源文件为中心），批准后 Netgen 结果才能作为工程资产进入显示和切面。
- 质量统计接入 016 诊断面板的进度和取消，以及后台执行。
- 任意平面切面。
