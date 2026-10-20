# 桌面入口（任务 005）：链接 QML 模块与 ViewModel 模块。
qt_add_executable(panta_native_app MANUAL_FINALIZATION main.cpp)
set_target_properties(panta_native_app PROPERTIES OUTPUT_NAME panta EXPORT_NAME native-app)
if(WIN32)
  # Qt 6.11.1+ 负责生成清单及 Windows 兼容/长路径声明，并禁用链接器默认清单。
  # 官方属性只覆盖应用标识和 UAC 级别，避免维护重复的手写 manifest。
  set_target_properties(
    panta_native_app PROPERTIES QT_WINDOWS_APP_PROJECT_IDENTIFIER "com.yuki-nagori.panta"
                                QT_WINDOWS_APP_PROJECT_EXECUTION_LEVEL "asInvoker")
endif()
if(APPLE)
  # Qt 的 macOS 安装部署入口要求标准 .app 布局；开发期也从 bundle 内启动，
  # 让 cargo run 与安装后启动使用相同的可执行文件相对资源结构。
  set_target_properties(
    panta_native_app
    PROPERTIES MACOSX_BUNDLE TRUE
               MACOSX_BUNDLE_GUI_IDENTIFIER "com.yuki-nagori.panta"
               MACOSX_BUNDLE_BUNDLE_NAME "Panta")
endif()
panta_native_defaults(panta_native_app)
target_link_libraries(
  panta_native_app
  PRIVATE panta::foundation
          panta::shell
          panta_shellplugin
          "${PANTA_FFI_STATIC_LIB}"
          Qt6::Gui
          Qt6::Qml
          Qt6::Quick
          Qt6::QuickControls2
          Qt6::QuickDialogs2)
target_include_directories(panta_native_app PRIVATE "${PANTA_FFI_INCLUDE_DIR}")
if(UNIX)
  find_package(Threads REQUIRED)
  target_link_libraries(panta_native_app PRIVATE Threads::Threads ${CMAKE_DL_LIBS})
elseif(WIN32)
  target_link_libraries(panta_native_app PRIVATE ws2_32 userenv ntdll)
  # 真实窗口验收直接裸启 exe（AGENTS 验收流程）：部署机制与覆盖范围见
  # cmake/windows-app-runtime-deployment.cmake 头注。
  # install(FILES) 会把路径写入 cmake_install.cmake；先转换为正斜杠，避免
  # Windows 根路径中的反斜杠被解释成 CMake 转义序列。
  string(REPLACE "\\" "/" PANTA_WINDOWS_SYSTEM_ROOT "$ENV{windir}")
  set(PANTA_D3DCOMPILER_47_DLL "${PANTA_WINDOWS_SYSTEM_ROOT}/System32/d3dcompiler_47.dll")
  panta_find_windows_dxc_runtime(_panta_dxcompiler_dll _panta_dxil_dll)
  panta_deploy_windows_app_runtime(
    panta_native_app "${CMAKE_CURRENT_SOURCE_DIR}/../../qml" "${PANTA_D3DCOMPILER_47_DLL}"
    "${_panta_dxcompiler_dll}" "${_panta_dxil_dll}")
endif()
if(PANTA_ENABLE_BRIDGE_MODULE)
  # main.cpp 的进程级 crash handler 已在上方链接 FFI；这里仅追加 Bridge/VTK
  # 的插件和渲染依赖，关闭 Bridge 时不会把可选模块带入 app。
  target_link_libraries(panta_native_app PRIVATE panta::visualization)
  target_link_libraries(panta_native_app PRIVATE panta_bridgeplugin panta_visualizationplugin)
  target_compile_definitions(panta_native_app PRIVATE PANTA_ENABLE_BRIDGE_MODULE=1)
  set(_panta_entry "${CMAKE_CURRENT_SOURCE_DIR}/../../qml/AppEntry.qml")
else()
  set(_panta_entry "${CMAKE_CURRENT_SOURCE_DIR}/../../qml/AppNoBridgeEntry.qml")
endif()
get_filename_component(_panta_alias "${_panta_entry}" NAME)
set_source_files_properties("${_panta_entry}" PROPERTIES QT_RESOURCE_ALIAS "${_panta_alias}")

# 应用入口包装 Shell 的实际页面。独立应用模块让 Qt 部署扫描从应用目标追踪
# Shell 及 QtQuick imports，不需要另外维护一份手写的 QML 依赖清单。
qt_add_qml_module(
  panta_native_app
  URI
  Panta.App
  VERSION
  1.0
  RESOURCE_PREFIX
  /qt/qml
  DEPENDENCIES
  TARGET
  panta_shell
  QML_FILES
  "${_panta_entry}")

