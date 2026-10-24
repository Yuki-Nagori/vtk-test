# 在 VTK 顶层 project() 完成后延迟覆盖 RenderingWebGPU 的目标标准。
#
# VTK 9.7 的模块描述仍为 cxx_std_17，但固定 Dawn source build 的
# webgpu_cpp.h 使用 C++20 语言与标准库特性。通过 VTK 提供的
# CMAKE_PROJECT_VTK_INCLUDE 钩子只调整这个生产目标，不修改上游源码；Dawn
# 本身由 superbuild 以 DAWN_ENABLE_RTTI=ON 构建，避免在这里伪造 ABI 符号。

function(panta_vtk_require_cxx20)
  if(NOT TARGET VTK::RenderingWebGPU)
    message(FATAL_ERROR "VTK::RenderingWebGPU target 尚未创建，无法固定 C++20")
  endif()

  get_target_property(_vtk_rendering_webgpu_real VTK::RenderingWebGPU ALIASED_TARGET)
  if(NOT _vtk_rendering_webgpu_real)
    set(_vtk_rendering_webgpu_real VTK::RenderingWebGPU)
  endif()

  set_property(TARGET "${_vtk_rendering_webgpu_real}" PROPERTY CXX_STANDARD 20)
  set_property(TARGET "${_vtk_rendering_webgpu_real}" PROPERTY CXX_STANDARD_REQUIRED ON)
  set_property(TARGET "${_vtk_rendering_webgpu_real}" PROPERTY CXX_EXTENSIONS OFF)
  set_property(
    TARGET "${_vtk_rendering_webgpu_real}"
    APPEND
    PROPERTY INTERFACE_COMPILE_FEATURES cxx_std_20)
endfunction()

# project(VTK) 的钩子执行早于各模块 add_subdirectory；延迟到顶层目录结束，
# 保证 RenderingWebGPU target 已存在后再设置目标属性。
cmake_language(DEFER CALL panta_vtk_require_cxx20)
