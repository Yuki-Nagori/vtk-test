# 016 — 可取消、带百分比的后台任务进度

- 状态：implemented
- 阶段：UI / 基础设施
- 依赖：[015](015-mesh-quality-panel.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

导入、导出、网格质量诊断都在后台线程执行，但界面只显示“处理中…”，导出和诊断也无法取消。本任务定义统一的进度协议，并接到这三类任务与状态栏。

## 必读

[UI 与桥接架构](../architecture/ui-and-bridge.md)、[分层规则](../standards/layering.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

- 协议：`panta_mesh::Progress`（最底层公共 crate）——原子万分比进度 + 取消标志；`report(start, end, done, total)` 把阶段内比例映射到整体区间并保证单调；检查点间隔 `PROGRESS_STRIDE = 65536` 项。
- 诊断：`diagnose_surface_with_progress`（焊接 0–60%、边统计 60–95%），取消返回 `DiagnosticError::Cancelled`。
- 导出：`export_surface_mesh_with_progress`（索引去重 0–40%，按估算字节写出 40–99%，原子替换后 100%）；取消删除 `.partial` 临时文件，目标文件不变，错误码 `project.export_cancelled`。
- 导入：阶段式进度（开始 5%、解析复核完成 50%、缩放 60%、资产写出 90%、清单提交 100%），沿用既有 `cancel_stl_import` 的提交闸门语义。
- FFI：opaque `TaskProgress`（共享 `Arc`）及 `task_progress_new/share/fraction/cancel`、`project_service_stl_import_progress`；`export_mesh_display`、`diagnose_mesh_display` 增加进度参数。
- ViewModel：`importProgress` / `exportProgress` / `diagnosticsProgress`（无任务为 -1，`taskProgressChanged` 约 100ms 一次）、`cancelExport()`、`cancelDiagnostics()`。
- QML：状态栏显示“导入/导出/诊断 NN%”、进度条与“取消”按钮。
- 非目标：工程创建/打开/保存的进度；STL 预览解析的细粒度进度；进度持久化或任务队列 UI。

## 保守决策

- 取消是协作式的：工作线程只在检查点停止；导入一旦越过清单提交闸门即不可取消（与既有语义一致）。
- 导入不改动 panta-import 解析器内部，只按阶段报告，避免触碰已验证的读取/复核路径；细粒度解析进度留给 task 019（大网格 out-of-core）。
- 导出字节总量是估算值，写出阶段的进度封顶 99%，只有原子替换完成才到 100%。
- 取消后的完成信号仍会到达（`succeeded=false`），状态栏显示“导出已取消。”/“网格质量诊断已取消。”。
- 状态栏同时只显示一个任务，优先级 导入 > 导出 > 诊断（三者已因 busy 互斥）。

## 验收标准

- [x] `Progress` 单调、阶段映射、取消标志（Rust 单测）。
- [x] 诊断完成到 100%，预先取消返回 `Cancelled` / `project.diagnostics_cancelled`（Rust 单测 + FFI 单测）。
- [x] 导出完成到 100%；取消不留目标与临时文件（Rust 单测 + C++ ViewModel 测试）。
- [x] 排队中的导入进度可查询为 0，取消后为 None（Rust 单测）。
- [x] ViewModel 进度属性、取消接口、取消后的错误码（C++ `ReportsProgressAndCancelsDiagnosticsAndExport`）。
- [ ] 真实窗口目测状态栏进度条与取消按钮；大网格上进度平滑度（无 GUI 环境，待实机验证）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | 本机 Linux `cargo test --locked --workspace`（CTest 62 项、qmllint）、`cargo lint clippy --check`、`cargo format --check` | 通过 | 通过 |
| 2026-10-24 | CI Linux test + quality | 通过 | 见 PR |
| — | 真实窗口目测、大网格取消响应 | 平滑、及时 | 无 GUI 环境，未执行 |

## 清理与兼容例外

`export_surface_mesh`、`diagnose_surface`、`export_mesh_file` 保留为无进度的薄包装（CLI 与既有调用方使用），不是兼容垫片。无废弃项。

## 风险与回退

检查点开销：每 65536 项一次原子读写，可忽略。回退即 revert。

## 决策与工作记录

- 2026-10-24：创建并实施。

## 完成摘要

导入/导出/诊断均有百分比进度，导出与诊断新增协作式取消；待实机目测。
