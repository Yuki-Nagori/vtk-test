# VTK 9.7.0 制品构建描述（原 Panta 任务 038，vtk-test 任务 041 迁入）。
#
# 源码固定：tag v9.7.0 → commit 23f0a095621e91bbdbeace8451e22b950c8e5f46
# （2026-09-17 git ls-remote 解引用复核；annotated tag 对象 a78e2d95…）。
# CI 从 GitHub mirror 抓取；该 mirror 的 v9.7.0^{} 与上游 pin 一致，避免
# runner 到 VTK 上游服务的源码 clone 受网络策略影响。
# 配置开关冻结为 007 视口所需最小面：VTK WebGPU + 平台 hardware window
# （Linux 为 Wayland、macOS 为 Cocoa、Windows 为 Win32）；不启用
# Qt/GUISupportQtQuick，不把 Qt OpenGL scenegraph 集成带入制品。VTK 的
# 默认 group 会带入大量与 WebGPU 无关的 OpenGL/Views/Parallel 模块，下面
# 显式关闭它们，再直接打开 RenderingUI/RenderingWebGPU。RenderingAnnotation
# 为视口内色标 vtkScalarBarActor 打开（vtk-test 任务 041 / 038；新增传递依赖
# 仅 ImagingColor，其余已随 WebGPU 进入制品）。SDK 不提供
# Python/Java 等包装层，因此同时关闭 VTK wrapping。
# 许可证随源码树收集。

set(PANTA_VTK_VERSION 9.7.0)
set(PANTA_VTK_COMMIT 23f0a095621e91bbdbeace8451e22b950c8e5f46)
set(PANTA_VTK_INSTALL_DIR "${PANTA_SDK_OUT_ROOT}/vtk/${PANTA_VTK_VERSION}/${PANTA_SDK_TRIPLE}")
set(PANTA_VTK_CMAKE_DIR_RELATIVE "lib/cmake/vtk-9.7")
set(PANTA_VTK_CMAKE_DIR "${PANTA_VTK_INSTALL_DIR}/${PANTA_VTK_CMAKE_DIR_RELATIVE}")

# VTK 9.7 的 RenderingWebGPU 在桌面平台依赖 Dawn；只打开
# VTK_ENABLE_WEBGPU 而不提供 Dawn_DIR 会在 configure 阶段失败。Dawn 按 VTK
# 官方指定的 GitHub source tag 构建，避免 native release 与 VTK 的 RTTI/ABI
# 选项不一致。
set(PANTA_DAWN_VERSION 20260421.125655)
set(PANTA_DAWN_TAG v20260421.125655)
set(PANTA_DAWN_COMMIT b073946efbf0de690e2aeec16ef0d5c68362c951)
set(PANTA_DAWN_LICENSE_URL
    "https://raw.githubusercontent.com/google/dawn/${PANTA_DAWN_COMMIT}/LICENSE")
set(PANTA_DAWN_INSTALL_DIR "${CMAKE_BINARY_DIR}/dawn/${PANTA_DAWN_VERSION}/${PANTA_SDK_TRIPLE}")
set(PANTA_DAWN_WINDOW_ARGS -DDAWN_USE_GLFW=OFF -DDAWN_USE_X11=OFF)

if(PANTA_SDK_TRIPLE STREQUAL "linux-x86_64")
  set(PANTA_VTK_WINDOW_SYSTEM Wayland)
  set(PANTA_VTK_HARDWARE_WINDOW vtkWaylandHardwareWindow)
  set(PANTA_VTK_WINDOW_ARGS -DVTK_USE_X=OFF -DVTK_USE_Wayland=ON)
  list(APPEND PANTA_DAWN_WINDOW_ARGS -DDAWN_USE_WAYLAND=ON)
elseif(PANTA_SDK_TRIPLE STREQUAL "macos-arm64")
  set(PANTA_VTK_WINDOW_SYSTEM Cocoa)
  set(PANTA_VTK_HARDWARE_WINDOW vtkCocoaHardwareWindow)
  set(PANTA_VTK_WINDOW_ARGS -DVTK_USE_COCOA=ON)
  list(APPEND PANTA_DAWN_WINDOW_ARGS -DDAWN_USE_WAYLAND=OFF)
elseif(PANTA_SDK_TRIPLE STREQUAL "windows-x86_64")
  set(PANTA_VTK_WINDOW_SYSTEM Win32)
  set(PANTA_VTK_HARDWARE_WINDOW vtkWin32HardwareWindow)
  set(PANTA_VTK_WINDOW_ARGS -DVTK_USE_WIN32=ON)
  list(APPEND PANTA_DAWN_WINDOW_ARGS -DDAWN_USE_WAYLAND=OFF)
else()
  message(FATAL_ERROR "VTK WebGPU 制品没有登记窗口系统：${PANTA_SDK_TRIPLE}")
endif()

