# VTK/OCCT/Netgen/GoogleTest 预编译 SDK 供给（任务 031/071）。
#
# 托管原则（standards/dependency-acquisition.md、modules/native-dependency-supply.md）：
# - 只消费 manifest 中 URL + SHA256 固定的预编译 SDK；本平台缺资产时立即
#   失败并给出修复动作，不退回系统库、Python wheel 或本地源码构建。
# - staging 布局 `<PANTA_SDK_PROVISION_DIR>/<name>/<version>/<triple>/`，由
#   临时目录解包后原子 rename 发布；不同版本各自成目录，互不覆盖。归档
#   仅服务本次下载校验与解包，发布即删；复用只看 marker，损坏走受限重下载。
# - 消费统一走 `find_package(... CONFIG ... PATHS staging NO_DEFAULT_PATH)`，
#   平台库名不散落到适配器。
# - Cargo 入口由 build.rs 注入共享根 `target/panta-deps/sdk`（任务 041，
#   跨 profile 复用）；presets 直接 configure 时默认构建树内（与 Qt 供给同型）。
#
# 消费方（007/009/010）落地前生产构建不触发任何 SDK 下载；本模块逻辑由
# ctest `Build.SdkProvision` 用 fixture SDK 驱动验证（cmake/tests/）。

if(NOT DEFINED PANTA_SDK_PROVISION_DIR)
  set(PANTA_SDK_PROVISION_DIR "${CMAKE_BINARY_DIR}/sdk")
endif()

