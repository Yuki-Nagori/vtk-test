# Native SDK 制品生产 superbuild（原 Panta 任务 038，vtk-test 任务 041 迁入）。
#
# 这是可审计构建描述的载体：以 ExternalProject 固定源码 commit、冻结配置
# 开关与依赖版本，产出统一布局的预编译 SDK（modules/native-dependency-supply.md）。
# 只在受信 CI/维护者环境中运行；不进入普通开发构建图（cargo build 不触达
# 本目录），开发者经 031 的 sdk-provision 消费已发布的归档。
#
# 入口约定：
#   cmake -S tools/sdk -B <build> -G Ninja
#   cmake --build <build> [--target <name>_sdk]
# VTK 制品只生产 WebGPU/平台硬件窗口和所需核心模块，不再消费 Qt 或
# GUISupportQtQuick；OCCT/Netgen 描述同样不消费 Qt（GUI/Visualization 全关）。
# 用 -DPANTA_SDK_ENABLE_VTK=OFF 跳过 VTK 描述（sdk-occt-netgen.yml 传此开关）。
# 产物：<build>/out/<name>/<version>/<triple>/（安装树）；
# 自检见 selfcheck/（真实 configure 消费工程），打包见 package.cmake。

cmake_minimum_required(VERSION 3.22)
project(panta_sdk_production NONE)

# 空的 GIT_SUBMODULES 明确表示不初始化 Dawn 的递归 submodule；依赖由
# DAWN_FETCH_DEPENDENCIES 的选择性脚本处理。CMP0097=NEW 才保留该语义。
if(POLICY CMP0097)
  cmake_policy(SET CMP0097 NEW)
endif()

# triple 映射与 031 消费侧同源，避免生产/供给两侧命名漂移。
include(${CMAKE_CURRENT_SOURCE_DIR}/../../native/cmake/sdk-provision.cmake)
panta_sdk_triple(PANTA_SDK_TRIPLE)

set(PANTA_SDK_MACOS_DEPLOYMENT_TARGET "")
set(PANTA_SDK_MACOS_DEPLOYMENT_TARGET_JSON null)
set(PANTA_SDK_PLATFORM_CMAKE_ARGS)
if(PANTA_SDK_TRIPLE STREQUAL "macos-arm64")
  set(PANTA_SDK_MACOS_DEPLOYMENT_TARGET "13.0")
  set(PANTA_SDK_MACOS_DEPLOYMENT_TARGET_JSON "\"13.0\"")
  list(APPEND PANTA_SDK_PLATFORM_CMAKE_ARGS
       "-DCMAKE_OSX_DEPLOYMENT_TARGET=${PANTA_SDK_MACOS_DEPLOYMENT_TARGET}")
endif()

set(PANTA_SDK_OUT_ROOT
    "${CMAKE_BINARY_DIR}/out"
    CACHE PATH "SDK 安装树根")
message(STATUS "panta SDK 生产：triple=${PANTA_SDK_TRIPLE} out=${PANTA_SDK_OUT_ROOT}")

# 生产顺序：occt → netgen（依赖自产 OCCT，同 triple 同 ABI）；vtk 独立。
# 各制品 workflow 显式关闭不属于自身的 ExternalProject，避免 VTK 生产
# 重新编译 OCCT/Netgen，或 OCCT/Netgen 生产触达 VTK/Dawn。
option(PANTA_SDK_ENABLE_OCCT "Configure the OpenCASCADE SDK description" ON)
option(PANTA_SDK_ENABLE_NETGEN "Configure the Netgen SDK description" ON)
option(PANTA_SDK_ENABLE_VTK "Configure the VTK WebGPU hardware-window SDK description" ON)
if(PANTA_SDK_ENABLE_OCCT)
  include(${CMAKE_CURRENT_SOURCE_DIR}/occt.cmake)
endif()
if(PANTA_SDK_ENABLE_NETGEN)
  if(NOT PANTA_SDK_ENABLE_OCCT)
    message(FATAL_ERROR "PANTA_SDK_ENABLE_NETGEN requires PANTA_SDK_ENABLE_OCCT=ON")
  endif()
  include(${CMAKE_CURRENT_SOURCE_DIR}/netgen.cmake)
endif()
if(PANTA_SDK_ENABLE_VTK)
  include(${CMAKE_CURRENT_SOURCE_DIR}/vtk.cmake)
endif()
