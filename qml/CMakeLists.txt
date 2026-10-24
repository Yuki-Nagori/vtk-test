# UI 模块（qml.md）：qt_add_qml_module 与 qml/ 目录布局一致，由 native 顶层纳入构建。
# URI Panta.Shell；资源前缀由 QTP0001(NEW) 统一为 /qt/qml。

# 主题单例声明必须先于 qt_add_qml_module（qmldir 的 singleton 标记在调用时读取）。
set_source_files_properties(Themes/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)
# 输出根目录需在创建模块前设定；Qt 会在其下追加 URI 的 Panta/Shell
# 路径，便于 qmllint、qmltestrunner 和 IDE 通过标准 import 解析模块。
set(QT_QML_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/..")

set(PANTA_SHELL_QML_FILES
    App.qml
    Components/Atoms/PanelSurface.qml
    Components/Atoms/ThemedButton.qml
    Components/Atoms/ThemedComboBox.qml
    Components/Atoms/ThemedIcon.qml
    Components/Atoms/ThemedLabel.qml
    Components/Atoms/ThemedTextField.qml
    Components/Atoms/ThemedToolButton.qml
    Components/Composites/DialogButtonRow.qml
    Components/Composites/DialogFrame.qml
    Components/Composites/DialogTitleBar.qml
    Components/Composites/DialogWindow.qml
    Components/Composites/DocumentTabBar.qml
    Dialogs/GenerateVolumeDialog.qml
    Dialogs/ImportDialog.qml
    Dialogs/LargeMeshSettingsDialog.qml
    Dialogs/NewProjectDialog.qml
    Panels/ViewportPane.qml
    Themes/Theme.qml)
set(PANTA_SHELL_QML_DEPENDENCIES)
if(PANTA_ENABLE_BRIDGE_MODULE)
  list(APPEND PANTA_SHELL_QML_DEPENDENCIES DEPENDENCIES TARGET panta_bridge panta_visualization)
endif()
if(NOT PANTA_ENABLE_BRIDGE_MODULE)
  # 提供无 Bridge 的最小 Shell，避免关闭可选模块后残留 Panta.Bridge import；
  # ViewportPane 依赖 Panta.Visualization（CaeViewport），一并剔除。
  list(REMOVE_ITEM PANTA_SHELL_QML_FILES App.qml Panels/ViewportPane.qml
       Dialogs/NewProjectDialog.qml Dialogs/ImportDialog.qml
       Dialogs/GenerateVolumeDialog.qml Dialogs/LargeMeshSettingsDialog.qml)
  list(APPEND PANTA_SHELL_QML_FILES AppNoBridge.qml)
endif()

if(UNIX AND NOT APPLE)
  # 与 Panta.Bridge 保持一致：Linux runner 的预编译 Qt qtpaths 需要未提供的
  # ICU 73；qmlls.build.ini 是编辑器辅助产物，跳过不影响 QML 资源和 typeinfo。
  set(PANTA_QML_MODULE_OPTIONS __QT_INTERNAL_NO_GENERATE_QMLLS_BUILD_INI)
endif()

# Shell 为静态模块；消费者显式导入静态插件，保留类型注册。
qt_add_qml_module(
  panta_shell
  STATIC
  URI
  Panta.Shell
  VERSION
  1.0
  RESOURCE_PREFIX
  /qt/qml
  ${PANTA_SHELL_QML_DEPENDENCIES}
  ${PANTA_QML_MODULE_OPTIONS}
  QML_FILES
  ${PANTA_SHELL_QML_FILES})

set_target_properties(panta_shell PROPERTIES EXPORT_NAME shell)
panta_native_defaults(panta_shell)
add_library(panta::shell ALIAS panta_shell)
target_sources(panta_shell PRIVATE ../native/app/icon_provider.cpp ../native/app/icon_provider.hpp)
target_include_directories(panta_shell PUBLIC ../native/app)
target_link_libraries(panta_shell PRIVATE Qt6::Svg Qt6::Quick Qt6::QuickDialogs2)

# 彩色图标由 ThemedIcon 从 qrc 原色加载；单色 utility 图标由 panta-icons provider 着色。
# CONFIGURE_DEPENDS 收集资源；视觉、功能及来源清单见 ai-docs/standards/icons.md。
file(
  GLOB PANTA_SHELL_ICON_FILES CONFIGURE_DEPENDS
  RELATIVE "${CMAKE_CURRENT_SOURCE_DIR}"
  "${CMAKE_CURRENT_SOURCE_DIR}/icons/*.svg")
qt_add_resources(panta_shell "panta_shell_icons" PREFIX "/qt/qml/Panta/Shell" FILES
                 ${PANTA_SHELL_ICON_FILES})
qt_add_resources(panta_shell "panta_shell_branding" PREFIX "/qt/qml/Panta/Shell" FILES
                 branding/panta-placeholder.svg)

if(BUILD_TESTING)
  add_executable(panta_qml_theme_component_test ../tests/qml/theme_component_test.cpp)
  panta_native_defaults(panta_qml_theme_component_test)
  target_link_libraries(
    panta_qml_theme_component_test PRIVATE panta_shell panta_shellplugin Qt6::Gui Qt6::Qml
                                           Qt6::Test Qt6::Quick)
  panta_add_qml_test(Qml.ThemeComponentParameters panta_qml_theme_component_test)
  if(PANTA_ENABLE_BRIDGE_MODULE)
    target_link_libraries(panta_qml_theme_component_test PRIVATE panta::bridge panta_bridgeplugin)
    target_compile_definitions(panta_qml_theme_component_test PRIVATE PANTA_TEST_WITH_BRIDGE=1)
  endif()
  # QML 格式门禁（任务 032）：qmlformat 来自 Qt 预编译供给（与 qmllint
  # 同源、同版本锁定），三平台同路径。
  find_program(
    PANTA_QMLFORMAT_EXECUTABLE
    NAMES qmlformat
    PATHS "${QT_STAGING}/bin"
    NO_DEFAULT_PATH REQUIRED)
  add_test(
    NAME Qml.FormatCheck
    COMMAND
      "${CMAKE_COMMAND}" "-DQMLFMT=${PANTA_QMLFORMAT_EXECUTABLE}"
      "-DQML_DIR=${CMAKE_CURRENT_SOURCE_DIR}" -P
      "${CMAKE_CURRENT_SOURCE_DIR}/../native/cmake/tests/check-qml-format.cmake")
  set_tests_properties(Qml.FormatCheck PROPERTIES LABELS "qml")
endif()
