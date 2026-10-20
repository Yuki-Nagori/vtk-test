# 在 find_package/FetchContent/qt_add_qml_module 之前包含：Qt 生成的 object
# library 与 GTest 也必须继承 ABI，不能仅修改业务 target。
include_guard(GLOBAL)

get_property(_panta_multi_config GLOBAL PROPERTY GENERATOR_IS_MULTI_CONFIG)
if(NOT _panta_multi_config AND NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE
      Debug
      CACHE STRING "构建类型" FORCE)
endif()
if(CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
  set(CMAKE_INSTALL_PREFIX
      "${CMAKE_BINARY_DIR}/install"
      CACHE PATH "安装前缀" FORCE)
endif()
if(NOT CMAKE_EXPORT_COMPILE_COMMANDS)
  set(CMAKE_EXPORT_COMPILE_COMMANDS
      ON
      CACHE BOOL "导出 compile_commands.json" FORCE)
endif()

if(NOT PANTA_USE_SYSTEM_TOOLS)
  if(NOT PANTA_LLVM_VERSION STREQUAL "22.1.7")
    message(FATAL_ERROR "Cargo 托管 LLVM 版本必须是 22.1.7，实际为 '${PANTA_LLVM_VERSION}'")
  endif()
  foreach(_panta_compiler IN ITEMS CMAKE_C_COMPILER CMAKE_CXX_COMPILER)
    execute_process(
      COMMAND "${${_panta_compiler}}" --version
      RESULT_VARIABLE _panta_version_status
      OUTPUT_VARIABLE _panta_version
      ERROR_VARIABLE _panta_version_error
      OUTPUT_STRIP_TRAILING_WHITESPACE)
    if(NOT _panta_version_status EQUAL 0 OR NOT _panta_version MATCHES "clang version 22\\.1\\.7")
      message(
        FATAL_ERROR "${_panta_compiler} 未使用 LLVM 22.1.7：${_panta_version} ${_panta_version_error}")
    endif()
  endforeach()
endif()

# 编译器统一为 LLVM，标准库头与运行库仍跟随平台 SDK，避免新版 libc++
# 头引用当前 macOS 系统运行库尚未提供的符号（如 __hash_memory）。
if(APPLE AND NOT PANTA_USE_SYSTEM_TOOLS)
  if(NOT EXISTS "${CMAKE_OSX_SYSROOT}/usr/include/c++/v1/string")
    message(FATAL_ERROR "Apple SDK 缺少 libc++ 头文件，请通过 Cargo 配置 sysroot")
  endif()
  add_compile_options("$<$<COMPILE_LANGUAGE:CXX>:-nostdinc++>"
                      "$<$<COMPILE_LANGUAGE:CXX>:-isystem${CMAKE_OSX_SYSROOT}/usr/include/c++/v1>")
endif()

if(MSVC)
  # 当前 Rust/CXX staticlib 使用 /MD；Debug 保留调试符号及未优化代码，
  # 但整个 native 图统一使用 release CRT/STL ABI，包括 Qt 生成目标。
  set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreadedDLL)
  add_compile_definitions(_ITERATOR_DEBUG_LEVEL=0)
  # Qt 也选择 release ABI，避免 /MD 对象链接 Qt Debug DLL 导致混用 CRT。
  # 空元素允许无配置 imported 工具；不回退到 Debug 库。
  set(CMAKE_MAP_IMPORTED_CONFIG_DEBUG "Release;RelWithDebInfo;")
endif()

# 告警仅应用到自有代码；生成代码和第三方只继承上面的 ABI 策略。
function(panta_native_defaults target)
  target_compile_features(${target} PUBLIC cxx_std_20)
  set_target_properties(${target} PROPERTIES CXX_EXTENSIONS OFF)
  if(MSVC)
    target_compile_options(${target} PRIVATE /W4 /permissive-)
    # Rust/CXX bridge 的错误边界抛出 rust::Error；clang-cl 默认关闭异常。
    target_compile_options(${target} PRIVATE /EHsc)
  else()
    target_compile_options(${target} PRIVATE -Wall -Wextra -Wpedantic)
  endif()
endfunction()

# QML 测试统一使用托管 Qt、无窗口平台与可定制的 Controls 样式。
# Windows 无控制台时 QtTest 默认写调试输出；强制 stderr 使 CTest 能展示断言。
# TIMEOUT 兜底：这些测试正常亚秒完成；平台挂起（Windows 上链接 VTK/Dawn
# 静态库的消费方二进制曾整轮挂起，见任务 007 验证表）时快速失败并保留
# 诊断输出，不拖垮整个 CI。20s 对亚秒级测试已留足余量。
function(panta_add_qml_test name target)
  set(_panta_qml_timeout 20)
  set(_panta_qml_labels qml)
  if(ARGC GREATER 2)
    list(APPEND _panta_qml_labels "${ARGV2}")
  endif()
  add_test(NAME ${name} COMMAND ${target})
  set_tests_properties(
    ${name}
    PROPERTIES
      TIMEOUT
      ${_panta_qml_timeout}
      LABELS
      "${_panta_qml_labels}"
      ENVIRONMENT
      "QT_QPA_PLATFORM=offscreen;QT_FORCE_STDERR_LOGGING=1;QT_QUICK_CONTROLS_STYLE=Basic;QT_PLUGIN_PATH=${QT_STAGING}/plugins;QML_IMPORT_PATH=${QT_STAGING}/qml"
  )
endfunction()

# 递归核对真实构建图（包括 Qt 生成的 object libraries），提前阻断上游或
# 子目录覆盖运行库策略。依赖自己的源文件也必须遵守相同 ABI。
function(panta_verify_msvc_abi directory)
  if(NOT MSVC)
    return()
  endif()
  get_property(
    _targets
    DIRECTORY "${directory}"
    PROPERTY BUILDSYSTEM_TARGETS)
  get_property(
    _directory_defines
    DIRECTORY "${directory}"
    PROPERTY COMPILE_DEFINITIONS)
  foreach(_target IN LISTS _targets)
    get_target_property(_type ${_target} TYPE)
    if(_type MATCHES "^(EXECUTABLE|STATIC_LIBRARY|SHARED_LIBRARY|MODULE_LIBRARY|OBJECT_LIBRARY)$")
      get_target_property(_runtime ${_target} MSVC_RUNTIME_LIBRARY)
      get_property(
        _defines
        TARGET ${_target}
        PROPERTY COMPILE_DEFINITIONS)
      list(APPEND _defines ${_directory_defines})
      if(NOT _runtime STREQUAL "MultiThreadedDLL"
         OR NOT "_ITERATOR_DEBUG_LEVEL=0" IN_LIST _defines
         OR "_ITERATOR_DEBUG_LEVEL=1" IN_LIST _defines
         OR "_ITERATOR_DEBUG_LEVEL=2" IN_LIST _defines)
        message(
          FATAL_ERROR
            "${_target}: inconsistent MSVC ABI (${_runtime}; ${_defines}); expected /MD and _ITERATOR_DEBUG_LEVEL=0"
        )
      endif()
    endif()
  endforeach()
  get_property(
    _directories
    DIRECTORY "${directory}"
    PROPERTY SUBDIRECTORIES)
  foreach(_directory IN LISTS _directories)
    panta_verify_msvc_abi("${_directory}")
  endforeach()
endfunction()
