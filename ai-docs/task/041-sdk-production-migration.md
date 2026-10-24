# 041 — 预编译 SDK 生产迁入 vtk-test 并开启 RenderingAnnotation

- 状态：in-progress
- 阶段：构建与依赖供给（[038](038-ui-stage3-viewport-legend.md) 的前置）
- 依赖：无（只读参考 `Yuki-Nagori/panta` 的 `tools/sdk/` 与 `sdk-*.yml`）
- 优先级：P0
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

038 需要用 `vtkScalarBarActor`（034 的 Q2）在视口内画色标，但当前消费的 `sdk-vtk-9.7.0-webgpu`（原 Panta Release）没有 `VTK::RenderingAnnotation`，归档内不存在 `vtkScalarBarActor.h`。用户 2026-10-24 决定：把 panta 中发布预编译 SDK 的 workflow 与相关脚本、CMake 配置、文档迁入 vtk-test，此后以 vtk-test 为 base 自行构建和发布 SDK；panta 仓库只读、不修改。创建本仓库的 SDK Release 属于用户已同意的范围。

完成后：本仓库 `.github/workflows/sdk-*.yml` 可手动生产并发布 VTK、OCCT/Netgen、GoogleTest 三类 SDK；`native/cmake/sdk-provision.cmake` 的 URL 与 SHA256 全部指向 vtk-test Release；VTK SDK 额外提供 `VTK::RenderingAnnotation`。

## 必读

[依赖获取规范](../standards/dependency-acquisition.md)、[技术基线](../standards/baseline.md)、[VTK 规范](../standards/vtk.md)、[OCCT 规范](../standards/occt.md)、[Native 依赖供给](../modules/native-dependency-supply.md)、[质量工具](../modules/quality-tooling.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)。VTK 模块依赖以 commit `23f0a095621e91bbdbeace8451e22b950c8e5f46` 的 `Rendering/Annotation/vtk.module` 为准（2026-10-24 查阅）。

## panta 现状调查（只读，panta HEAD `ee84d95`）

| 项 | 事实 |
|---|---|
| 覆盖 SDK | VTK 9.7.0 + Dawn（`sdk-vtk.yml`）；OCCT 8.0.1 + Netgen 6.2.2604 成对（`sdk-occt-netgen.yml`）；GoogleTest 1.18.0（`sdk-googletest.yml`） |
| 平台 | `macos-latest`→`macos-arm64`（Ninja，`MACOSX_DEPLOYMENT_TARGET=13.0`）、`ubuntu-latest`→`linux-x86_64`（Ninja，装 `libwayland-dev wayland-protocols libxkbcommon-dev`）、`windows-2022`→`windows-x86_64`（VS 17 2022 x64） |
| 流程 | `workflow_dispatch`（`publish` 布尔输入）；矩阵 job：superbuild 生产 → selfcheck → `package.cmake` 打包 → upload-artifact；`publish=true` 时单一收口 job 下载三平台制品，Release 不存在则创建，存在则刷新说明、删全部旧资产后 `--clobber` 重传 |
| 命名与 tag | tag `sdk-vtk-9.7.0-webgpu`、`sdk-occt-netgen-8.0.1-6.2.2604`、`sdk-googletest-1.18.0`；资产 `<name>-<version>-<triple>.tar.gz` 与同名 `.sha256`；说明取自 `tools/sdk/releases/*.md` |
| 耗时（panta 2026-10-14 运行） | VTK：macOS ≈11 min、Linux ≈38 min、Windows ≈43 min；OCCT/Netgen ≈34–38 min；GoogleTest 数分钟。均在 GitHub 托管 runner 默认 360 min 上限内 |
| secrets | 只用 `github.token`（workflow 声明 `contents: write`，GoogleTest 仅 publish job 声明 write）；无其他 secret |
| Rust 工具 | panta 没有 SDK 生产相关的 Rust/xtask 代码；SDK 消费侧（`panta-build`、`sdk-provision.cmake`）vtk-test 已有，无需迁移 |

## 范围与非目标

本次交付：

