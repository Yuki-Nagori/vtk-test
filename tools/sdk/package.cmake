# SDK 制品打包（原 Panta 任务 038，vtk-test 任务 041 迁入）：把安装树打成带 triple 的归档并生成 SHA256；
# 发布、SBOM 与 provenance 由后续增量（受信 CI/发布存储决策）补齐。
#
# 用法：cmake -DPKG_ROOT=<安装树> -DPKG_NAME=vtk -DPKG_VERSION=9.7.0
#       -DPKG_TRIPLE=macos-arm64 -DPKG_DIST=<输出目录> -P package.cmake
# 产物：<PKG_DIST>/<name>-<version>-<triple>.tar.gz 与 .sha256。
#
# 归档内容为平铺布局（include/、lib/ 等直接位于归档根）：031 供给解包后
# 配置文件前缀即安装树本身，无需包装目录剥离。

if(NOT DEFINED PKG_ROOT OR NOT DEFINED PKG_DIST)
  message(FATAL_ERROR "package 需要 -DPKG_ROOT 与 -DPKG_DIST")
endif()
foreach(_key PKG_NAME PKG_VERSION PKG_TRIPLE)
  if(NOT DEFINED ${_key})
    message(FATAL_ERROR "package 需要 -D${_key}")
  endif()
endforeach()

file(MAKE_DIRECTORY "${PKG_DIST}")
set(_archive "${PKG_DIST}/${PKG_NAME}-${PKG_VERSION}-${PKG_TRIPLE}.tar.gz")
execute_process(
  # "." 打包含 ./include 等条目；解包后即平铺安装树。
  COMMAND "${CMAKE_COMMAND}" -E tar czf "${_archive}" .
  WORKING_DIRECTORY "${PKG_ROOT}"
  RESULT_VARIABLE _result)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "打包失败（退出码 ${_result}）：${_archive}")
endif()
file(SHA256 "${_archive}" _sha)
file(WRITE "${PKG_DIST}/${PKG_NAME}-${PKG_VERSION}-${PKG_TRIPLE}.sha256" "${_sha}\n")
message(STATUS "package：${_archive}（SHA256 ${_sha}）")
