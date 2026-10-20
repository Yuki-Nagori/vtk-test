以下记录原 Panta 已发布 OCCT / Netgen SDK 的来源与构建参数，原始[构建描述](https://github.com/Yuki-Nagori/panta/tree/main/tools/sdk/)位于来源仓库。
本仓库通过 `sdk-provision` 的固定 URL / SHA256 消费成对制品，普通开发与 CI 不编译两者源码。

## 为何合并发布

Netgen 以 `USE_OCC=ON` 构建时在 C++ 层直接链接 OCCT（`ngcore`/`nglib` 的
接口使用 OCCT 类型），而 OCCT 不承诺跨版本的 C++ ABI 稳定性——Netgen 制品
只与**构建它时所链接的那个 OCCT** 兼容，混用新旧版本会在加载/链接期失败，
甚至静默出错。因此两者在原生产管线以同一工具链生产，并作为**一个 GitHub Release 成对
发布**：配对关系在制品层就是原子的，消费者不可能拿到错配组合；任何一侧
升级都必须成对重建、成对发布（Release 标签同时编码两个版本）。配对与
版本关系同时记录在依赖规范的"依赖间版本关系"与每个归档的
`panta-sdk.json` 中。

## 源码与可复现性

- OCCT：<https://github.com/Open-Cascade-SAS/OCCT.git> tag `V8.0.1`
  解引用 commit **`b8f597c677811d1f9f4d8a97f5ae2825c0353a42`**
- Netgen：<https://github.com/NGSolve/netgen.git> tag `v6.2.2604`
  解引用 commit **`3ee489c7d58fdbc2a6708cca3cbaefaae506dc17`**
- 两者均为未修改的上游源码。
- OCCT 构建配置：Release、共享库；模块 `Draw`/`Visualization`/`DETools`
  关闭，`USE_FREETYPE`/`USE_XLIB` 关闭（渲染归 VTK；制品不依赖系统第三方
  库）；其余默认启用（含 STEP 所需 DataExchange）。
- Netgen 构建配置：Release、共享库；`USE_OCC=ON`、`USE_GUI/USE_PYTHON/
  USE_MPI/USE_JPEG/USE_MPEG` 关闭；`USE_NATIVE_ARCH=OFF`——上游
  默认 `ON` 会给 Linux 传播 `-march=native`、在 Windows 探测 `/arch:AVX…`，
  制品绑定生产机 ISA（v6.2.2604 首版 linux 制品内含 AVX-512，在无 AVX-512
  的消费机加载即 SIGILL）；预编译 SDK 一律按上游 generic 基线出包。
- 每个归档内的 `panta-sdk.json` 记录目标平台、源码固定版本、构建开关、依赖与
  许可证入口（机器可读来源信息）。
- macOS arm64 制品以 macOS 13.0 为最低部署版本；Qt 与其他运行时依赖也须满足此基线。

## 覆盖重产记录

本仓库消费原 Panta 仓库已经发布的 SDK，不运行生产或覆盖重传 workflow。归档身份以 `native/cmake/sdk-provision.cmake` 锁定的 URL 和 SHA256 为准；升级制品时同步消费清单与集成验证记录。

## 许可证

- OCCT 按 **LGPL-2.1（附 OCCT 例外）** 再分发：`share/licenses/OCCT/`
  （`LICENSE_LGPL_21.txt`、`OCCT_LGPL_EXCEPTION.txt`）。
- Netgen 按 **LGPL-2.1** 再分发：`share/licenses/Netgen/LICENSE`。
- 两者均为动态链接共享库；源码可从上方仓库按对应标签获取。

## 校验

每个平台资产附带同名 `.sha256`（SHA256）。使用与供给规则见
[依赖获取规范](https://github.com/Yuki-Nagori/panta/blob/main/ai-docs/standards/dependency-acquisition.md) 与
[native 依赖供给模块](https://github.com/Yuki-Nagori/panta/blob/main/ai-docs/modules/native-dependency-supply.md)。
