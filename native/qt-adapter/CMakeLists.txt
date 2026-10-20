# Qt 平台服务与 QML Bridge、native visualization 隔离。
add_library(panta_qt_adapter STATIC)
add_library(panta::qt_adapter ALIAS panta_qt_adapter)
panta_native_defaults(panta_qt_adapter)

target_sources(
  panta_qt_adapter
  PRIVATE include/panta/qt_adapter/standard_paths.hpp
          include/panta/qt_adapter/system_motion_preference.hpp src/standard_paths.cpp
          src/system_motion_preference.cpp)
target_include_directories(panta_qt_adapter PUBLIC include)
target_link_libraries(panta_qt_adapter PUBLIC Qt6::Core)

if(APPLE)
  target_sources(panta_qt_adapter PRIVATE src/system_motion_preference_macos.mm)
  set_property(SOURCE src/system_motion_preference_macos.mm PROPERTY COMPILE_OPTIONS "-fobjc-arc")
  set_target_properties(
    panta_qt_adapter
    PROPERTIES OBJCXX_STANDARD 20
               OBJCXX_STANDARD_REQUIRED ON
               OBJCXX_EXTENSIONS OFF)
  find_library(PANTA_APPKIT_FRAMEWORK AppKit REQUIRED)
  target_link_libraries(panta_qt_adapter PUBLIC "${PANTA_APPKIT_FRAMEWORK}")
elseif(UNIX AND NOT APPLE)
  target_link_libraries(panta_qt_adapter PUBLIC Qt6::DBus)
elseif(WIN32)
  target_link_libraries(panta_qt_adapter PUBLIC user32)
endif()

if(BUILD_TESTING)
  add_executable(panta_qt_adapter_standard_paths_test
                 ../../tests/cpp/qt-adapter/standard_paths_test.cpp)
  panta_native_defaults(panta_qt_adapter_standard_paths_test)
  target_link_libraries(panta_qt_adapter_standard_paths_test PRIVATE panta::qt_adapter
                                                                     GTest::gtest_main)
  gtest_discover_tests(panta_qt_adapter_standard_paths_test DISCOVERY_MODE POST_BUILD
                       PROPERTIES LABELS qt-adapter)

endif()
