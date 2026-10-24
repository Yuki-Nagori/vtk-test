vtk-test 受信 CI 按[冻结的构建描述](https://github.com/Yuki-Nagori/vtk-test/blob/main/tools/sdk/vtk.cmake)生产预编译 VTK SDK（生产管线自原 Panta 迁入，任务 041）。
本仓库通过 `sdk-provision` 的固定 URL / SHA256 消费制品，普通开发与 CI 不编译 VTK 源码。SDK 提供 `RenderingWebGPU`、视口标注模块 `RenderingAnnotation`（`vtkScalarBarActor` 等）与平台原生窗口（Linux Wayland、macOS Cocoa、Windows Win32）。

## 源码与可复现性

- CI 源码与依赖均来自 GitHub：<https://github.com/Kitware/VTK.git>、
  <https://github.com/google/dawn.git>
- 标签 `v9.7.0`（附注标签对象 `a78e2d950b5bf3aca6d78cc24b3153fd94cdaeec`）
  解引用 commit **`23f0a095621e91bbdbeace8451e22b950c8e5f46`**（"Update
  version number to 9.7.0"）；源码保持上游原样，未作修改。
- 构建配置：Release、C++20、共享库、`VTK_GROUP_ENABLE_Qt=NO`；通过
  `CMAKE_PROJECT_VTK_INCLUDE` 将 `RenderingWebGPU` 的上游 `cxx_std_17`
  目标要求提升为 C++20；Dawn 按上游源码标签构建并显式开启
  `DAWN_ENABLE_RTTI=ON`，不在 VTK 侧伪造 RTTI 或 ABI 符号；
  `VTK_GROUP_ENABLE_Rendering/StandAlone/Imaging/Parallel/Views/Web=DONT_WANT`，
  只直接启用 `RenderingUI`、`RenderingWebGPU` 与 `RenderingAnnotation` 及其传递依赖
  （`RenderingAnnotation` 相对上一版仅新增 `ImagingColor`）；
  `VTK_ENABLE_WRAPPING=OFF`，不打包 Python/Java 等包装层；
  `VTK_ENABLE_WEBGPU=ON`、`VTK_MODULE_ENABLE_VTK_RenderingUI=YES`、
  `VTK_MODULE_ENABLE_VTK_RenderingWebGPU=YES`、
  `VTK_MODULE_ENABLE_VTK_RenderingAnnotation=YES`、
  `VTK_RELOCATABLE_INSTALL=ON`；Linux 选择原生 Wayland（`VTK_USE_X=OFF`、
  `VTK_USE_Wayland=ON`），macOS 使用 `vtkCocoaHardwareWindow` 的 Metal
  layer/view，Windows 使用 `vtkWin32HardwareWindow`；Dawn source tag 为
  `v20260421.125655`（commit `b073946efbf0de690e2aeec16ef0d5c68362c951`，
  VTK 9.7.0 上游 WebGPU 文档指定），并随 VTK SDK 一起提供其头文件、CMake
  包配置和原生动态库；Linux 额外随 VTK CMake 包提供上游遗漏的
  `FindWAYLAND.cmake` 与 `FindXKBCOMMON.cmake`，保证安装树可独立被消费。
- 每个归档内的 `panta-sdk.json` 记录 triple、源码 pin、构建开关与许可证
  入口（机器可读来源信息）。
- macOS arm64 制品以 macOS 13.0 为最低部署版本；Qt 与其他运行时依赖也须满足此基线。

## 覆盖重产记录

本 Release 维持单一标签 `sdk-vtk-9.7.0-webgpu`：`sdk-vtk.yml` 重产时，只有三平台矩阵全部通过后才清空旧资产并全量重传；归档字节随重产变化，消费侧以 `native/cmake/sdk-provision.cmake` 登记的 SHA256 为准，登记更新与集成验证同任务完成。

- 2026-10-24：任务 041 在 vtk-test 首次生产，相对原 Panta 同名 Release 新增 `RenderingAnnotation`。

## 许可证

- VTK 按 **BSD-3-Clause** 再分发；许可证文件随每个归档的
  `share/licenses/VTK/Copyright.txt` 提供（VTK 源码树内嵌的第三方组件
  许可同样以其 Copyright.txt 为准）。
- 本制品不链接 Qt；Qt Quick 应用侧的 Qt 供给与本制品独立。
- Dawn 按其上游许可证随制品提供；Dawn 上游仓库、标签、commit 和 RTTI 构建选项
  写入 `panta-sdk.json`，原生动态库与 VTK 安装树共用 `lib` 目录。

## 校验

每个平台资产附带同名 `.sha256`（SHA256）。使用与供给规则见
[依赖获取规范](https://github.com/Yuki-Nagori/vtk-test/blob/main/ai-docs/standards/dependency-acquisition.md) 与
[native 依赖供给模块](https://github.com/Yuki-Nagori/vtk-test/blob/main/ai-docs/modules/native-dependency-supply.md)。