include(ExternalProject)
ExternalProject_Add(
  dawn_sdk
  GIT_REPOSITORY https://github.com/google/dawn.git
  GIT_TAG ${PANTA_DAWN_TAG}
  GIT_SHALLOW TRUE
  # Dawn 的默认 git update 会递归 checkout ANGLE、SwiftShader、LLVM 等
  # 与 webgpu_dawn 无关的整棵依赖树；其中包含不可匿名访问的
  # chrome-internal.googlesource.com URL，并会在 Windows 触发路径长度限制。
  # 只保留 Dawn 源码，依赖由其受控的 fetch_dawn_dependencies.py 按需取得。
  GIT_SUBMODULES "" GIT_SUBMODULES_RECURSE FALSE
  GIT_PROGRESS TRUE
  CMAKE_ARGS -DCMAKE_BUILD_TYPE=Release
             -DCMAKE_INSTALL_PREFIX=${PANTA_DAWN_INSTALL_DIR}
             ${PANTA_SDK_PLATFORM_CMAKE_ARGS}
             ${PANTA_DAWN_WINDOW_ARGS}
             -DDAWN_ENABLE_INSTALL=ON
             -DDAWN_ENABLE_RTTI=ON
             -DDAWN_ENABLE_PIC=ON
             -DDAWN_FETCH_DEPENDENCIES=ON
             -DDAWN_BUILD_SAMPLES=OFF
             -DDAWN_BUILD_TESTS=OFF
             -DDAWN_BUILD_BENCHMARKS=OFF
             -DDAWN_BUILD_FUZZERS=OFF
             -DDAWN_BUILD_NODE_BINDINGS=OFF
             -DDAWN_ENABLE_SWIFTSHADER=OFF
             -DDAWN_BUILD_PROTOBUF=OFF
             -DTINT_BUILD_IR_BINARY=OFF
             -DTINT_BUILD_CMD_TOOLS=OFF
             -DTINT_BUILD_TESTS=OFF
  BUILD_COMMAND ${CMAKE_COMMAND} --build . --config Release --target webgpu_dawn
  INSTALL_COMMAND ${CMAKE_COMMAND} --install . --config Release
  USES_TERMINAL_DOWNLOAD TRUE
  USES_TERMINAL_BUILD TRUE
  USES_TERMINAL_INSTALL TRUE)

ExternalProject_Add_Step(
  dawn_sdk license
  COMMAND ${CMAKE_COMMAND} -E copy_if_different <SOURCE_DIR>/LICENSE
          ${PANTA_DAWN_INSTALL_DIR}/LICENSE
  DEPENDEES install
  USES_TERMINAL)

ExternalProject_Add(
  vtk_sdk
  GIT_REPOSITORY https://github.com/Kitware/VTK.git
  GIT_TAG ${PANTA_VTK_COMMIT}
  GIT_SHALLOW TRUE
  GIT_PROGRESS TRUE
  # 子工程继承外层生成器（macOS/Linux 为 Ninja；Windows CI 传 Visual
  # Studio 17 2022 + x64，Ninja 子构建在无 vcvars 的 runner 上找不到 cl）。
  # 多配置生成器的构建/安装显式选 Release，单配置忽略 --config。
  BUILD_COMMAND ${CMAKE_COMMAND} --build . --config Release
  CMAKE_ARGS -DCMAKE_BUILD_TYPE=Release
             -DCMAKE_CXX_STANDARD=20
             -DCMAKE_CXX_STANDARD_REQUIRED=ON
             -DCMAKE_CXX_EXTENSIONS=OFF
             -DCMAKE_PROJECT_VTK_INCLUDE=${CMAKE_CURRENT_SOURCE_DIR}/vtk-cxx20.cmake
             -DBUILD_SHARED_LIBS=ON
             -DBUILD_TESTING=OFF
             -DVTK_ENABLE_WRAPPING=OFF
             -DCMAKE_INSTALL_PREFIX=${PANTA_VTK_INSTALL_DIR}
             ${PANTA_SDK_PLATFORM_CMAKE_ARGS}
             -DDawn_DIR=${PANTA_DAWN_INSTALL_DIR}/lib/cmake/Dawn
             -DVTK_RELOCATABLE_INSTALL=ON
             -DVTK_GROUP_ENABLE_Qt=NO
             -DVTK_GROUP_ENABLE_Rendering=DONT_WANT
             -DVTK_GROUP_ENABLE_StandAlone=DONT_WANT
             -DVTK_GROUP_ENABLE_Imaging=DONT_WANT
             -DVTK_GROUP_ENABLE_Parallel=DONT_WANT
             -DVTK_GROUP_ENABLE_Views=DONT_WANT
             -DVTK_GROUP_ENABLE_Web=DONT_WANT
             -DVTK_ENABLE_WEBGPU=ON
             -DVTK_MODULE_ENABLE_VTK_RenderingUI=YES
             -DVTK_MODULE_ENABLE_VTK_RenderingWebGPU=YES
             -DVTK_MODULE_ENABLE_VTK_RenderingAnnotation=YES
             ${PANTA_VTK_WINDOW_ARGS}
  DEPENDS dawn_sdk
  # 安装后补齐统一布局的非构建产物：许可证与 SDK 元数据。
  INSTALL_COMMAND ${CMAKE_COMMAND} --install . --config Release
  COMMAND ${CMAKE_COMMAND} -E make_directory ${PANTA_VTK_INSTALL_DIR}/share/licenses/VTK
  COMMAND ${CMAKE_COMMAND} -E copy_if_different <SOURCE_DIR>/Copyright.txt
          ${PANTA_VTK_INSTALL_DIR}/share/licenses/VTK/Copyright.txt
  USES_TERMINAL_DOWNLOAD TRUE
  USES_TERMINAL_BUILD TRUE)

