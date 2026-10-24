# 网格模块（任务 010）：自有 Mesh IR + Netgen 生成适配器。
# 结构与 geometry（src/occt）/visualization（src/vtk）同型：公共契约在
# include/panta/mesh/（零第三方类型），第三方头收敛在 src/netgen/ 适配层。
# Netgen 与 OCCT 成对发布（同 Release ABI 锁定）；NetgenConfig 按目标名
# 引用 OCCT imported targets，须在本目录之前完成 find_package(OpenCASCADE)
# （native/CMakeLists.txt 的子目录顺序保证）。
panta_require_sdk("netgen" _netgen_staging)
# libnglib 运行期跨 staging 依赖 OCCT 工具链；Linux 的 DT_RUNPATH 不传递，
# 这些工具链必须成为可执行文件的直接依赖才能经构建 rpath 解析，因此按
# 制品 NetgenConfig 的 NETGEN_OCC_LIBRARIES 全集显式列出并链接（与
# Release sdk-occt-netgen-8.0.1-6.2.2604 核对一致；2026-10-24 本仓库重产后
# 复核未变，升级时同步核对）。
# find_package 的 imported targets 留在调用目录作用域，故 OCCT 需在本
# 目录再次供给（缓存 marker 命中，不重复下载）。
panta_require_sdk("occt" _occt_staging)
set(_panta_netgen_occt_toolkits
    TKernel
    TKMath
    TKG2d
    TKG3d
    TKGeomBase
    TKBRep
    TKGeomAlgo
    TKTopAlgo
    TKPrim
    TKBO
    TKShHealing
    TKBool
    TKHLR
    TKFillet
    TKOffset
    TKMesh
    TKCDF
    TKLCAF
    TKCAF
    TKVCAF
    TKService
    TKV3d
    TKXSBase
    TKDESTEP
    TKDEIGES
    TKDESTL
    TKXCAF)

add_library(panta_mesh STATIC)
add_library(panta::mesh ALIAS panta_mesh)

target_include_directories(panta_mesh PUBLIC include)
target_include_directories(panta_mesh PRIVATE "${PANTA_FFI_INCLUDE_DIR}"
                                              "${CMAKE_CURRENT_SOURCE_DIR}/../geometry/src/occt")

target_sources(
  panta_mesh PRIVATE include/panta/mesh/mesh_ir.hpp include/panta/mesh/netgen_mesher.hpp
                     include/panta/mesh/rust_dto.hpp
                     src/mesh_ir.cpp src/netgen/netgen_mesher.cpp)

panta_native_defaults(panta_mesh)
# nglib 经其 INTERFACE_LINK_LIBRARIES 传递 ngcore；OCCT 工具链全集见上
# 文说明。geometry 为公共头类型依赖，因此 PUBLIC；第三方库保持 PRIVATE
# （静态库符号经 LINK_ONLY 传播给最终链接者）。
target_link_libraries(
  panta_mesh
  PUBLIC panta::geometry
  PRIVATE nglib ${_panta_netgen_occt_toolkits} "${PANTA_FFI_STATIC_LIB}")
if(UNIX)
  find_package(Threads REQUIRED)
  target_link_libraries(panta_mesh PRIVATE Threads::Threads ${CMAKE_DL_LIBS})
elseif(WIN32)
  target_link_libraries(panta_mesh PRIVATE ws2_32 userenv ntdll)
endif()

# 冒烟阶段不进安装导出集：消费方（网格业务任务）接入时再按 foundation
# 模式补 install(TARGETS) 与 package 配置。
if(BUILD_TESTING)
  # 生成冒烟夹具复用 tests/fixtures/geometry 的 box_mm.step（来源/单位/
  # 预期见其 README.md）；裸路径宏的取舍见 009 同型说明。
  get_filename_component(_panta_mesh_fixture_dir
                         "${CMAKE_CURRENT_SOURCE_DIR}/../../tests/fixtures/geometry" ABSOLUTE)
  add_executable(panta_mesh_ir_test ../../tests/cpp/mesh/mesh_ir_test.cpp)
  panta_native_defaults(panta_mesh_ir_test)
  target_link_libraries(panta_mesh_ir_test PRIVATE panta::mesh GTest::gtest_main)
  add_executable(panta_mesh_netgen_test ../../tests/cpp/mesh/netgen_mesher_test.cpp)
  panta_native_defaults(panta_mesh_netgen_test)
  target_link_libraries(panta_mesh_netgen_test PRIVATE panta::mesh GTest::gtest_main)
  target_compile_definitions(panta_mesh_netgen_test
                             PRIVATE PANTA_MESH_FIXTURE_DIR=${_panta_mesh_fixture_dir})
  # 在构建后生成稳定的 CTest 用例清单（与 foundation 同型，见 native/CMakeLists.txt）。
  gtest_discover_tests(panta_mesh_ir_test DISCOVERY_MODE POST_BUILD DISCOVERY_TIMEOUT 120
                       PROPERTIES LABELS mesh)
  gtest_discover_tests(panta_mesh_netgen_test DISCOVERY_MODE POST_BUILD DISCOVERY_TIMEOUT 120
                       PROPERTIES LABELS mesh)
endif()
