# Build.SdkProvision 的 consumer 工程（任务 031）：以真实 configure 驱动
# sdk-provision.cmake。fixture 模式用 -D 传入资产参数登记 fixture 条目；
# production 模式直接请求生产 manifest 里的名字，验证缺资产诊断。
# 启用 CXX：生产 SDK（VTK 等）的 config 会执行 add_library(IMPORTED) 与
# FindThreads 等编译探测，NONE 语言会在那一步失败。结果经 RESULT_FILE
# 回传给驱动脚本。
cmake_minimum_required(VERSION 3.22)
project(panta_sdk_consumer LANGUAGES CXX)

if(NOT DEFINED PANTA_SDK_MODULE)
  message(FATAL_ERROR "consumer 需要 -DPANTA_SDK_MODULE 指向 sdk-provision.cmake")
endif()
include("${PANTA_SDK_MODULE}")

if(CONSUMER_KIND STREQUAL "production")
  panta_require_sdk("${SDK_NAME}" _staging)
  file(WRITE "${RESULT_FILE}" "staging=${_staging}\n")
  return()
endif()

# missing-asset：只登记版本、不登记任何平台资产，验证"已登记名但本平台
# 无资产"的 038 指向诊断——不依赖生产 manifest 的实际登记状态，不触网。
if(CONSUMER_KIND STREQUAL "missing-asset")
  panta_sdk_declare_version("${SDK_NAME}" "${SDK_VERSION}")
  panta_require_sdk("${SDK_NAME}" _staging)
  return()
endif()

# fixture 资产参数来自 -D；SDK_NAME/SDK_VERSION/ASSET_URL/ASSET_SHA/PACKAGE
# 必填，INNER_ARCHIVE/INNER_SHA256/CHECK_TARGET 可空。值不含空格（驱动脚本
# 以无引号 ${ARGN} 传参）。
panta_sdk_triple(_host_triple)
panta_sdk_declare_version("${SDK_NAME}" "${SDK_VERSION}")
set(_inner_args)
if(INNER_ARCHIVE)
  list(APPEND _inner_args INNER_ARCHIVE "${INNER_ARCHIVE}" INNER_SHA256 "${INNER_SHA256}")
endif()
panta_sdk_declare_asset(
  "${SDK_NAME}"
  "${_host_triple}"
  URL
  "${ASSET_URL}"
  SHA256
  "${ASSET_SHA}"
  PACKAGE
  "${PACKAGE}"
  ${_inner_args}
  REQUIRED_TARGETS
  "${CHECK_TARGET}")
panta_require_sdk("${SDK_NAME}" _staging)

set(_target_found 0)
if(TARGET "${CHECK_TARGET}")
  set(_target_found 1)
endif()
file(WRITE "${RESULT_FILE}"
     "staging=${_staging}\nhost_triple=${_host_triple}\ntarget_found=${_target_found}\n")