# 在 app 子目录显式完成 Qt executable finalization，使最终目标配置集中在 app 入口处。
qt_finalize_executable(panta_native_app)
if(WIN32)
  function(_panta_validate_windows_manifest)
    get_target_property(_panta_finalized panta_native_app _qt_is_finalized)
    if(NOT _panta_finalized)
      # Qt 可能将 target finalization 延后到 QML 文件生成之后；验证必须排在它之后。
      get_property(
        _panta_retry_count
        DIRECTORY
        PROPERTY _panta_manifest_validation_retries)
      if(NOT _panta_retry_count)
        set(_panta_retry_count 0)
      endif()
      if(_panta_retry_count GREATER_EQUAL 8)
        message(FATAL_ERROR "Qt 未能完成 panta_native_app finalization，无法验证 Windows manifest")
      endif()
      math(EXPR _panta_retry_count "${_panta_retry_count} + 1")
      set_property(DIRECTORY PROPERTY _panta_manifest_validation_retries "${_panta_retry_count}")
      cmake_language(DEFER CALL _panta_validate_windows_manifest)
      return()
    endif()

    set(_panta_windows_manifest "${CMAKE_CURRENT_BINARY_DIR}/panta.exe.manifest")
    if(NOT EXISTS "${_panta_windows_manifest}")
      message(FATAL_ERROR "Qt 未生成预期的 Windows 应用清单：${_panta_windows_manifest}")
    endif()
    get_target_property(_panta_app_sources panta_native_app SOURCES)
    set(_panta_manifest_sources "${_panta_app_sources}")
    list(FILTER _panta_manifest_sources INCLUDE REGEX "\\.manifest$")
    list(LENGTH _panta_manifest_sources _panta_manifest_source_count)
    if(NOT _panta_windows_manifest IN_LIST _panta_manifest_sources
       OR NOT _panta_manifest_source_count EQUAL 1)
      message(
        FATAL_ERROR
          "Windows 应用目标必须且只能引用二进制目录中的清单：${_panta_windows_manifest}；实际引用：${_panta_manifest_sources}")
    endif()
    # CMake 的 VS 链接包装会把用户清单作为 /MANIFESTINPUT 交给 LLD，
    # 其内部合并会改写 UAC 属性命名空间。链接阶段排除清单，再由 SDK mt.exe 嵌入。
    list(REMOVE_ITEM _panta_app_sources "${_panta_windows_manifest}")
    set_property(TARGET panta_native_app PROPERTY SOURCES "${_panta_app_sources}")
    # 清单是链接后的资源输入；修改应用标识或 UAC 设置时必须重链并重新嵌入。
    set_property(
      TARGET panta_native_app
      APPEND
      PROPERTY LINK_DEPENDS "${_panta_windows_manifest}")
    add_custom_command(
      TARGET panta_native_app
      POST_BUILD
      COMMAND "${PANTA_WINDOWS_MANIFEST_TOOL}" -nologo -manifest "${_panta_windows_manifest}"
              "-outputresource:$<TARGET_FILE:panta_native_app>;#1"
      COMMENT "嵌入 Windows 应用清单"
      VERBATIM)
  endfunction()
  cmake_language(DEFER CALL _panta_validate_windows_manifest)
endif()

if(BUILD_TESTING)
  add_executable(panta_qml_shell_module_test ../../tests/cpp/app/shell_module_load_test.cpp)
  panta_native_defaults(panta_qml_shell_module_test)
  target_link_libraries(
    panta_qml_shell_module_test
    PRIVATE panta::shell
            panta_shellplugin
            Qt6::Gui
            Qt6::Qml
            Qt6::Quick
            Qt6::QuickControls2
            Qt6::QuickDialogs2
            Qt6::Test)
  if(PANTA_ENABLE_BRIDGE_MODULE)
    target_link_libraries(
      panta_qml_shell_module_test PRIVATE panta::bridge panta::visualization panta_bridgeplugin
                                          panta_visualizationplugin)
    target_compile_definitions(panta_qml_shell_module_test PRIVATE PANTA_ENABLE_BRIDGE_MODULE=1)
  endif()
  panta_add_qml_test(Qml.ShellModuleLoads panta_qml_shell_module_test app)

  if(PANTA_ENABLE_BRIDGE_MODULE)
    # 默认验证模块加载与条目析构；真实 GPU 生命周期用例需显式启用，
    # 不在 offscreen 环境触发（见性能模块说明）。
    add_executable(panta_qml_viewport_module_test ../../tests/cpp/app/viewport_module_load_test.cpp)
    panta_native_defaults(panta_qml_viewport_module_test)
    target_include_directories(panta_qml_viewport_module_test PRIVATE ../visualization/src/vtk)
    target_link_libraries(
      panta_qml_viewport_module_test PRIVATE panta::visualization panta_visualizationplugin
                                             Qt6::Gui Qt6::Qml Qt6::Test)
    panta_add_qml_test(Qml.ViewportModuleLoads panta_qml_viewport_module_test app)
  endif()
endif()

