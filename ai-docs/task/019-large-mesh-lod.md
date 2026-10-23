# 019 — 大网格：流式资产读写、显示 LOD 与规模基准

- 状态：implemented
- 阶段：性能
- 依赖：[016](016-background-progress.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

长期目标是 1 亿～10 亿面网格快速加载、不卡顿。整网格内存表示为每面 72 B，10 亿面即 67 GiB，无法整体读入；需要按块流式处理并为显示生成面数受控的 LOD，同时提供可在用户机器上复现的规模基准。

## 必读

`crates/panta-mesh/src/asset/mod.rs`（v3 资产格式）、[提交规范](../standards/commits.md)。

## 范围与非目标

- `write_surface_asset_streaming`：按块回调生成三角面、分批并行压缩写入，输出与内存编码器逐字节一致（同一摘要），内存只与批大小相关。
- `SurfaceAssetChunks`：按批读取 v3 表面资产，并行解压并校验每块 SHA-256，可取消、可报告进度。
- `build_surface_lod`：两遍流式扫描（包围盒 → 均匀网格顶点聚类），输出面数 ≤ 预算（默认 2M）；源面数不超预算时原样返回。`simplify_surface` 为内存版本。
- `SyntheticTorus`：确定性合成曲面，可按下标并行生成任意面数。
- `examples/scale_bench.rs`：生成 → LOD → 整网格读取的耗时与峰值内存，输出 JSON。CI 只跑 30 万面小规模。
- 非目标（后续任务，见“决策与工作记录”）：应用激活大资产时自动改用 LOD 显示、LOD 上的拾取/诊断提示、分层 LOD 与视点相关细化、帧率自动测量。

## 保守决策

- LOD 只用于显示；质量诊断与导出仍基于完整网格，不得用 LOD 结果。
- 聚类采用均匀网格（拓扑可能变化、细小特征会合并），优先保证内存与时间上界；首遍超预算时在内存中按更小分辨率再聚类。
- 峰值内存仅在 Linux 通过 VmHWM 测得，其他平台输出 N/A；阶段间通过 `/proc/self/clear_refs` 重置，分配器保留的内存会计入后一阶段。
- 整网格读取默认仅在展开体积 < 可用内存 70% 时执行，避免基准把机器压到 OOM。
- 合成曲面平滑，zstd 压缩率（约 1/4.4）高于真实 CAE 数据，磁盘占用与生成耗时应视为下界。

## 验收标准

- [x] 流式写入与内存编码器摘要、内容一致；按批读取覆盖多块（`streaming_writer_matches_in_memory_encoder`）。
- [x] LOD 满足预算、点在包围盒内、小网格直通、可取消；内存聚类确定性（`lod::tests`）。
- [x] 本机 1M / 10M / 100M / 1B 基准完成并记录；CI 小规模基准步骤。
- [ ] 帧率：无 GPU 环境不可测，需实机。

## 本机基准（8 核、15 GiB 内存、无 swap、无 GPU；release）

| 面数 | 资产文件 | 生成 | LOD 耗时 / 输出面数 / 峰值 RSS | 整网格读取 / 峰值 RSS | 帧率 |
| --- | --- | --- | --- | --- | --- |
| 1M | 10.8 MiB | 0.08 s | 0.10 s / 1,000,000（直通）/ 210 MiB | 0.05 s / 210 MiB | N/A |
| 10M | 111 MiB | 0.70 s | 5.7 s / 1,623,397 / 873 MiB | 0.45 s / 995 MiB | N/A |
| 100M | 1.16 GiB | 6.2 s | 11.3 s / 1,524,935 / 820 MiB | 7.4 s / 6.85 GiB | N/A |
| 1B | 15.3 GiB | 69.6 s | 76.3 s / 1,493,422 / 843 MiB | 跳过（需 67 GiB） | N/A |

10M 的 LOD 首遍超出预算触发内存再聚类，因此单位面数耗时高于 100M。

## 在自己机器上跑（含 1B）

```bash
cargo run --release -p panta-mesh --example scale_bench -- --triangles 1B --dir /path/with/20GB/free
```

参数：`--triangles N`（支持 k/M/B 后缀）、`--dir`、`--budget N`（LOD 预算，默认 2M）、`--reuse`（复用已生成资产）、`--keep`（保留资产文件）、`--full auto|always|never`。1B 需要约 16 GiB 磁盘、约 1 GiB 内存（LOD 阶段）；整网格读取需要 >67 GiB 内存，默认自动跳过。最后一行 `JSON {...}` 便于汇总。macOS/Windows 峰值内存显示 N/A，可用 `/usr/bin/time -l` 或任务管理器观察。

## 验证计划与结果

| 验证 | 结果 |
| --- | --- |
| `cargo test --locked --workspace` | 通过 |
| `cargo format --check`、clippy、qmllint | 通过 |
| 本机规模基准 | 见上表 |
| 帧率 | 无 GPU，未测 |

## 清理与兼容例外

无格式变更：流式写入产出标准 v3 资产。

## 风险与回退

均匀聚类对细长/薄壁件可能出现孔洞或粘连；回退为不使用 LOD（现有整网格路径不变）。

## 决策与工作记录

- 2026-10-24：应用激活大资产时改用 LOD 显示（含状态提示、拾取改在 LOD 上）属界面集成，留待 019 的后续 PR；本 PR 只交付内核与基准。

## 完成摘要

流式读写、两遍流式 LOD、合成网格与规模基准已实现；本机 1B 面 LOD 76 s、峰值内存 <1 GiB。

## 后续：LOD 接入应用（PR feat/lod-activation）

- 激活时若 v3 表面资产面数 > `LOD_ACTIVATION_TRIANGLES`（20M），自动用 `build_surface_lod` 生成显示 LOD（预算 2M 面），只驻留 LOD；摘要先校验，取消在批次边界生效。
- `ProjectService` 记录 `mesh_lod_sources`（文档 id → 源面数）；重新导入/替换项目时清除。
- 保守决策：网格诊断与导出遇 LOD 返回 `project.mesh_lod_only`，UI 同时禁用按钮，避免在简化网格上给出错误结论或导出失真数据。
- 侧栏显示“大网格：显示简化 LOD N 面 / 源 M 面…”提示；拾取沿用显示三角形，因此在 LOD 上直接可用，坐标为近似值。
- 需真机/GPU 验证：大资产实际显示、帧率、LOD 上拾取精度。
- 导入解析的细粒度进度另开 PR。

## 后续：导入解析细粒度进度（PR feat/import-progress）

- 预检：读取按 256 KiB 块上报（0–50%），STL 解析按 65536 三角面 / 文本行上报（50–100%）；其他格式解析只报完成（保守，后续可逐格式补）。
- 确认导入：复核读取/解析 5–50%，缩放 60%，资产写入 60–90%（编码完成 20%，暂存写入按 4 MiB 块），清单提交 100%。资产格式与摘要不变。
- FFI `project_service_stl_preview_progress`；`importPreviewProgress` 属性；导入窗口显示百分比。
