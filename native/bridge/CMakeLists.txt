# C++ ViewModel 静态库，同时作为 Panta.Bridge QML 模块的 backing target。
# qt_add_qml_module 生成 typeinfo 和静态 plugin，应用通过 Panta.Shell 的
# TARGET 依赖导入 plugin；不在 main.cpp 里维护第二套手动注册入口。
if(UNIX AND NOT APPLE)
  # Qt Linux 预编译工具的 qtpaths 依赖其构建时 ICU ABI；当前官方归档面向
  # RHEL9，而 Ubuntu runner 没有同版本运行库。qmlls.build.ini 只是 IDE 辅助
  # 文件，不影响 typeinfo、qmlcachegen 或运行时，因此在 Linux 跳过它。
  set(PANTA_QML_MODULE_OPTIONS __QT_INTERNAL_NO_GENERATE_QMLLS_BUILD_INI)
endif()

add_library(panta_bridge STATIC)
add_library(panta::bridge ALIAS panta_bridge)

qt_add_qml_module(
  panta_bridge
  URI
  Panta.Bridge
  VERSION
  1.0
  OUTPUT_DIRECTORY
  "${CMAKE_BINARY_DIR}/Panta/Bridge"
  ${PANTA_QML_MODULE_OPTIONS}
  SOURCES
  src/settings.hpp
  src/settings.cpp
  src/utf8_boundary.hpp
  src/utf8_boundary.cpp
  src/project_view_model.hpp
  src/project_view_model.cpp
  src/large_mesh_settings.cpp)
panta_native_defaults(panta_bridge)
# 测试和 C++ 消费者按 <project_view_model.hpp> 引用头文件。
target_include_directories(panta_bridge PUBLIC src)
# ViewModel 使用 CXX 生成头，staticlib 及系统库随 target 传播到最终链接者。
target_include_directories(panta_bridge PUBLIC "${PANTA_FFI_INCLUDE_DIR}")
target_link_libraries(panta_bridge PUBLIC Qt6::Core Qt6::Qml panta::visualization panta::qt_adapter
                                          panta::mesh "${PANTA_FFI_STATIC_LIB}")
if(UNIX)
  find_package(Threads REQUIRED)
  target_link_libraries(panta_bridge PUBLIC Threads::Threads ${CMAKE_DL_LIBS})
elseif(WIN32)
  # Rust std 需要的 Windows 系统库不由 CMake target 传播，在此显式声明。
  target_link_libraries(panta_bridge PUBLIC ws2_32 userenv ntdll)
endif()

if(BUILD_TESTING)
  include(GoogleTest)
  add_executable(panta_bridge_utf8_boundary_test ../../tests/cpp/bridge/utf8_boundary_test.cpp)
  panta_native_defaults(panta_bridge_utf8_boundary_test)
  target_link_libraries(panta_bridge_utf8_boundary_test PRIVATE panta::bridge GTest::gtest_main)
  gtest_discover_tests(panta_bridge_utf8_boundary_test DISCOVERY_MODE POST_BUILD DISCOVERY_TIMEOUT
                       120 PROPERTIES LABELS bridge)

  add_executable(panta_bridge_project_view_model_test
                 ../../tests/cpp/bridge/project_view_model_test.cpp)
  panta_native_defaults(panta_bridge_project_view_model_test)
  target_include_directories(panta_bridge_project_view_model_test PRIVATE ../../tests/support/qt)
  target_link_libraries(panta_bridge_project_view_model_test PRIVATE panta::bridge GTest::gtest
                                                                     Qt6::Test)
  # 023：STEP → Netgen 体网格命令复用 tests/fixtures/geometry/box_mm.step（裸路径宏同 009/010）。
  get_filename_component(_panta_bridge_geometry_fixture_dir
                         "${CMAKE_CURRENT_SOURCE_DIR}/../../tests/fixtures/geometry" ABSOLUTE)
  target_compile_definitions(
    panta_bridge_project_view_model_test
    PRIVATE PANTA_BRIDGE_GEOMETRY_FIXTURE_DIR=${_panta_bridge_geometry_fixture_dir})
  gtest_discover_tests(panta_bridge_project_view_model_test DISCOVERY_MODE POST_BUILD
                       DISCOVERY_TIMEOUT 120 PROPERTIES LABELS bridge)

  add_executable(panta_bridge_settings_test ../../tests/cpp/bridge/settings_test.cpp)
  panta_native_defaults(panta_bridge_settings_test)
  target_link_libraries(panta_bridge_settings_test PRIVATE panta::bridge Qt6::Test)
  # 该行为测试加载 QML 引擎，使用 QtTest 统一注册运行环境。
  panta_add_qml_test(Qml.SettingsMotionPreference panta_bridge_settings_test bridge)
endif()
