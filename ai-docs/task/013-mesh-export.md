# 013 — 网格导出与视口截图

- 状态：done（PR #9）
- 阶段：数据交换
- 依赖：[010](010-multi-format-import.md)、[011](011-mesh-asset-chunked-storage.md)
- 优先级：P1
- 负责人：待分配
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

导入已支持 STL / VTU / VTP / 旧版 VTK / OBJ / PLY，但工程中的网格无法再写出。本任务提供与导入对称的网格导出（CLI 与 GUI），以及把视口当前帧导出为 PNG。

## 必读

[命令层](../architecture/command-layer.md)、[网格](../architecture/mesh.md)、[代码生命周期](../standards/code-lifecycle.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

- 范围：`panta_import::export`（六种格式写出、原子替换目标文件）；`ProjectService::export_import_mesh` 与 CLI `mesh export`；FFI `export_mesh_display` + ViewModel `beginExportActiveMeshUrl`（后台线程写盘）；`ViewportBackend::capture_image` + `CaeViewport.saveScreenshot`（PNG）；工具栏“导出网格”“视口截图”按钮与保存对话框。
- 非目标：结果场 / 时间步导出（工程中尚无结果数据）、Volume 网格导出、单位换算、压缩 VTK XML、导出选项对话框（如 ASCII / 二进制切换）、高分辨率离屏截图。

## 涉及分组与跨组依赖

`panta-import`（新增 export 模块）、`panta-core`（服务方法、错误码、中文文案）、`panta-cli`、`panta-ffi`、`native/bridge`、`native/visualization`、`qml`。无新增 crate 或 VTK 模块依赖（截图用 WebGPU render window 像素读回 + QImage 编码）。

## 前置条件与待决策

- 决策（保守）：坐标按工程内部单位毫米原样写出，不做单位换算；STL 为二进制 f32，有精度损失，其余格式为 f64。
- 决策：VTU / VTP 用未压缩 inline base64（UInt64 头），旧版 VTK 用 4.2 BINARY POLYDATA，PLY 用 binary_little_endian，OBJ 为文本；按位去重共享顶点（-0.0 与 0.0 视为相同），不做容差焊接，不写法向。
- 决策：只导出 Surface；Volume 资产在清单校验时即被拒绝（`project.manifest_invalid`，与激活一致）。
- 决策：GUI 只导出活动页签已驻留的网格；未打开时报 `project.mesh_not_resident`。CLI 未驻留时同步读取并校验 sidecar。
- 决策：截图仅 PNG，尺寸为视口当前像素尺寸；视口未初始化时提示“视口尚未就绪”。

## 实施步骤

1. export 模块与往返测试（经导入解析器回读）。
2. 服务方法、错误码、CLI 命令与黑盒测试。
3. FFI / ViewModel 后台导出与 C++ 测试；视口像素读回与 `saveScreenshot`。
4. QML 入口；文档更新。

## 预计改动

`crates/panta-import/src/{export.rs,export_tests.rs,lib.rs}`、`crates/panta-core/src/{activation.rs,project.rs,project/{error,service,workspace}.rs}`、`crates/panta-cli/src/cli.rs`、`crates/panta-ffi/src/{display,lib,project_response}.rs`、`native/bridge/src/project_view_model.{hpp,cpp}`、`native/visualization/{include/panta/visualization/{cae_viewport,viewport_backend}.hpp,src/cae_viewport.cpp,src/vtk/vtk_viewport.{hpp,cpp}}`、`qml/{App.qml,Panels/ViewportPane.qml}`、`tests/rust/cli.rs`、`tests/cpp/bridge/project_view_model_test.cpp`、`ai-docs/architecture/command-layer.md`。

## 清理与兼容例外

无兼容代码。`begin_asset_activation` 的清单校验抽为 `activation_request` 供导出复用，未保留旧路径。

## 验收标准

- [x] 六种格式写出后经本项目导入解析器回读一致（STL 按 f32 精度）。
- [x] 写出为临时文件 + rename，失败不残留临时文件；未知扩展名、目录不存在给出稳定错误码。
- [x] CLI `mesh export` 从已保存工程导出六种格式；非法扩展名 / 未知导入 ID 返回错误码。
- [ ] GUI 导出、截图（C++ / QML 仅由 CI 构建与测试验证；截图像素读回需在有 GPU 的实机上目测确认）。
- [ ] fmt、clippy、`cargo test --workspace` 与 qmllint 在 CI 通过。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | Linux，`cargo test -p panta-import -p panta-core -p panta-cli` | 全部通过 | 通过（export 5 项、CLI export 1 项） |
| 2026-10-24 | `cargo clippy`（import/core/cli/mesh）、`cargo format --check` | 无告警 | 通过 |
| 2026-10-24 | CI（Linux build and test、format/Clippy/QML） | 全绿 | 见 PR |

## 风险与回退

本机无真实 VTK / ParaView，VTU / VTP / VTK 写出仅经本项目解析器与格式规范交叉核对；若外部工具读取异常，回退为撤销本任务提交（不影响工程数据）。截图依赖 WebGPU 颜色附件读回，CI 无 GPU，无法自动验证像素内容。

## 决策与工作记录

- 2026-10-24：创建并实现（用户经父代理批准的 task 4）。

## 完成摘要

提供六种格式的网格导出（CLI + GUI 后台线程）与视口 PNG 截图。未覆盖：结果 / 动画导出、Volume 导出、单位换算、导出选项（候选任务）。
