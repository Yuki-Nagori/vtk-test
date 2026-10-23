# 006 — STL 修复前置：表面网格质量诊断

- 状态：done
- 阶段：STL 修复
- 依赖：无
- 优先级：P1
- 负责人：待分配
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

README 将“网格修复”列为待实现；`stl inspect` 明确“不评估网格质量”。修复前需要先能识别问题：重复 / 未焊接顶点、退化面、开放边界（孔）、非流形边、法向不一致与自相交。完成后 Rust 侧对已解析 `SurfaceMesh` 输出可测试的诊断报告，并可经 CLI 查看。

## 必读

[网格架构](../architecture/mesh.md)、[几何架构](../architecture/geometry.md)、[分层规则](../standards/layering.md)、[测试规范](../standards/testing.md)。

## 范围与非目标

- 纯拓扑诊断（顶点焊接容差、边邻接统计、退化面、法向一致性）优先在 Rust 实现；自相交可后续评估是否走 OCCT。
- 非目标：实际修复（见 [007](007-stl-repair-pipeline.md)）、GUI 展示。
- OCCT STEP 与 Netgen 网格适配必须保留，后续修复会复用。

## 前置条件与待决策

- 已决策：焊接容差为相对值，默认 `包围盒对角线 × 1e-6`，单位随来源坐标（与 STL 不记录单位一致），CLI 用 `--weld-tolerance <比例>` 覆盖；0 表示仅合并完全相同坐标。
- 已提供 `tests/fixtures/mesh/` 小型缺陷 STL 夹具。
- 自相交检测本次不实现（需要空间加速结构或 OCCT，超出纯拓扑诊断范围），列为后续候选，未建任务。

## 验收标准

- [x] 各类缺陷有专门 fixture 与断言，干净模型报告无问题。
- [x] CLI 输出诊断摘要；大文件不会因诊断显著放大内存（记录实测）。

## 风险与回退

诊断只读，不改工程数据。

## 决策与工作记录

- 2026-10-24：创建任务，待细化容差与 fixture 后转 ready。
- 2026-10-24：实现 `panta_mesh::diagnose_surface`（网格哈希焊接、无向边邻接统计、并查集统计孔数），`ProjectService::inspect_stl_with_diagnostics` 复用预检解析结果，`stl inspect` 输出诊断并支持 `--weld-tolerance`。退化面（焊接后顶点重合或面积 ≤ 容差²）不参与边拓扑统计。
- 验证：`cargo test --locked -p panta-mesh -p panta-cli -p panta-import -p panta-core` 通过；Clippy `-D warnings` 通过；完整 workspace 行为测试由 PR Linux CI 执行。
- 内存实测（release，500k 三角形 25 MB binary STL）：改动前 `stl inspect` 峰值 RSS 60 MB，加诊断后 138 MB，耗时 0.75 s；额外占用来自焊接索引和边表，与焊接后顶点数/边数线性相关，未复制三角形坐标。

## 完成摘要

`stl inspect` 输出近重复顶点、退化面、边界边与孔、非流形边、法向不一致边统计；焊接容差默认相对包围盒对角线 1e-6，可配置。六个缺陷/干净夹具覆盖各类问题。自相交检测留作后续候选。