1. 迁入 `.github/workflows/sdk-vtk.yml`、`sdk-occt-netgen.yml`、`sdk-googletest.yml`，以及 `tools/sdk/` 的 `CMakeLists.txt`、`vtk.cmake`、`vtk-cxx20.cmake`、`install-dawn.cmake`、`occt.cmake`、`netgen.cmake`、`package.cmake`、`selfcheck/`（含 `googletest/`）；源码 pin、构建选项与 panta 相同，仅注释来源改为“原 Panta 任务 NNN，vtk-test 任务 041 迁入”。
2. VTK 构建打开 `VTK_MODULE_ENABLE_VTK_RenderingAnnotation=YES`，selfcheck required targets 与 `panta-sdk.json` 的 `modules_highlights` 增加 `RenderingAnnotation`。
3. 在 vtk-test 上触发三条 workflow 的 `publish=true` 得到本仓库 Release；`sdk-provision.cmake` 的 URL / SHA256 切到本仓库资产（SHA 以 sidecar 与 Release API digest 双重核对），VTK `REQUIRED_TARGETS` 增加 `VTK::RenderingAnnotation`。
4. 更新 README、`tools/sdk/releases/*.md`、依赖获取规范、技术基线、VTK/OCCT 规范、Native 依赖供给与质量工具模块文档中“本仓库不保留生产 workflow / 消费 panta Release”的表述。

非目标：

- 不修改 panta 仓库；不修改本仓库设置（含 Actions 权限）。
- 不升级任何 SDK 版本或源码 pin；OCCT/Netgen/GoogleTest 只按原描述在本仓库重产。
- 不为 038 以外的需求增开 VTK 模块（见下文评估）。
- `vtkScalarBarActor` 的接入与色标 UI 属于 038。

## RenderingAnnotation 与其他模块评估

- `VTK::RenderingAnnotation` 的 DEPENDS 为 CommonCore/CommonDataModel/CommonExecutionModel/RenderingCore，PRIVATE_DEPENDS 为 CommonMath/CommonTransforms/FiltersCore/FiltersGeneral/FiltersSources/ImagingColor/RenderingFreeType。对照现有归档，只有 `ImagingColor` 是新增模块，体积影响很小。
- `vtkScalarBarActor` 依赖 `vtkTexturedActor2D`、`vtkPolyDataMapper2D`、`vtkTextActor`，均在 RenderingCore/RenderingFreeType 中，现有 WebGPU 已实现 2D mapper；VTK 上游 `RenderingWebGPU` 的 TEST_DEPENDS 本身包含 `RenderingAnnotation`。
- 其他候选：`vtkCubeAxesActor`、`vtkCornerAnnotation`、`vtkAxesActor` 同属 RenderingAnnotation，后续如需可直接使用；`InteractionWidgets`（方向小部件）不在 034/038–040 范围，不开启。039 的颜色图预设可用 `vtkColorTransferFunction`（RenderingCore）实现，无需 `CommonColor`/`vtkNamedColors` 以外的新模块。结论：只增 RenderingAnnotation。
- 待验证：scalar bar 在 WebGPU 真实窗口上的绘制效果需 GPU 实机确认（offscreen 测试仅能覆盖场景装配与参数）。

## 与 sdk-provision.cmake 的对接

- tag 与资产名沿用原名（仓库不同，不冲突），URL 只把 `Yuki-Nagori/panta` 换成 `Yuki-Nagori/vtk-test`；版本号不变。
- 缓存失效：staging marker 记录归档 SHA256，SHA 变化即判定未命中并重新下载、解包、原子替换，开发者已有的旧 `target/panta-deps/sdk/vtk/9.7.0/<triple>` 无需手动清理。
- 切换后 `native/` 编译器与 ABI 记录不变；Linux 仍由 ubuntu-24.04 gcc13 生产（若 runner 镜像变更，以 `panta-sdk.json` 与 selfcheck 为准记录）。

## 涉及分组与跨组依赖

构建与依赖供给（`.github/workflows`、`tools/sdk`、`native/cmake`）与文档；不新增跨组依赖。

## 前置条件与待决策

- 若 Actions 的 `GITHUB_TOKEN` 没有创建 Release 的写权限、runner 资源不足或构建超时：不改仓库设置，停下汇报缺什么。当前 PAT 无法读取 Actions 权限设置（HTTP 403），需以实际 publish 运行结果为准。
- 若 OCCT/Netgen 或 GoogleTest 重产失败而 VTK 成功：只切 VTK，其余保留 panta URL 并汇报。

## 实施拆分

1. PR A：迁入 workflow、`tools/sdk`、RenderingAnnotation 开关与文档；合并后在 main 上 dispatch 三条 workflow（`publish=true`）。
2. PR B：manifest URL / SHA256 与依赖文档切换；PR CI 与 nightly 全绿后合并。

## 验收

- 三条 workflow 在 vtk-test 三平台生产、自检、发布成功（记录 run 链接）。
- 新 VTK 归档含 `include/vtk-9.7/vtkScalarBarActor.h` 与 `VTK::RenderingAnnotation` target。
- manifest 切换后 PR CI（三平台）与 nightly 通过；`Build.SdkProvision` 与全部 ctest 通过。

## 验证记录

| 日期 | 项 | 结果 |
|---|---|---|