# 计算 host triple：windows-x86_64 / macos-arm64 / macos-x86_64 /
# linux-x86_64 / linux-aarch64。manifest 条目按该命名登记；同 profile
# 并发构建本就不受支持（任务 041），staging 无须加锁。
function(panta_sdk_triple out_var)
  string(TOLOWER "${CMAKE_SYSTEM_PROCESSOR}" _proc)
  if(_proc MATCHES "^(amd64|x86_64)$")
    set(_arch x86_64)
  elseif(_proc MATCHES "^(arm64|aarch64)$")
    if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
      set(_arch aarch64)
    else()
      set(_arch arm64)
    endif()
  else()
    message(FATAL_ERROR "panta_sdk_triple：未知架构 ${CMAKE_SYSTEM_PROCESSOR}"
                        "（${CMAKE_SYSTEM_NAME}）；请在 native/cmake/sdk-provision.cmake 扩展 triple 映射")
  endif()
  if(CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    set(_os macos)
  elseif(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(_os linux)
  elseif(CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(_os windows)
  else()
    message(FATAL_ERROR "panta_sdk_triple：不支持的平台 ${CMAKE_SYSTEM_NAME}；"
                        "记录到 dependency-acquisition.md 后再扩展供给")
  endif()
  set(${out_var}
      "${_os}-${_arch}"
      PARENT_SCOPE)
endfunction()

# 登记 SDK 的固定版本（升级 = 改本节并同一 commit 完成集成验证）。
# 版本独立于资产登记：没有资产的平台也要能报出期望版本。
function(panta_sdk_declare_version name version)
  set_property(GLOBAL PROPERTY "panta_sdk_version_${name}" "${version}")
  get_property(_names GLOBAL PROPERTY panta_sdk_names)
  if(NOT name IN_LIST _names)
    list(APPEND _names "${name}")
    set_property(GLOBAL PROPERTY panta_sdk_names "${_names}")
  endif()
endfunction()

# 登记一个平台的预编译资产。URL/SHA256 必须来自下载实测，并同步
# ai-docs/standards/dependency-acquisition.md；升级换 URL 时同步换哈希。
# INNER_ARCHIVE/INNER_SHA256 支持官方归档内嵌真正 SDK 包的形态（如 OCCT
# Windows 外层 zip 套内层 zip）。COMPONENTS/REQUIRED_TARGETS 是 find_package
# 与供给自检的约束：缺组件或 imported target 立即失败。
function(panta_sdk_declare_asset name triple)
  cmake_parse_arguments(
    PARSE_ARGV 2 PASSED "" "URL;SHA256;PACKAGE;ABI;MODULES;LICENSE;INNER_ARCHIVE;INNER_SHA256"
    "COMPONENTS;REQUIRED_TARGETS")
  if(NOT PASSED_URL
     OR NOT PASSED_SHA256
     OR NOT PASSED_PACKAGE)
    message(FATAL_ERROR "panta_sdk_declare_asset(${name} ${triple})：" "URL、SHA256、PACKAGE 均为必填")
  endif()
  # file:// 仅限 fixture 测试消费同一代码路径；生产资产必须是 https。
  if(NOT PASSED_URL MATCHES "^(https|file)://")
    message(FATAL_ERROR "panta_sdk_declare_asset(${name} ${triple})："
                        "URL 必须以 https://（生产）或 file://（测试）开头：${PASSED_URL}")
  endif()
  string(TOLOWER "${PASSED_SHA256}" PASSED_SHA256)
  string(LENGTH "${PASSED_SHA256}" _sha_length)
  if(NOT _sha_length EQUAL 64 OR NOT PASSED_SHA256 MATCHES "^[0-9a-f]+$")
    message(FATAL_ERROR "panta_sdk_declare_asset(${name} ${triple})："
                        "SHA256 必须是 64 位小写十六进制：${PASSED_SHA256}")
  endif()
  if(PASSED_INNER_ARCHIVE STREQUAL "" AND PASSED_INNER_SHA256 STREQUAL "")
    # 无内层归档，允许。
  elseif(PASSED_INNER_ARCHIVE STREQUAL "" OR PASSED_INNER_SHA256 STREQUAL "")
    message(FATAL_ERROR "panta_sdk_declare_asset(${name} ${triple})："
                        "INNER_ARCHIVE 与 INNER_SHA256 必须成对提供")
  endif()

  # 条目存为 key=value 列表（global property）；值内不得出现 list 分隔符，
  # 多值关键字以空格连接，消费侧 separate_arguments 还原。
  set(_entry)
  foreach(
    _key
    URL
    SHA256
    PACKAGE
    ABI
    MODULES
    LICENSE
    INNER_ARCHIVE
    INNER_SHA256)
    set(_value "${PASSED_${_key}}")
    if(NOT _value STREQUAL "")
      list(APPEND _entry "${_key}=${_value}")
    endif()
  endforeach()
  foreach(_key COMPONENTS REQUIRED_TARGETS)
    set(_joined "${PASSED_${_key}}")
    if(NOT _joined STREQUAL "")
      string(REPLACE ";" " " _joined "${_joined}")
      list(APPEND _entry "${_key}=${_joined}")
    endif()
  endforeach()
  set_property(GLOBAL PROPERTY "panta_sdk_asset_${name}_${triple}" "${_entry}")
  get_property(_triples GLOBAL PROPERTY "panta_sdk_asset_triples_${name}")
  if(NOT triple IN_LIST _triples)
    list(APPEND _triples "${triple}")
    set_property(GLOBAL PROPERTY "panta_sdk_asset_triples_${name}" "${_triples}")
  endif()
  get_property(_names GLOBAL PROPERTY panta_sdk_names)
  if(NOT name IN_LIST _names)
    list(APPEND _names "${name}")
    set_property(GLOBAL PROPERTY panta_sdk_names "${_names}")
  endif()
endfunction()

# 请求并供给一个 SDK：manifest 查条目 → 归档下载/校验 → 解包 → 原子发布
# staging → find_package(CONFIG) + imported target 自检。成功后
# `<staging_var>` 指向发布根目录；所有失败路径清场（删归档/临时目录），
# 不留下半可用构建树。
function(panta_require_sdk name staging_var)
  get_property(_names GLOBAL PROPERTY panta_sdk_names)
  if(NOT name IN_LIST _names)
    message(FATAL_ERROR "panta_require_sdk(${name})：未登记的 SDK 名；"
                        "已登记：${_names}。检查拼写或在 native/cmake/sdk-provision.cmake 登记。")
  endif()
  get_property(_version GLOBAL PROPERTY "panta_sdk_version_${name}")
  panta_sdk_triple(_triple)
  get_property(_entry GLOBAL PROPERTY "panta_sdk_asset_${name}_${_triple}")
  if(NOT _entry)
    get_property(_triples GLOBAL PROPERTY "panta_sdk_asset_triples_${name}")
    if(NOT _triples)
      set(_triples "（无任何平台）")
    endif()
    message(
      FATAL_ERROR
        "panta_require_sdk(${name})：manifest 没有本平台的预编译 SDK 资产，\n"
        "  固定版本：${_version}\n"
        "  平台 triple：${_triple}（${CMAKE_SYSTEM_NAME}/${CMAKE_SYSTEM_PROCESSOR}）\n"
        "  已登记资产的 triple：${_triples}\n"
        "  修复：查阅 ai-docs/modules/native-dependency-supply.md 获取可校验 Release 制品，\n"
        "  并在 native/cmake/sdk-provision.cmake 登记 URL/SHA256；不退回系统库、\n"
        "  Python wheel 或本地源码构建（standards/dependency-acquisition.md）。")
  endif()
  if(_version STREQUAL "")
    message(FATAL_ERROR "panta_require_sdk(${name})：缺少 panta_sdk_declare_version"
                        " 登记，staging 目录无法按版本隔离")
  endif()

  # 条目局部变量先置空：if() 对未定义变量不做空串展开（保留字面量），
  # 缺省键必须以定义过的空值参与后续判断。
  set(_url "")
  set(_sha256 "")
  set(_package "")
  set(_abi "")
  set(_modules "")
  set(_license "")
  set(_inner_archive "")
  set(_inner_sha256 "")
  set(_components "")
  set(_required_targets "")
  foreach(_pair IN LISTS _entry)
    string(REGEX REPLACE "^([^=]+)=(.*)$" "\\1" _k "${_pair}")
    string(REGEX REPLACE "^([^=]+)=(.*)$" "\\2" _v "${_pair}")
    string(TOLOWER "${_k}" _k)
    set(_${_k} "${_v}")
  endforeach()

  set(_root "${PANTA_SDK_PROVISION_DIR}/${name}")
  set(_staging "${_root}/${_version}/${_triple}")
  set(_marker "${_staging}/.panta-sdk-provisioned")
  set(_archive "${_root}/archives/${name}-${_version}-${_triple}.archive")

  # 缓存命中：marker 记录的归档哈希一致即复用（离线可用）；marker 缺失或
  # 不一致视为缓存损坏，清 staging 重建，不影响其他版本目录。
  set(_marker_prefix "<unset>")
  if(EXISTS "${_marker}")
    file(STRINGS "${_marker}" _marker_lines)
    foreach(_line IN LISTS _marker_lines)
      if(_line MATCHES "^sha256=(.+)$")
        set(_marker_sha "${CMAKE_MATCH_1}")
      elseif(_line MATCHES "^config_prefix=(.*)$")
        set(_marker_prefix "${CMAKE_MATCH_1}")
      endif()
    endforeach()
    if(NOT
       (DEFINED _marker_sha
        AND _marker_sha STREQUAL "${_sha256}"
        AND NOT _marker_prefix STREQUAL "<unset>"))
      file(REMOVE_RECURSE "${_staging}")
      message(STATUS "panta SDK ${name} ${_version}(${_triple}) staging 标记" " 缺失或不符，按缓存归档重建")
      set(_marker_prefix "<unset>")
    endif()
  endif()

  if(_marker_prefix STREQUAL "<unset>")
    file(MAKE_DIRECTORY "${_root}/archives")
    # 归档命中预期哈希则跳过下载（离线重建）；不符或残缺立即删除重下。
    if(EXISTS "${_archive}")
      file(SHA256 "${_archive}" _actual)
      if(NOT _actual STREQUAL "${_sha256}")
        file(REMOVE "${_archive}")
        message(STATUS "panta SDK ${name} 归档校验不符，已删除并重新下载")
      endif()
    endif()
    if(NOT EXISTS "${_archive}")
      message(STATUS "下载 ${name} ${_version}(${_triple}) SDK：${_url}")
      file(
        DOWNLOAD "${_url}" "${_archive}"
        INACTIVITY_TIMEOUT 120
        TIMEOUT 1800
        EXPECTED_HASH SHA256=${_sha256}
        STATUS _download_status)
      list(GET _download_status 0 _download_code)
      if(NOT _download_code EQUAL 0)
        file(REMOVE "${_archive}")
        list(GET _download_status 1 _download_message)
        message(
          FATAL_ERROR
            "下载 ${name} ${_version}(${_triple}) SDK 失败：${_download_message}\n" "  URL：${_url}\n"
            "  归档已删除；网络可用时重试，或按 manifest 手动放置归档到\n" "  ${_archive}（SHA256=${_sha256}）")
      endif()
    endif()

    # 解包走两段临时目录：内嵌归档形态先解外层（outer），SDK 内容统一落在
    # sdk 临时目录；发布只 rename sdk 目录，外层包装（含内层归档本体）不进
    # staging。任一步失败清场退出。
    set(_outer_tmp "${_root}/.tmp-${_version}-${_triple}-outer")
    set(_sdk_tmp "${_root}/.tmp-${_version}-${_triple}-sdk")
    file(REMOVE_RECURSE "${_outer_tmp}" "${_sdk_tmp}")
    if(_inner_archive STREQUAL "")
      file(MAKE_DIRECTORY "${_sdk_tmp}")
      _panta_extract_archive("${_archive}" "${_sdk_tmp}" "${name} ${_version}(${_triple})")
    else()
      file(MAKE_DIRECTORY "${_outer_tmp}")
      _panta_extract_archive("${_archive}" "${_outer_tmp}" "${name} ${_version}(${_triple}) 外层")
      set(_inner "${_outer_tmp}/${_inner_archive}")
      if(NOT EXISTS "${_inner}")
        file(REMOVE_RECURSE "${_outer_tmp}" "${_sdk_tmp}")
        message(FATAL_ERROR "${name} ${_version}(${_triple}) 外层归档缺少内层归档"
                            " ${_inner_archive}：上游布局可能变化，请核对 manifest 并重新实测")
      endif()
      file(SHA256 "${_inner}" _inner_actual)
      if(NOT _inner_actual STREQUAL "${_inner_sha256}")
        file(REMOVE_RECURSE "${_outer_tmp}" "${_sdk_tmp}")
        message(FATAL_ERROR "${name} ${_version}(${_triple}) 内层归档 SHA256 不符："
                            "预期 ${_inner_sha256}，实际 ${_inner_actual}；外层归档内容与 manifest" " 不一致，已清场")
      endif()
      file(MAKE_DIRECTORY "${_sdk_tmp}")
      _panta_extract_archive("${_inner}" "${_sdk_tmp}" "${name} ${_version}(${_triple}) 内层")
    endif()

    # 定位 CONFIG package：归档内必须恰好一个 <Package>Config.cmake /
    # <package>-config.cmake（文件名大小写不敏感；VTK 用 vtk-config.cmake）。
    # 多处命中即布局歧义，拒绝猜测。
    string(TOLOWER "${_package}" _package_lower)
    file(GLOB_RECURSE _config_hits "${_sdk_tmp}/*.cmake")
    set(_hit "")
    foreach(_candidate IN LISTS _config_hits)
      get_filename_component(_base "${_candidate}" NAME)
      string(TOLOWER "${_base}" _base_lower)
      if(_base_lower STREQUAL "${_package_lower}config.cmake" OR _base_lower STREQUAL
                                                                 "${_package_lower}-config.cmake")
        if(NOT _hit STREQUAL "")
          file(REMOVE_RECURSE "${_outer_tmp}" "${_sdk_tmp}")
          message(
            FATAL_ERROR "${name} ${_version}(${_triple}) 归档内多处命中"
                        " ${_package} 配置文件（${_hit} 与 ${_candidate}）：布局歧义，" "请核对 manifest 与制品内容")
        endif()
        set(_hit "${_candidate}")
      endif()
    endforeach()
    if(_hit STREQUAL "")
      file(REMOVE_RECURSE "${_outer_tmp}" "${_sdk_tmp}")
      message(
        FATAL_ERROR
          "${name} ${_version}(${_triple}) 解包完成但未找到"
          " ${_package}Config.cmake / ${_package}-config.cmake：归档布局与 manifest"
          " 不符，已清场；请核对制品（038）并更新登记")
    endif()

    # 从命中文件向上推导 find_package 前缀：剥掉尾部的 cmake|lib|<Package>*
    # 目录名后剩余的路径即前缀（覆盖 lib/cmake/<Pkg>/、cmake/ 与外层包装
    # 目录三种布局）；<Package>* 前缀匹配对当前登记的包名无正则特殊字符。
    file(RELATIVE_PATH _hit_rel "${_sdk_tmp}" "${_hit}")
    string(REGEX REPLACE "/[^/]*$" "" _dir "${_hit_rel}")
    set(_prefix_rel "")
    while(NOT _dir STREQUAL "")
      string(REGEX REPLACE ".*/" "" _leaf "${_dir}")
      string(TOLOWER "${_leaf}" _leaf_lower)
      if(_leaf_lower STREQUAL "cmake"
         OR _leaf_lower STREQUAL "lib"
         OR _leaf_lower MATCHES "^${_package_lower}")
        if(_dir MATCHES "/")
          string(REGEX REPLACE "/[^/]*$" "" _dir "${_dir}")
        else()
          set(_dir "")
        endif()
        set(_prefix_rel "${_dir}")
      else()
        break()
      endif()
    endwhile()

    # file(RENAME) 不创建缺失的父目录；版本目录先就位再原子发布。
    file(MAKE_DIRECTORY "${_root}/${_version}")
    file(REMOVE_RECURSE "${_outer_tmp}" "${_staging}")
    file(RENAME "${_sdk_tmp}" "${_staging}")
    file(WRITE "${_marker}" "sha256=${_sha256}\nconfig_prefix=${_prefix_rel}\n")
    # 归档只服务本次下载校验与解包；发布即删（复用仅看 marker，损坏走
    # 受限重下载），避免归档与解包树在缓存中长期双份。
    file(REMOVE "${_archive}")
    set(_marker_prefix "${_prefix_rel}")
  endif()

  # find_package 前缀：staging 根 + marker 记录的相对前缀（包装目录布局）。
  set(_prefix "${_staging}")
  if(NOT _marker_prefix STREQUAL "" AND NOT _marker_prefix STREQUAL "<unset>")
    string(APPEND _prefix "/${_marker_prefix}")
  endif()

  set(_find_args "")
  if(NOT _components STREQUAL "")
    separate_arguments(_components_list UNIX_COMMAND "${_components}")
    list(PREPEND _components_list COMPONENTS)
    set(_find_args "${_components_list}")
  endif()
  find_package(
    ${_package}
    CONFIG
    REQUIRED
    ${_find_args}
    PATHS
    "${_prefix}"
    NO_DEFAULT_PATH)

  separate_arguments(_required_target_list UNIX_COMMAND "${_required_targets}")
  foreach(_target IN LISTS _required_target_list)
    if(NOT TARGET "${_target}")
      message(
        FATAL_ERROR
          "${name} ${_version}(${_triple}) SDK 已供给但缺少"
          " imported target ${_target}：制品模块与 manifest REQUIRED_TARGETS 不符；"
          "检查 ${_staging} 的包内容或更新 manifest/制品（038）")
    endif()
  endforeach()

  message(STATUS "panta SDK ${name} ${_version}(${_triple}) 供给就绪：${_staging}")
  set(${staging_var}
      "${_staging}"
      PARENT_SCOPE)
endfunction()

# `cmake -E tar xf`（libarchive）解包：zip/tgz 等 三平台零额外工具。
function(_panta_extract_archive archive destination label)
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -E tar xf "${archive}"
    WORKING_DIRECTORY "${destination}"
    RESULT_VARIABLE _result)
  if(NOT _result EQUAL 0)
    file(REMOVE_RECURSE "${destination}")
    message(FATAL_ERROR "解包 ${label} 归档失败（退出码 ${_result}）：${archive}；" "归档可能损坏，删除缓存归档后重试将重新下载")
  endif()
endfunction()

# ── 固定 manifest（升级 = 修改本节并同一 commit 完成集成验证后回写
#    ai-docs/standards/dependency-acquisition.md；登记数据必须来自下载实测）──

panta_sdk_declare_version(vtk 9.7.0)
# VTK 9.7.0 WebGPU 制品（038 受信 CI 生产，Release sdk-vtk-9.7.0-webgpu，
# 2026-09-20 三平台 production/selfcheck/package success；2026-09-21 随任务
# 049 单一 tag 重产覆盖。2026-10-14 以 macOS 13.0 为部署目标再次重产，
# workflow run 38509650210 三平台自检、打包、发布全绿；下方 SHA 按 sidecar
# 与 Release API digest 实测回填。发布说明与合规元数据见
# tools/sdk/releases/vtk-9.7.0.md。源码 tag
# v9.7.0 → commit 23f0a095621e91bbdbeace8451e22b950c8e5f46（unmodified）；
# Release/共享库/C++20/WebGPU hardware-window，不链接 Qt；Dawn 与 VTK 同包，
# 三平台 REQUIRED_TARGETS 以真实归档内的 imported targets 为准。
# Linux 注意：ubuntu-24.04 gcc13 生产，有效 glibc 基线高于 Qt 的 RHEL9
# （≥2.34）；实际下限由 007 运行验证后回写。
panta_sdk_declare_asset(
  vtk
  macos-arm64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-vtk-9.7.0-webgpu/vtk-9.7.0-macos-arm64.tar.gz
  SHA256
  fd8cbab6bdf5db1d9ae919c73b4fb3c3e7b60d5ed369bfc4b01c6c5b8d03ac6e
  PACKAGE
  VTK
  REQUIRED_TARGETS
  VTK::RenderingWebGPU
  VTK::RenderingUI
  dawn::webgpu_dawn
  ABI
  macos-15-apple-clang-arm64-Release-shared
  MODULES
  RenderingWebGPU/RenderingUI/RenderingCore
  及
  vtkCocoaHardwareWindow（Cocoa）
  LICENSE
  share/licenses/VTK/Copyright.txt
  share/licenses/Dawn/LICENSE（Dawn
  上游许可证）
  (BSD-3))
panta_sdk_declare_asset(
  vtk
  linux-x86_64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-vtk-9.7.0-webgpu/vtk-9.7.0-linux-x86_64.tar.gz
  SHA256
  82b8fa2df5c8e275fa81eab618050584994150adbab8f14c268bb1ce586093d7
  PACKAGE
  VTK
  REQUIRED_TARGETS
  VTK::RenderingWebGPU
  VTK::RenderingUI
  dawn::webgpu_dawn
  ABI
  ubuntu-24.04-gcc13-x86_64-Release-shared
  MODULES
  RenderingWebGPU/RenderingUI/RenderingCore
  及
  vtkWaylandHardwareWindow（Wayland，VTK_USE_X=OFF）
  LICENSE
  share/licenses/VTK/Copyright.txt
  share/licenses/Dawn/LICENSE（Dawn
  上游许可证）
  (BSD-3))
panta_sdk_declare_asset(
  vtk
  windows-x86_64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-vtk-9.7.0-webgpu/vtk-9.7.0-windows-x86_64.tar.gz
  SHA256
  9015243120fd22d2f1ba509d6213b1d1579b699d7f83b8d3b32cd3586ec72e49
  PACKAGE
  VTK
  REQUIRED_TARGETS
  VTK::RenderingWebGPU
  VTK::RenderingUI
  dawn::webgpu_dawn
  ABI
  windows-msvc2022-v143-x64-Release-shared-MD
  MODULES
  RenderingWebGPU/RenderingUI/RenderingCore
  及
  vtkWin32HardwareWindow（Win32）
  LICENSE
  share/licenses/VTK/Copyright.txt
  share/licenses/Dawn/LICENSE（Dawn
  上游许可证）
  (BSD-3))

panta_sdk_declare_version(occt 8.0.1)
# OCCT 8.0.1 制品（038 受信 CI 生产，Release sdk-occt-netgen-8.0.1-6.2.2604，
# 2026-09-18 登记；2026-09-21 随任务 049 单一 tag 重产覆盖，与 Netgen 同管线
# 成对生产。2026-10-14 以 macOS 13.0 为部署目标再次重产，workflow run
# 38509650514 三平台自检、打包、发布全绿；下方 SHA 按 sidecar 与 Release API
# digest 实测回填，见 tools/sdk/releases/
# occt-netgen-8.0.1-6.2.2604.md）。维护者决策：三平台统一自托管，官方
# Windows SDK 不再消费（仅 Windows 有归档、跨平台工具链不一致）。源码 tag
# V8.0.1 → commit b8f597c677811d1f9f4d8a97f5ae2825c0353a42（unmodified）；
# Release/Shared，Draw/Visualization/DETools 与 USE_FREETYPE/USE_XLIB 关闭
# （渲染归 VTK）。
panta_sdk_declare_asset(
  occt
  macos-arm64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-occt-netgen-8.0.1-6.2.2604/occt-8.0.1-macos-arm64.tar.gz
  SHA256
  863f6bf5bba3d4cf623f697e429b34a4c9f5a32bba8d310e9dd85c0bffa4b2d7
  PACKAGE
  OpenCASCADE
  REQUIRED_TARGETS
  TKernel
  TKDESTEP
  ABI
  macos-15-apple-clang-arm64-Release-shared
  MODULES
  ModelingData/ModelingAlgorithms/DataExchange/ApplicationFramework（Draw/Visualization/DETools
  关闭）
  LICENSE
  share/licenses/OCCT
  (LGPL-2.1 + OCCT exception))
panta_sdk_declare_asset(
  occt
  linux-x86_64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-occt-netgen-8.0.1-6.2.2604/occt-8.0.1-linux-x86_64.tar.gz
  SHA256
  a87e1c47a8a1c74beb7c8d1103df3f89c03e2733084e2cac0b101440eeb4d80c
  PACKAGE
  OpenCASCADE
  REQUIRED_TARGETS
  TKernel
  TKDESTEP
  ABI
  ubuntu-24.04-gcc13-x86_64-Release-shared
  MODULES
  ModelingData/ModelingAlgorithms/DataExchange/ApplicationFramework（Draw/Visualization/DETools
  关闭）
  LICENSE
  share/licenses/OCCT
  (LGPL-2.1 + OCCT exception))
panta_sdk_declare_asset(
  occt
  windows-x86_64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-occt-netgen-8.0.1-6.2.2604/occt-8.0.1-windows-x86_64.tar.gz
  SHA256
  211d950c888c56137f2862573fd1d296fdecf16e3105bdda0a583ee297079513
  PACKAGE
  OpenCASCADE
  REQUIRED_TARGETS
  TKernel
  TKDESTEP
  ABI
  windows-msvc2022-v143-x64-Release-shared-MD
  MODULES
  ModelingData/ModelingAlgorithms/DataExchange/ApplicationFramework（Draw/Visualization/DETools
  关闭）
  LICENSE
  share/licenses/OCCT
  (LGPL-2.1 + OCCT exception))

panta_sdk_declare_version(netgen 6.2.2604)
# Netgen v6.2.2604 制品（038 同管线生产，与 OCCT 成对发布——USE_OCC 链接
# 其构建时的 OCCT，硬 ABI 锁定，升级必须成对；2026-09-18 登记，2026-09-21
# 按任务 049 覆盖重产。2026-10-14 随 OCCT 以 macOS 13.0 为目标重产，
# workflow run 38509650514 三平台自检、打包、发布全绿；下方 SHA 按 sidecar
# 与 Release API digest 实测回填。源码 tag v6.2.2604 → commit
# 3ee489c7d58fdbc2a6708cca3cbaefaae506dc17（unmodified）；GUI/Python/MPI
# 关闭；USE_NATIVE_ARCH=OFF（上游默认 ON 会使制品绑定生产机 ISA，linux
# 首版因此内含 AVX-512，无 AVX-512 消费机加载即 SIGILL）。包配置文件名为
# NetgenConfig.cmake（大写 N）：find_package 须用 `Netgen`。运行期依赖同
# 平台 OCCT 资产（消费侧处理加载路径，009/010）。
panta_sdk_declare_asset(
  netgen
  macos-arm64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-occt-netgen-8.0.1-6.2.2604/netgen-6.2.2604-macos-arm64.tar.gz
  SHA256
  f68ef20b69632f0cf9c32ea4fd922d5279ddf3303e62f3820c0ac62343afcbfe
  PACKAGE
  Netgen
  REQUIRED_TARGETS
  ngcore
  nglib
  ABI
  macos-15-apple-clang-arm64-Release-shared（链接本
  Release
  的
  occt
  macos-arm64）
  MODULES
  网格生成内核（ngcore/nglib；GUI/Python/MPI
  关闭）
  LICENSE
  share/licenses/Netgen/LICENSE
  (LGPL-2.1))
panta_sdk_declare_asset(
  netgen
  linux-x86_64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-occt-netgen-8.0.1-6.2.2604/netgen-6.2.2604-linux-x86_64.tar.gz
  SHA256
  cdd4f2f27b2eacb09de16a4efeeb2c92647b1b6fb2adfe83d90d6904683496f4
  PACKAGE
  Netgen
  REQUIRED_TARGETS
  ngcore
  nglib
  ABI
  ubuntu-24.04-gcc13-x86_64-Release-shared（链接本
  Release
  的
  occt
  linux-x86_64）
  MODULES
  网格生成内核（ngcore/nglib；GUI/Python/MPI
  关闭）
  LICENSE
  share/licenses/Netgen/LICENSE
  (LGPL-2.1))
panta_sdk_declare_asset(
  netgen
  windows-x86_64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-occt-netgen-8.0.1-6.2.2604/netgen-6.2.2604-windows-x86_64.tar.gz
  SHA256
  a986842033c69185d0ff651da30fb73b375c6ca2b86e702f3e53ef3d78c0b7de
  PACKAGE
  Netgen
  REQUIRED_TARGETS
  ngcore
  nglib
  ABI
  windows-msvc2022-v143-x64-Release-shared-MD（链接本
  Release
  的
  occt
  windows-x86_64）
  MODULES
  网格生成内核（ngcore/nglib；GUI/Python/MPI
  关闭）
  LICENSE
  share/licenses/Netgen/LICENSE
  (LGPL-2.1))

panta_sdk_declare_version(googletest 1.18.0)
# GoogleTest 1.18.0 测试专用静态 SDK（任务 070/071；workflow run
# 35999015400 三平台 MSVC 修正版构建、自检、打包与 Release 发布全绿；
# 2026-10-14 workflow run 38509650651 按 macOS 13.0 目标重产并通过三平台自检、
# 打包、发布，下方 SHA 按 sidecar 与 Release API digest 实测更新。上游 commit
# 063de7e9578f82b369302001269680b4b1553359；C++17，Windows /MD；不含 gmock。
panta_sdk_declare_asset(
  googletest
  macos-arm64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-googletest-1.18.0/googletest-1.18.0-macos-arm64.tar.gz
  SHA256
  5c7c6467a1988da5891ff57a864308bbc54f7cc5c593bd3b30b6f6dd43678a18
  PACKAGE
  GTest
  REQUIRED_TARGETS
  GTest::gtest
  GTest::gtest_main
  ABI
  macos-15-apple-clang-arm64-Release-static-cxx17
  LICENSE
  BSD-3-Clause)
panta_sdk_declare_asset(
  googletest
  linux-x86_64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-googletest-1.18.0/googletest-1.18.0-linux-x86_64.tar.gz
  SHA256
  2c3f1941a732475106a3caea607299fe8f8cd7a0481af3f8246d2e6e2794c841
  PACKAGE
  GTest
  REQUIRED_TARGETS
  GTest::gtest
  GTest::gtest_main
  ABI
  ubuntu-24.04-gcc-x86_64-Release-static-cxx17
  LICENSE
  BSD-3-Clause)
panta_sdk_declare_asset(
  googletest
  windows-x86_64
  URL
  https://github.com/Yuki-Nagori/panta/releases/download/sdk-googletest-1.18.0/googletest-1.18.0-windows-x86_64.tar.gz
  SHA256
  0fc2f014e6b2b5c7c84fd5020fb53cac30072717008d238bd06af03cfcd70a33
  PACKAGE
  GTest
  REQUIRED_TARGETS
  GTest::gtest
  GTest::gtest_main
  ABI
  windows-msvc2022-v143-x64-Release-static-MD-cxx17
  LICENSE
  BSD-3-Clause)
