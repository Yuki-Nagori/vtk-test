# 011 — Mesh 资产分块存储（sidecar v3）

- 状态：done
- 阶段：网格存储
- 依赖：[010](010-multi-format-import.md)
- 优先级：P1
- 负责人：待分配
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

sidecar v2 是单个 Zstandard 流加整段 SHA-256：编码、解码和校验只能单线程顺序进行，读取前必须整段载入内存，无法局部读取，也没有为 LOD、时间步预留位置。长期目标是亿级网格和时序动画，Mesh IR 需要走向分块、零拷贝 / mmap、按需与流式加载。本任务交付分块的 v3 文件格式与流式读取，作为后续 LOD、out-of-core 与动画任务的存储基础。

## 必读

[网格](../architecture/mesh.md)、[资产激活](../modules/asset-activation.md)、[代码生命周期](../standards/code-lifecycle.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

- 范围：v3 格式（头部 + 目录 + 64 B 对齐数据块、逐块 zstd、逐块 SHA-256、目录摘要即资产摘要）；并行编码 / 解码；`MeshAssetReader` 流式读取并支持取消；激活改用流式读取；v2 只读兼容。
- 非目标：LOD 生成、时间步 / 动画数据、mmap 零拷贝显示路径、out-of-core 局部加载、v2 自动迁移改写、焊接索引存储。目录中的 LOD / 时间步 / filter 字段仅预留，v3 读取端要求为 0。

## 涉及分组与跨组依赖

`panta-mesh`（asset 模块拆为 `asset/mod.rs`、`asset/v2.rs`、`asset/tests.rs`，新增 bench example）与 `panta-core`（激活读取、版本判定）。无新增跨组依赖、无新增 crate 依赖；native / FFI 不读取 sidecar（已检索确认）。

## 前置条件与待决策

- 决策（保守）：v2 只读、不自动改写；已有工程激活时继续读 v2，新导入一律写 v3。清单 schema 不变，`format_version` 记录 2 或 3。
- 决策：不做字节重排 shuffle。实测 sphere 数据 shuffle 后 zstd 输出由 33.6 MB 增至 68.5 MB，filter 字段保留为 0 供以后按数据集评估。
- 决策：块大小目标 4 MiB 未压缩、上限 64 MiB；zstd 级别 3（与 v2 相同）。
- 决策：v3 头部 / 目录格式错误映射为 `project.asset_unsupported_format`，块内容错误映射为 `project.asset_parse_failed`，I/O 错误映射为 `project.asset_read_failed`。

## 实施步骤

1. 定义 v3 头部、目录条目与 stream（SurfaceTriangles / VolumeCounts / VolumeNodes / VolumeTets / VolumeBoundary），并行编码。
2. `decode_mesh_asset` 内存解码与 `MeshAssetReader` 流式解码；严格校验对齐、重叠、排序、预留字段、record_count、长度与 EOF。
3. 原实现移入 `asset/v2.rs`，仅保留解码，并加入 v2 夹具测试。
4. 激活改用 `MeshAssetReader`；服务版本判定改用 `is_readable_mesh_asset_version`。

## 预计改动

`crates/panta-mesh/src/{asset/*,lib.rs}`、`crates/panta-mesh/tests/data/*-v2.pmesh`、`crates/panta-mesh/examples/mesh_asset_bench.rs`、`crates/panta-core/src/{activation.rs,project/service.rs}`、`crates/panta-core/Cargo.toml`（dev 依赖 sha2）、`tests/rust/activation.rs`、架构与模块文档。

## 清理与兼容例外

- `COMPAT(task 011; remove-task 012)`：`crates/panta-mesh/src/asset/v2.rs`（v2 只读解码）、`is_readable_mesh_asset_version` 中的版本 2 分支、`asset/tests.rs` 的 v2 夹具测试、`tests/rust/activation.rs` 的 v2 激活测试及 `tests/data/*-v2.pmesh` 夹具。原因：task 011 之前保存的工程仍登记 v2 sidecar。删除条件：见 [012](012-mesh-asset-v2-removal.md)（提供迁移手段或确认不再支持旧工程后）。
- v2 编码路径已删除，不保留双写。

## 验收标准

- [x] Surface / Volume 往返一致，多块并行编解码正确，摘要稳定。
- [x] 目录非法条目（未对齐、重叠、越界、乱序、预留字段非 0、未知 codec / stream）、record_count 不符、NaN、长度不一致均被拒绝。
- [x] 流式读取可在批次间取消；2 GiB 展开上限仍生效。
- [x] 旧 v2 夹具可解码，v2 工程可激活且不改写清单。
- [x] fmt、clippy、`cargo test` 通过；CI 全绿后合并。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | Linux，`cargo test -p panta-import -p panta-core -p panta-cli -p panta-mesh` | 全部通过 | 通过（panta-mesh 27 项，activation 9 项） |
| 2026-10-24 | release `mesh_asset_bench`，8 核，sphere 3,914,400 三角形 | v3 不劣于 v2 | 编码 0.65 s → 0.13 s；体积 33.60 → 33.25 MB；内存解码 0.31 s → 0.134 s（峰值 RSS 305 → 335 MB，因整文件读入）；流式解码 0.14 s / 311 MB |
| 2026-10-24 | 同上，50 万三角形 | 同上 | 编码 0.073 → 0.016 s；3.89 → 3.88 MB；解码 0.041 → 0.02 s |

## 风险与回退

新导入的 v3 工程无法被旧版本程序读取（旧程序会报不支持的格式，不会损坏数据）。回退：撤销本任务提交；已写出的 v3 sidecar 需重新导入。

## 决策与工作记录

- 2026-10-24：创建并实现。目录预留 LOD / 时间步 / filter 位，供后续任务扩展而无需改头部。

## 完成摘要

sidecar 升级为分块 v3，编解码并行化，激活走流式读取并支持取消；v2 只读兼容登记于 012。未覆盖：LOD、动画时间步、mmap 零拷贝、局部加载（候选任务）。
