# Native 依赖供给与 SDK 制品

[模块导航](README.md) · [依赖获取规范](../standards/dependency-acquisition.md)

## 目标

VTK 负责渲染；OCCT 与 Netgen 支持 STEP、体网格生成和后续网格修复验证。应用继续消费 manifest 锁定的既有 Release，开发构建不从源码重建重库。

## 制品生命周期

1. [sdk-provision.cmake](../../native/cmake/sdk-provision.cmake) 登记版本、URL、SHA256、triple、ABI、package、targets 和许可证，通过 `panta_require_sdk` 供给并调用 `find_package(... CONFIG)`。
2. SDK 生产入口为 `.github/workflows/sdk-vtk.yml`、`sdk-occt-netgen.yml`、`sdk-googletest.yml`（`workflow_dispatch`，`publish=true` 才发布）：矩阵三平台按 `tools/sdk/` superbuild 生产，`tools/sdk/selfcheck` 以真实 configure 校验 package 与 targets，`package.cmake` 打包并生成 `.sha256`，三平台全部成功后由单一 job 清空旧资产并全量重传到单一 tag，说明取自 `tools/sdk/releases/`。只需 `github.token` 的 `contents: write`。重产后按 sidecar 与 Release API digest 回填 manifest（任务 041 自原 Panta 迁入）。
3. Netgen 的 OCC 路径链接构建时的 OCCT，两者必须按同工具链与 ABI 成对升级。自检需确认头文件、库、package、targets、许可证和元数据完整，再更新消费 manifest。
4. 供给缓存位于 `target/panta-deps/sdk/<name>/<version>/<triple>/`；下载按哈希验证，临时解包后原子发布，损坏 marker 按缓存重建，缺包和配置歧义立即失败。
5. 升级后验证三平台构建及实际窗口、STEP、网格冒烟；不能仅凭 package configure 成功宣称运行期可用。

## 统一布局与诊断

建议归档内部布局为：

```text
<sdk-root>/
├── include/
├── lib/                 # 静态库、导入库或平台对应目录
├── bin/                 # 运行期 DLL/工具（按平台需要）
├── lib/cmake/<Package>/ # CONFIG package 与 targets
├── share/licenses/
└── panta-sdk.json       # 版本、triple、ABI、Qt、模块、来源和哈希
```

供给失败的诊断至少包含依赖名、期望版本、平台 triple、编译器/运行库 ABI、staging 路径、缺失 target 或模块、manifest 来源和修复动作。不能静默改用系统库、另一个版本或源码构建。

## 当前消费边界

- manifest 消费 VTK 9.7.0 的 `sdk-vtk-9.7.0-webgpu` Release，以及 OCCT 8.0.1 / Netgen 6.2.2604 的配对 Release；确切资产 URL 与哈希以源码为准。
- OCCT targets 为 `TKernel` / `TKDESTEP`；Netgen package 名为大小写敏感的 `Netgen`，其运行期还需同平台 OCCT。VTK 使用 `VTK::RenderingWebGPU` 与 `dawn::webgpu_dawn`，不依赖 Qt VTK GUI 模块。
- `Build.SdkProvision` 以本地 fixture 覆盖哈希、缺资产、缓存、marker、版本隔离、嵌套归档和配置歧义；它验证供给逻辑，不能代替实际 SDK 链接与运行验收。

## 边界

Qt、CMake 和 Ninja 由既有托管工具链供给；本模块只维护重库 SDK 消费契约及制品元数据。运行库部署、签名和未来增量更新使用这些信息，分别验证自身生命周期。
