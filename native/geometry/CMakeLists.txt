# OCCT STEP 适配冒烟模块（任务 009）：geometry 最小公共契约 + OCCT 适配器。
# OCCT 消费走 031 固定供给（panta_require_sdk，三平台同源，禁止源码回退）；
# standards/occt.md：OCCT include 与类型限定在 src/occt/，公共头零第三方
# 类型，因此第三方库全部 PRIVATE 链接。
panta_require_sdk("occt" _occt_staging)

add_library(panta_geometry STATIC)
add_library(panta::geometry ALIAS panta_geometry)

target_include_directories(panta_geometry PUBLIC include)

target_sources(panta_geometry PRIVATE include/panta/geometry/step_import.hpp
                                      src/occt/step_reader.cpp)

panta_native_defaults(panta_geometry)
# TKDESTEP 覆盖 STEPControl 读取与 STEP 单位实体；拓扑遍历/包围盒工具
# （TKBRep/TKTopAlgo/TKMath）经其 imported target 传递依赖解析。
target_link_libraries(panta_geometry PRIVATE TKernel TKDESTEP)

# 冒烟阶段不进安装导出集：消费方（工程存储/导入业务任务）接入时再按
# foundation 模式补 install(TARGETS) 与 package 配置。
if(BUILD_TESTING)
  # 夹具根（可分发 STEP 样例，来源/单位/预期见 tests/fixtures/geometry/）。
  get_filename_component(_panta_geometry_fixture_dir
                         "${CMAKE_CURRENT_SOURCE_DIR}/../../tests/fixtures/geometry" ABSOLUTE)
  add_executable(panta_geometry_step_import_test ../../tests/cpp/geometry/step_import_test.cpp)
  panta_native_defaults(panta_geometry_step_import_test)
  target_link_libraries(panta_geometry_step_import_test PRIVATE panta::geometry GTest::gtest_main)
  # 夹具根以路径宏传入，测试代码用字符串化宏还原字面量。
  target_compile_definitions(panta_geometry_step_import_test
                             PRIVATE PANTA_GEOMETRY_FIXTURE_DIR=${_panta_geometry_fixture_dir})
  # 在构建后生成稳定的 CTest 用例清单（与 foundation 同型，见 native/CMakeLists.txt）。
  gtest_discover_tests(panta_geometry_step_import_test DISCOVERY_MODE POST_BUILD DISCOVERY_TIMEOUT
                       120 PROPERTIES LABELS geometry)
endif()
