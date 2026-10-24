# VTK 原生视口模块：Panta.Visualization 静态 QML 模块。
# CaeViewport（应用/QML 面，零第三方类型）在 include/panta/visualization/；
# VTK 专属代码收敛在 src/vtk/ 适配器（standards/vtk.md：VTK 不是公共 API）。
# VTK 消费走固定供给（panta_require_sdk，三平台同源，禁止源码回退）；
# 模块链接显式最小集，其余依赖经 imported target 传递。
panta_require_sdk("vtk" _vtk_staging)

# 先声明 STATIC 库，qt_add_qml_module 才会生成静态 plugin（与 bridge 同型）。
# qt_add_qml_module 按 URI 自动创建别名 panta::visualization，无需手动。
add_library(panta_visualization STATIC)

# 公共头按 panta/<module>/<header>.hpp 布局（与 panta/foundation 同型）；
# include 目录必须在 qt_add_qml_module 之前声明——QML 类型注册的 include
# 形式按当时已声明的目标 include 目录推断。
target_include_directories(panta_visualization PUBLIC include)
target_include_directories(panta_visualization PRIVATE src include/panta/visualization)

qt_add_qml_module(
  panta_visualization
  URI
  Panta.Visualization
  VERSION
  1.0
  # 与 URI target path 对齐（qmllint import 解析依赖该布局，与 bridge 同型）。
  OUTPUT_DIRECTORY
  "${CMAKE_BINARY_DIR}/Panta/Visualization"
  SOURCES
  include/panta/visualization/cae_viewport.hpp
  include/panta/visualization/render_scene.hpp
  include/panta/visualization/mesh_source.hpp
  include/panta/visualization/viewport_backend.hpp
  src/cae_viewport.cpp
  src/vtk/vtk_viewport.hpp
  src/vtk/vtk_viewport.cpp
  src/vtk/navigation/viewport_camera.cpp
  src/vtk/navigation/viewport_camera.hpp
  src/vtk/navigation/viewport_input.cpp
  src/vtk/navigation/viewport_input.hpp
  src/vtk/navigation/viewport_orientation.cpp
  src/vtk/navigation/viewport_orientation.hpp
  src/vtk/navigation/viewport_tools.cpp
  src/vtk/navigation/viewport_tools.hpp
  src/vtk/primitive_scene.cpp
  src/vtk/primitive_scene.hpp
  src/vtk/surface_mesh.cpp
  src/vtk/surface_mesh.hpp
  src/vtk/vtk_native_surface.hpp)
if(APPLE)
  target_sources(panta_visualization PRIVATE src/vtk/vtk_native_surface.mm)
else()
  target_sources(panta_visualization PRIVATE src/vtk/vtk_native_surface.cpp)
endif()
add_library(panta::visualization ALIAS panta_visualization)
panta_native_defaults(panta_visualization)
# Linux embeds VTK in a Wayland subsurface instead of rendering into the
# top-level Qt surface, so the bridge also links the Wayland client protocol.
if(UNIX AND NOT APPLE)
  find_package(PkgConfig REQUIRED)
  pkg_check_modules(WAYLAND_CLIENT REQUIRED IMPORTED_TARGET wayland-client)
endif()
# WebGPU/hardware-window 路径不依赖 Qt VTK GUI 或 OpenGL scenegraph；
# 字形、法线和面内文本几何所需的 RenderingFreeType/FiltersCore/FiltersGeneral 与
# FiltersSources 显式列出；视口色标 vtkScalarBarActor 来自 RenderingAnnotation
# （task 038，SDK 由 task 041 开启）；Dawn 由 RenderingWebGPU 的接口依赖提供。公共头只有
# Qt 类型，VTK/Dawn 仅为模块内部消费——PRIVATE 链接避免第三方 include
# 路径泄漏给消费方（静态库符号仍经 LINK_ONLY 传播给最终链接者）；
# Qt6::Quick 对应公共头的 QQuickItem 基类，保持 PUBLIC。
target_link_libraries(
  panta_visualization
  PRIVATE VTK::RenderingWebGPU
          VTK::RenderingUI
          VTK::RenderingCore
          VTK::RenderingAnnotation
          VTK::RenderingFreeType
          VTK::FiltersCore
          VTK::FiltersGeneral
          VTK::FiltersSources
  PUBLIC Qt6::Quick)
if(UNIX AND NOT APPLE)
  target_link_libraries(panta_visualization PRIVATE PkgConfig::WAYLAND_CLIENT)
endif()
# VTK::RenderingWebGPU 声明需要模块自动初始化；否则 render window 能创建并提交帧，
# 但 WebGPU renderer/mapper 的实现工厂不会注册，场景会保持空白。
vtk_module_autoinit(
  TARGETS
  panta_visualization
  MODULES
  VTK::RenderingWebGPU
  VTK::RenderingUI
  VTK::RenderingCore
  VTK::RenderingAnnotation
  VTK::RenderingFreeType
  VTK::FiltersCore
  VTK::FiltersGeneral
  VTK::FiltersSources)

if(BUILD_TESTING)
  foreach(_test IN ITEMS viewport_navigation_test primitive_scene_test)
    add_executable(panta_${_test} ../../tests/cpp/visualization/${_test}.cpp)
    panta_native_defaults(panta_${_test})
    target_include_directories(panta_${_test} PRIVATE src/vtk)
    target_link_libraries(panta_${_test} PRIVATE panta_visualization Qt6::Test VTK::CommonDataModel)
  endforeach()
  target_link_libraries(panta_primitive_scene_test PRIVATE VTK::RenderingCore VTK::RenderingWebGPU
                                                          VTK::RenderingAnnotation)
  panta_add_qml_test(Visualization.PrimitiveScene panta_primitive_scene_test visualization)
  panta_add_qml_test(Visualization.ViewportNavigation panta_viewport_navigation_test visualization)
  if(APPLE)
    add_executable(panta_native_surface_lifecycle_test
                   ../../tests/cpp/visualization/native_surface_lifecycle_test.mm)
    panta_native_defaults(panta_native_surface_lifecycle_test)
    target_include_directories(panta_native_surface_lifecycle_test PRIVATE src/vtk)
    target_link_libraries(panta_native_surface_lifecycle_test PRIVATE panta_visualization Qt6::Test
                                                                      VTK::RenderingUI)
    panta_add_qml_test(Visualization.NativeSurfaceLifecycle panta_native_surface_lifecycle_test
                       visualization)
  endif()
endif()