install(
  TARGETS panta_native_app
  EXPORT pantaNativeTargets
  RUNTIME_DEPENDENCY_SET panta_native_runtime_dependencies
  BUNDLE DESTINATION . COMPONENT Runtime
  RUNTIME DESTINATION bin COMPONENT Runtime)

# QML 模块已静态编入程序；安装时只需复制扫描所得的运行期 QML imports 与插件。
# 动态库由 CMake dependency set 按 SDK 搜索路径解析并复制，确保 app 实际链接的
# native 依赖离开开发机路径后仍可加载。
qt_generate_deploy_qml_app_script(TARGET panta_native_app OUTPUT_SCRIPT
                                  panta_native_qml_deploy_script NO_TRANSLATIONS)
set(PANTA_NATIVE_QML_DEPLOY_SCRIPT
    "${panta_native_qml_deploy_script}"
    PARENT_SCOPE)
if(APPLE)
  install(
    RUNTIME_DEPENDENCY_SET
    panta_native_runtime_dependencies
    DIRECTORIES
    ${PANTA_RUNTIME_DEPENDENCY_DIRS}
    POST_EXCLUDE_REGEXES
    "^/System/Library/.*"
    "^/usr/lib/.*"
    "^${QT_STAGING}/.*"
    FRAMEWORK
    DESTINATION
    panta.app/Contents/Frameworks
    COMPONENT
    Runtime
    LIBRARY
    DESTINATION
    panta.app/Contents/Frameworks
    COMPONENT
    Runtime)
elseif(WIN32)
  # 系统目录排除必须取实际 %windir% 的大小写与分隔符：依赖解析给出的路径
  # 未规范化（如 C:\WINDOWS\system32/user32.dll，见 CMP0207），而 CMake
  # REGEX 区分大小写；写死 "Windows" 会在 windir=C:\WINDOWS 的机器上失配，
  # 把 kernel32/d3d12 等系统 DLL 复制进 bin 并在运行期阴影系统副本。
  string(REGEX REPLACE "[/\\\\]" "[/\\\\]" _panta_windir_regex "$ENV{windir}")
  install(
    RUNTIME_DEPENDENCY_SET
    panta_native_runtime_dependencies
    DIRECTORIES
    ${PANTA_RUNTIME_DEPENDENCY_DIRS}
    # Windows API Set 是由系统加载器映射的虚拟 DLL，不对应可复制的文件。
    # 在解析前排除它们，避免 CMake 将 api-ms-win-* / ext-ms-* 报为缺失依赖。
    # Windows 11 的系统库（shell32、urlmon、DMCmnUtils、Print.PrintSupport.
    # Source 等）经延迟导入引用 WDAC/设备证明可选组件；这些组件只存在于系统
    # 侧（部分机器无对应文件），由加载器按需解析，永不随应用分发。CMake 4.x
    # 原生 PE 解析器以小写名匹配 PRE_EXCLUDE（错误输出仍为原名），因此模式
    # 必须用小写。
    PRE_EXCLUDE_REGEXES
    "^api-ms-.*\\.dll$"
    "^ext-ms-.*\\.dll$"
    "^(hvsifiletrust|wpaxholder|pdmutilities|azureattestmanager|azureattestnormal)\\.dll$"
    POST_EXCLUDE_REGEXES
    "^${_panta_windir_regex}[/\\\\].*"
    "^${QT_STAGING}/.*"
    RUNTIME
    DESTINATION
    bin
    COMPONENT
    Runtime)
  # Dawn 动态加载 D3DCompiler，依赖扫描无法发现它；沿用开发部署的系统 DLL 来源。
  install(
    FILES "${PANTA_D3DCOMPILER_47_DLL}"
    DESTINATION bin
    COMPONENT Runtime)
  # Dawn 的 D3D12 后端同样动态加载 DXC 与 DXIL 签名库；缺它们时适配器枚举
  # 失败并 SEH 崩溃。二者随 Windows SDK  redistributable 分发，不进 System32。
  # find_file 返回本机反斜杠路径，先转正斜杠，避免生成安装脚本时被当作
  # 转义序列（013 记录过 \W/\a 解析事故）。
  # Dawn 在运行期动态加载这两个 SDK redistributable；路径查找与开发期部署
  # 共用 windows-app-runtime-deployment.cmake 中的实现。
  install(
    FILES "${_panta_dxcompiler_dll}" "${_panta_dxil_dll}"
    DESTINATION bin
    COMPONENT Runtime)
else()
  install(
    RUNTIME_DEPENDENCY_SET
    panta_native_runtime_dependencies
    DIRECTORIES
    ${PANTA_RUNTIME_DEPENDENCY_DIRS}
    POST_EXCLUDE_REGEXES
    "^/lib/.*"
    "^/lib64/.*"
    "^/usr/lib/.*"
    "^/usr/lib64/.*"
    "^${QT_STAGING}/.*"
    LIBRARY
    DESTINATION
    lib
    COMPONENT
    Runtime)
endif()
