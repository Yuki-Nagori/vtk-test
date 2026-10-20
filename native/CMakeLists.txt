# native 层版本须与根 Cargo.toml 的 workspace.package.version 保持一致；
# 任务 004 的 launcher 诊断将依赖这一致性。3.22 是本树用到的 presets
# schema 3（build/test presets）所需的最低版本，不随在线文档的最新版浮动。
cmake_minimum_required(VERSION 3.22)
project(
  panta_native
  VERSION 0.1.0
  LANGUAGES C CXX)

if(WIN32)
  # 使用与 Windows SDK 资源编译器同目录的 mt.exe 嵌入应用清单。
  get_filename_component(_panta_windows_sdk_bin "${CMAKE_RC_COMPILER}" DIRECTORY)
  set(PANTA_WINDOWS_MANIFEST_TOOL "${_panta_windows_sdk_bin}/mt.exe")
  if(NOT EXISTS "${PANTA_WINDOWS_MANIFEST_TOOL}")
    message(FATAL_ERROR "Windows SDK manifest 工具不存在：${PANTA_WINDOWS_MANIFEST_TOOL}")
  endif()
endif()

option(PANTA_ENABLE_FFI_TEST "Build the CXX Rust/C++ boundary smoke test supplied by Cargo" OFF)
option(PANTA_ENABLE_BRIDGE_MODULE "Build and register the Panta.Bridge QML module" ON)
option(PANTA_USE_SYSTEM_TOOLS "Use explicitly selected host tools instead of Cargo-managed tools"
       OFF)
set(PANTA_LLVM_VERSION
    ""
    CACHE STRING "Expected managed LLVM version")
if(APPLE)
  enable_language(OBJCXX)
endif()

include(cmake/build-policy.cmake)

# include(CTest) 定义 BUILD_TESTING 选项（默认 ON）并启用测试；OFF 时不供给
# GoogleTest SDK、不注册测试，供 004 的发布链关闭测试面。
include(CTest)

# 输入错误应在依赖下载之前报告。
if(PANTA_ENABLE_FFI_TEST)
  if(NOT BUILD_TESTING)
    message(FATAL_ERROR "PANTA_ENABLE_FFI_TEST requires BUILD_TESTING=ON")
  endif()
  if(NOT EXISTS "${PANTA_FFI_INCLUDE_DIR}/panta_ffi.h"
     OR NOT EXISTS "${PANTA_FFI_STATIC_LIB}"
     OR IS_DIRECTORY "${PANTA_FFI_STATIC_LIB}")
    message(
      FATAL_ERROR
        "PANTA_ENABLE_FFI_TEST requires existing PANTA_FFI_INCLUDE_DIR/panta_ffi.h and PANTA_FFI_STATIC_LIB; build via Cargo"
    )
  endif()
endif()

# Qt 预编译产物供给（任务 005）：缓存于构建树，首次 configure 需联网。
# QT_STAGING 暴露路径供 find_package 和诊断使用。
include(cmake/qt-provision.cmake)
# 同一平台构建的 Qt host tools 与目标库使用同一份锁定 SDK，避免 CMake
# 从系统前缀找到其他 Qt 版本的 qmlcachegen、moc 等工具。
set(QT_HOST_PATH "${QT_STAGING}")
set(_panta_qt_components
    Core
    Gui
    Qml
    Quick
    QuickControls2
    QuickDialogs2
    Svg)
if(BUILD_TESTING)
  list(APPEND _panta_qt_components Test)
endif()
if(UNIX
   AND NOT APPLE
   AND PANTA_ENABLE_BRIDGE_MODULE)
  list(APPEND _panta_qt_components DBus)
endif()
find_package(Qt6 6.11 REQUIRED COMPONENTS ${_panta_qt_components} PATHS "${QT_STAGING}"
                                          NO_DEFAULT_PATH)
qt_standard_project_setup()
qt_policy(SET QTP0001 NEW)
qt_policy(SET QTP0004 NEW)
qt_policy(SET QTP0005 NEW)

# 预编译 SDK 供给（任务 031）：manifest 登记与 staging 供给 API；VTK/OCCT/
# Netgen 供应用例引擎，GoogleTest 仅供 BUILD_TESTING；固定制品见
# dependency-acquisition.md，不回退系统库或源码获取。
include(cmake/sdk-provision.cmake)
# Windows 开发裸启运行库部署（任务 083）：依赖 qt-provision 的 QT_STAGING
# 与 sdk-provision 的登记属性，须在两者之后 include。
include(cmake/windows-app-runtime-deployment.cmake)

