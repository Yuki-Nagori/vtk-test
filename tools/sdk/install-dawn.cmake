# 将 VTK WebGPU 的 Dawn 编译/运行时文件并入 VTK SDK 安装树。

cmake_minimum_required(VERSION 3.22)

foreach(_required IN ITEMS PANTA_DAWN_INSTALL_DIR PANTA_VTK_INSTALL_DIR)
  if(NOT DEFINED ${_required})
    message(FATAL_ERROR "Dawn 安装缺少变量 ${_required}")
  endif()
endforeach()

if(NOT IS_DIRECTORY "${PANTA_DAWN_INSTALL_DIR}")
  message(FATAL_ERROR "Dawn 安装目录不存在：${PANTA_DAWN_INSTALL_DIR}")
endif()

foreach(_directory IN ITEMS include lib bin share)
  if(IS_DIRECTORY "${PANTA_DAWN_INSTALL_DIR}/${_directory}")
    file(COPY "${PANTA_DAWN_INSTALL_DIR}/${_directory}/"
         DESTINATION "${PANTA_VTK_INSTALL_DIR}/${_directory}")
  endif()
endforeach()

set(_license_source "")
foreach(_candidate IN ITEMS LICENSE LICENSE.txt COPYING)
  if(EXISTS "${PANTA_DAWN_INSTALL_DIR}/${_candidate}")
    set(_license_source "${PANTA_DAWN_INSTALL_DIR}/${_candidate}")
    break()
  endif()
endforeach()
if(NOT _license_source)
  message(FATAL_ERROR "Dawn 制品缺少 LICENSE/LICENSE.txt/COPYING")
endif()

set(_license_dir "${PANTA_VTK_INSTALL_DIR}/share/licenses/Dawn")
file(MAKE_DIRECTORY "${_license_dir}")
file(COPY "${_license_source}" DESTINATION "${_license_dir}")
