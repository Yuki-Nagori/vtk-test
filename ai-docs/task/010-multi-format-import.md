# 010 — 多格式网格导入（VTU / VTP / VTK / OBJ / PLY）

- 状态：done
- 阶段：网格导入
- 依赖：[006](006-stl-mesh-diagnostics.md)（诊断复用同一预检网格）
- 优先级：P1
- 负责人：待分配
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

导入只支持 STL。仿真结果和中间网格常见 VTK XML（`.vtu` / `.vtp`）、旧版 `.vtk`、OBJ、PLY。完成后 GUI 导入对话框与 CLI 可直接导入这些格式，结果仍是现有 `SurfaceMesh` → Mesh 资产管线，记录中的 `format` 反映真实来源格式。

## 必读

[规范索引](../standards/README.md)、[命令层](../architecture/command-layer.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)。VTK 文件格式依据：VTK 官方 “VTK File Formats” 文档（docs.vtk.org，2026-10-24 查阅）与 VTK 9.7.1 writer 实际输出。

## 范围与非目标

- 范围：VTU（UnstructuredGrid）、VTP（PolyData）的 ascii / inline binary / appended raw·base64、UInt32 / UInt64 头、`vtkZLibDataCompressor`；旧版 `.vtk` ASCII / BINARY 的 POLYDATA 与 UNSTRUCTURED_GRID，4.2 计数前缀和 5.1 OFFSETS/CONNECTIVITY 两种单元布局；OBJ（多边形面、负索引）；PLY（ascii、binary 大小端）。VTK 线性单元 1–14 与二次单元 21–25（按角点降阶）。体单元只保留外表面。
- 非目标：点/单元属性（标量、向量、时间步）、多 Piece / `.pvtu` 并行文件、LZ4/LZMA 压缩、高阶 Lagrange 单元、OBJ 材质与纹理、PLY 颜色。

## 涉及分组与跨组依赖

只改 `panta-import`（新增 `formats` 模块与 `flate2` 依赖，纯 Rust 后端）、`panta-core` 错误映射、`panta-cli` 命令入口和 QML 文案；无新增跨组依赖，CLI / core 仍不依赖 native。

## 前置条件与待决策

- 决策（保守）：解析器用 Rust 实现在 `panta-import`，而不是调用 VTK C++ reader。理由：CLI 与 core 不依赖 native，STL 解析已在 Rust；复用 VTK reader 需要新增 native → Rust 反向注入架构，超出本任务范围，已列为候选任务。
- 决策：格式由扩展名决定（与 STL 一致），不做内容嗅探。
- 决策：体单元面按排序节点集合做奇偶配对，出现奇数次的面视为外表面；三个以上单元共享同一面的非协调网格结果可能与 VTK 不同。

## 实施步骤

1. `formats/`：`TriangleSink` 输出三角形（解析器不绑定整体 `Vec`，后续可接流式/分块写入）、`CellSurface` 单元转表面、各格式解析器。
2. `ImportFormat` 新增 `Vtu / Vtp / VtkLegacy / Obj / Ply`，`parse_mesh_snapshot` 统一分发；`PreparedStl.format` 写入 `ImportRecord.format` 与默认显示名。
3. `ImportError::Format` 映射到 `project.import_parse_failed` / `project.asset_parse_failed`。
4. CLI 新增 `mesh inspect|import`，`stl` 保留为同义别名；QML 导入对话框文件过滤与文案改为“网格文件”。

## 预计改动

`crates/panta-import/src/{lib.rs,formats/*}`、`Cargo.toml`（`flate2`，rust_backend）、`crates/panta-core/src/{activation.rs,project/import.rs,project/workspace.rs}`、`crates/panta-cli/src/cli.rs`、`qml/{App.qml,Dialogs/ImportDialog.qml}`、`tests/fixtures/mesh/formats/`、`tests/rust/cli.rs`、README 与命令层文档。持久化格式不变（`format` 字段新增取值），`IMPORT_RECORD_VERSION` 不变。

## 清理与兼容例外

`parse_stl_snapshot` 替换为 `parse_mesh_snapshot`，调用点全部更新。CLI `stl` 子命令保留为别名（非兼容分支，属同一实现）。无兼容例外。

## 验收标准

- [x] 14 个 VTK writer 生成夹具三角形数与 VTK 表面过滤结果一致，诊断均为闭合、法向一致。
- [x] `mesh import` VTU 写入工程，记录 `format = "vtu"`，重开工程可读。
- [x] 非法索引、未知数据集、类型不符被拒绝；STEP / IGES 和未知扩展名仍返回不支持格式。
- [x] fmt、clippy、`cargo test` 通过；CI 全绿后合并。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | Linux，Rust 1.99.0，`cargo test -p panta-import -p panta-core -p panta-cli -p panta-mesh` | 全部通过 | 通过 |
| 2026-10-24 | release `mesh inspect`，3,072,000 四面体 / 531,441 点 zlib appended VTU（26.7 MB） | 外表面 76,800 三角形，与 VTK 一致 | 初版（每单元 `Vec` + 面计数表）6.43 s / 峰值 RSS 1121 MB；改为扁平单元表 + 面奇偶配对后 1.54 s / 244 MB |

## 风险与回退

自写 XML 标签扫描只覆盖 VTK writer 的输出结构；遇到不支持的写法返回解析错误，不会写入工程。回退：撤销本任务提交即恢复仅 STL。

## 决策与工作记录

- 2026-10-24：创建并实现。解析器以 `TriangleSink` 输出，便于后续接入分块 / 流式 Mesh IR（任务 011 方向）。

## 完成摘要

GUI 与 CLI 支持 STL、VTU、VTP、旧版 VTK、OBJ、PLY 导入与预检诊断。未覆盖：属性/时间步、并行文件、高阶单元、VTK native reader 路线（候选任务）。