# 把 Dawn 的头文件、CMake package 和 native library 放进同一 VTK SDK，
# 避免生成只在 CI 能 configure、消费侧却缺 Dawn 依赖的半成品。
ExternalProject_Add_Step(
  vtk_sdk dawn_runtime
  COMMAND
    ${CMAKE_COMMAND} -DPANTA_DAWN_INSTALL_DIR=${PANTA_DAWN_INSTALL_DIR}
    -DPANTA_VTK_INSTALL_DIR=${PANTA_VTK_INSTALL_DIR} -P
    ${CMAKE_CURRENT_SOURCE_DIR}/install-dawn.cmake
  DEPENDEES install
  USES_TERMINAL)

if(PANTA_SDK_TRIPLE STREQUAL "linux-x86_64")
  # VTK 的 Wayland 依赖在构建时由源码树里的 Find 模块解析，但上游的
  # vtkInstallCMakePackage.cmake 没有把这两个模块安装进导出包。复制到 VTK
  # 自己的 CMake module path，保证 Linux 消费者不需要 VTK 源码树或系统包提供
  # 同名 Find module。
  ExternalProject_Add_Step(
    vtk_sdk wayland_cmake_modules
    COMMAND ${CMAKE_COMMAND} -E copy_if_different <SOURCE_DIR>/CMake/FindWAYLAND.cmake
            ${PANTA_VTK_CMAKE_DIR}/FindWAYLAND.cmake
    COMMAND ${CMAKE_COMMAND} -E copy_if_different <SOURCE_DIR>/CMake/FindXKBCOMMON.cmake
            ${PANTA_VTK_CMAKE_DIR}/FindXKBCOMMON.cmake
    DEPENDEES install
    DEPENDERS dawn_runtime
    USES_TERMINAL)
endif()

# panta-sdk.json：031 供给自检与 012 缓存键的机器可读依据。
file(
  WRITE "${CMAKE_BINARY_DIR}/vtk-panta-sdk.json.in"
  [=[
{
  "name": "vtk",
  "version": "@PANTA_VTK_VERSION@",
  "triple": "@PANTA_SDK_TRIPLE@",
  "source": {
    "repository": "https://github.com/Kitware/VTK.git",
    "tag": "v9.7.0",
    "commit": "@PANTA_VTK_COMMIT@"
  },
  "build_type": "Release",
  "macos_deployment_target": @PANTA_SDK_MACOS_DEPLOYMENT_TARGET_JSON@,
  "cxx_standard": 20,
  "shared": true,
  "relocatable_install": true,
  "graphics_backend": "WebGPU",
  "window_system": "@PANTA_VTK_WINDOW_SYSTEM@",
  "hardware_window": "@PANTA_VTK_HARDWARE_WINDOW@",
  "dawn": {
    "version": "@PANTA_DAWN_VERSION@",
    "tag": "@PANTA_DAWN_TAG@",
    "commit": "@PANTA_DAWN_COMMIT@",
    "repository": "https://github.com/google/dawn.git",
    "rtti": true,
    "license_url": "@PANTA_DAWN_LICENSE_URL@",
    "license_path": "share/licenses/Dawn/LICENSE"
  },
  "modules_highlights": ["RenderingWebGPU", "RenderingUI", "RenderingCore", "RenderingAnnotation"],
  "cmake_package": ["@PANTA_VTK_CMAKE_DIR_RELATIVE@", "vtk-config.cmake"],
  "generator": "@CMAKE_GENERATOR@",
  "license_files": ["share/licenses/VTK/Copyright.txt", "share/licenses/Dawn/LICENSE"],
  "license": "share/licenses/VTK/Copyright.txt (BSD-3)"
}
]=])
configure_file("${CMAKE_BINARY_DIR}/vtk-panta-sdk.json.in"
               "${CMAKE_BINARY_DIR}/vtk-panta-sdk-configure.json" @ONLY)
ExternalProject_Add_Step(
  vtk_sdk metadata
  COMMAND ${CMAKE_COMMAND} -E copy_if_different ${CMAKE_BINARY_DIR}/vtk-panta-sdk-configure.json
          ${PANTA_VTK_INSTALL_DIR}/panta-sdk.json
  DEPENDEES install dawn_runtime)