if(BUILD_TESTING)
  add_test(
    NAME Build.MsvcAbiPolicy
    COMMAND
      "${CMAKE_COMMAND}" "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}/abi-policy-test"
      "-DTEST_CXX_COMPILER=${CMAKE_CXX_COMPILER}" "-DTEST_MAKE_PROGRAM=${CMAKE_MAKE_PROGRAM}" -P
      "${CMAKE_CURRENT_SOURCE_DIR}/cmake/tests/check-abi.cmake")
  set_tests_properties(Build.MsvcAbiPolicy PROPERTIES LABELS "build")
  # SDK 供给逻辑（031）用 fixture 归档驱动：不联网（file://）、不编译
  # （consumer 为 project NONE），三平台 CI 与本地同路径。
  add_test(
    NAME Build.SdkProvision
    COMMAND
      "${CMAKE_COMMAND}" "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}/sdk-provision-test"
      "-DTEST_MAKE_PROGRAM=${CMAKE_MAKE_PROGRAM}"
      "-DPANTA_SDK_MODULE=${CMAKE_CURRENT_SOURCE_DIR}/cmake/sdk-provision.cmake"
      "-DCONSUMER_SOURCE_DIR=${CMAKE_CURRENT_SOURCE_DIR}/cmake/tests/sdk/consumer" -P
      "${CMAKE_CURRENT_SOURCE_DIR}/cmake/tests/check-sdk-provision.cmake")
  set_tests_properties(Build.SdkProvision PROPERTIES LABELS "build")
  # GoogleTest v1.18.0 通过固定 manifest 下载并缓存到共享 SDK 根目录。
  # 只消费 gtest targets；gmock 待模块边界场景出现时按对应 task 打开。
  panta_require_sdk(googletest PANTA_GOOGLETEST_STAGING)
endif()

add_subdirectory(foundation)
# OCCT STEP 适配冒烟（任务 009）：经 031 固定供给消费 OpenCASCADE SDK；
# 无 Qt/QML 依赖，业务任务接入前仅由自身 gtest 消费。
add_subdirectory(geometry)
# Mesh IR 与 Netgen 适配（任务 010）：netgen 侧依赖 OCCT targets，
# 在 geometry 之后加入构建图。
add_subdirectory(mesh)
# VTK 原生视口核心（任务 007）：bridge 经 Panta.Bridge 注册 CaeViewport；
# 诊断构建（AppNoBridge）不带 VTK。
if(PANTA_ENABLE_BRIDGE_MODULE)
  add_subdirectory(qt-adapter)
  add_subdirectory(visualization)
  add_subdirectory(bridge)
endif()

# 安装时扫描 app 的真实动态依赖；搜索目录来自锁定供给树，避免依赖开发机
# 的 DYLD_LIBRARY_PATH、LD_LIBRARY_PATH 或 PATH 才能解析 SDK 运行库。
set(PANTA_RUNTIME_DEPENDENCY_DIRS)
panta_sdk_triple(_panta_runtime_triple)
foreach(_panta_sdk IN ITEMS vtk occt netgen)
  get_property(_panta_sdk_version GLOBAL PROPERTY "panta_sdk_version_${_panta_sdk}")
  set(_panta_sdk_root
      "${PANTA_SDK_PROVISION_DIR}/${_panta_sdk}/${_panta_sdk_version}/${_panta_runtime_triple}")
  if(EXISTS "${_panta_sdk_root}")
    file(
      GLOB_RECURSE _panta_sdk_runtime_files
      LIST_DIRECTORIES false
      "${_panta_sdk_root}/*.dylib" "${_panta_sdk_root}/*.so" "${_panta_sdk_root}/*.so.*"
      "${_panta_sdk_root}/*.dll")
    foreach(_panta_sdk_runtime_file IN LISTS _panta_sdk_runtime_files)
      get_filename_component(_panta_sdk_runtime_dir "${_panta_sdk_runtime_file}" DIRECTORY)
      list(APPEND PANTA_RUNTIME_DEPENDENCY_DIRS "${_panta_sdk_runtime_dir}")
    endforeach()
  endif()
endforeach()
list(REMOVE_DUPLICATES PANTA_RUNTIME_DEPENDENCY_DIRS)

