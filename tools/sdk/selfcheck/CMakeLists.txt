# SDK 制品自检（原 Panta 任务 038，vtk-test 任务 041 迁入；031 供给同规则）：以真实 configure 消费安装树，
# find_package(CONFIG) 必须可用、关键 imported target/模块、许可证与元数据
# 齐备；任一缺失立即失败——制品不完整就不发布。
#
# 用法（project 模式；VTK WebGPU 硬件窗口制品不依赖 Qt staging）：
#   cmake -S tools/sdk/selfcheck -B <build> -DPSDK_ROOT=<安装树>
#     -DPSDK_PACKAGE=VTK
#     -DPSDK_REQUIRED_TARGETS="VTK::RenderingWebGPU VTK::RenderingUI"
# 注：不能用 cmake -P 脚本模式——VTK 的 config 会执行 add_library(IMPORTED)，
# 脚本模式不可脚本化该命令，无法真实验证导入。

cmake_minimum_required(VERSION 3.22)
# VTK 的 config 会执行 add_library(IMPORTED) 并调用 FindThreads 等
# 编译探测模块：自检工程必须启用 CXX（运行环境本来就有编译器）。
project(panta_sdk_selfcheck LANGUAGES CXX)

if(NOT DEFINED PSDK_ROOT OR NOT DEFINED PSDK_PACKAGE)
  message(FATAL_ERROR "selfcheck 需要 -DPSDK_ROOT 与 -DPSDK_PACKAGE")
endif()

foreach(_required IN ITEMS panta-sdk.json share/licenses)
  if(NOT EXISTS "${PSDK_ROOT}/${_required}")
    message(FATAL_ERROR "${PSDK_PACKAGE} 制品缺少 ${_required}：${PSDK_ROOT}/${_required}")
  endif()
endforeach()

# VTK 的可搬迁安装会在自身 config 中按普通 CMake 前缀查找 Dawn；把当前
# 制品根加入依赖搜索路径，确保 selfcheck 验证的是归档内的 Dawn，而不是
# runner 上偶然存在的系统/构建树 package。
list(PREPEND CMAKE_PREFIX_PATH "${PSDK_ROOT}")
find_package(${PSDK_PACKAGE} CONFIG REQUIRED PATHS "${PSDK_ROOT}" NO_DEFAULT_PATH)
message(STATUS "selfcheck：${PSDK_PACKAGE} 配置入口可用（prefix=${PSDK_ROOT}）")

if(DEFINED PSDK_REQUIRED_TARGETS)
  separate_arguments(_targets UNIX_COMMAND "${PSDK_REQUIRED_TARGETS}")
  foreach(_target IN LISTS _targets)
    if(NOT TARGET "${_target}")
      message(FATAL_ERROR "${PSDK_PACKAGE} 制品缺少 imported target ${_target}；" "模块集与构建描述不符")
    endif()
  endforeach()
  message(STATUS "selfcheck：必需 target 齐备（${PSDK_REQUIRED_TARGETS}）")
endif()

message(STATUS "selfcheck：${PSDK_ROOT} 通过")