# qml/ 位于仓库根（repository-layout）；二进制目录固定在 native 构建树内。
add_subdirectory("${CMAKE_CURRENT_SOURCE_DIR}/../qml" qml)
add_subdirectory(app)

if(PANTA_ENABLE_FFI_TEST)
  add_subdirectory(ffi)
endif()

# 应用随动态 SDK 库一起部署时也要带上各 SDK 的许可证与制品来源记录。
# 只复制运行时依赖，不把 SDK 头文件、静态库和 CMake 包装进应用安装树。
if(PANTA_ENABLE_BRIDGE_MODULE)
  panta_sdk_triple(_panta_runtime_triple)
  if(APPLE)
    set(_panta_runtime_resource_dir "panta.app/Contents/Resources")
  else()
    set(_panta_runtime_resource_dir "share/panta")
  endif()
  foreach(_panta_sdk IN ITEMS vtk occt netgen)
    get_property(_panta_sdk_version GLOBAL PROPERTY "panta_sdk_version_${_panta_sdk}")
    set(_panta_sdk_root
        "${PANTA_SDK_PROVISION_DIR}/${_panta_sdk}/${_panta_sdk_version}/${_panta_runtime_triple}")
    # install() 会把源路径写入 cmake_install.cmake；将 Windows 反斜杠转成
    # CMake 路径分隔符，避免 `\a`、`\t` 等被安装脚本当作转义序列。
    string(REPLACE "\\" "/" _panta_sdk_root "${_panta_sdk_root}")
    if(EXISTS "${_panta_sdk_root}/share/licenses")
      install(
        DIRECTORY "${_panta_sdk_root}/share/licenses/"
        DESTINATION "${_panta_runtime_resource_dir}/licenses"
        COMPONENT Runtime)
    endif()
    if(EXISTS "${_panta_sdk_root}/panta-sdk.json")
      install(
        FILES "${_panta_sdk_root}/panta-sdk.json"
        DESTINATION "${_panta_runtime_resource_dir}/sdk/${_panta_sdk}"
        COMPONENT Runtime)
    endif()
  endforeach()
endif()

# Qt 的 SPDX/CycloneDX 文档含运行时依赖逐组件的许可证表达式与来源信息。
# 所有构建变体都会部署 Qt，即使裁掉 Bridge 也要保留这份机器可读清单。
if(APPLE)
  set(_panta_runtime_resource_dir "panta.app/Contents/Resources")
else()
  set(_panta_runtime_resource_dir "share/panta")
endif()
string(REPLACE "\\" "/" _panta_qt_staging "${QT_STAGING}")
install(
  DIRECTORY "${_panta_qt_staging}/sbom/"
  DESTINATION "${_panta_runtime_resource_dir}/sbom"
  COMPONENT Runtime
  FILES_MATCHING
  PATTERN "*.spdx.json"
  PATTERN "*.cdx.json")

set(PANTA_NATIVE_INSTALL_CONFIG_DIR lib/cmake/panta-native)

include(CMakePackageConfigHelpers)
configure_package_config_file(
  cmake/panta-native-config.cmake.in "${CMAKE_BINARY_DIR}/cmake/panta-native-config.cmake"
  INSTALL_DESTINATION "${PANTA_NATIVE_INSTALL_CONFIG_DIR}")
write_basic_package_version_file("${CMAKE_BINARY_DIR}/cmake/panta-native-config-version.cmake"
                                 COMPATIBILITY SameMajorVersion)

install(
  EXPORT pantaNativeTargets
  FILE panta-native-targets.cmake
  NAMESPACE panta::
  DESTINATION "${PANTA_NATIVE_INSTALL_CONFIG_DIR}")
install(FILES "${CMAKE_BINARY_DIR}/cmake/panta-native-config.cmake"
              "${CMAKE_BINARY_DIR}/cmake/panta-native-config-version.cmake"
        DESTINATION "${PANTA_NATIVE_INSTALL_CONFIG_DIR}")

# Qt 部署脚本必须在全部 bundle 资源安装后执行。macOS 的部署工具会签名 app；
# 若许可证或 SBOM 随后再写入 bundle，资源封印会失效，Gatekeeper 将拒绝启动。
install(SCRIPT "${PANTA_NATIVE_QML_DEPLOY_SCRIPT}" COMPONENT Runtime)

# 等 Qt 的延迟目标创建完成后审计，包括 plugin_init/resource object targets。
cmake_language(DEFER CALL panta_verify_msvc_abi "${CMAKE_CURRENT_SOURCE_DIR}")
