# Netgen v6.2.2604 制品构建描述（原 Panta 任务 038，vtk-test 任务 041 迁入）。
#
# 源码固定：tag v6.2.2604（lightweight）→ commit
# 3ee489c7d58fdbc2a6708cca3cbaefaae506dc17（2026-09-18 git ls-remote 复核）。
# 配置冻结：USE_OCC=ON 且链接本 superbuild 生产的 OCCT（ABI 同源，满足
# dependency-acquisition 的 Netgen↔OCCT 匹配约束）；GUI/Python/MPI/多媒体
# 关闭——网格生成内核经 nglib 消费，交互式 GUI 与 Python 绑定非本项目范围。
# USE_NATIVE_ARCH 必须显式关闭：上游默认 ON，Linux 给 ngcore 传播
# -march=native（Windows 探测 /arch:AVX…），制品绑定生产机 ISA——v6.2.2604
# 首版 linux 制品因此内含 AVX-512，在无 AVX-512 的消费机加载即 SIGILL
# （原 Panta 任务 049）。预编译 SDK 必须全机可移植，一律按上游 generic 基线出包；
# 性能调优口径归原 Panta 任务 048 的性能基线，不在制品层混入 host 相关开关。
# 许可证：LGPL-2.1（随源码树 LICENSE 文件收集）。

set(PANTA_NETGEN_VERSION 6.2.2604)
set(PANTA_NETGEN_COMMIT 3ee489c7d58fdbc2a6708cca3cbaefaae506dc17)
set(PANTA_NETGEN_INSTALL_DIR
    "${PANTA_SDK_OUT_ROOT}/netgen/${PANTA_NETGEN_VERSION}/${PANTA_SDK_TRIPLE}")

include(ExternalProject)
ExternalProject_Add(
  netgen_sdk
  GIT_REPOSITORY https://github.com/NGSolve/netgen.git
  GIT_TAG ${PANTA_NETGEN_COMMIT}
  GIT_SHALLOW TRUE
  GIT_PROGRESS TRUE
  DEPENDS occt_sdk
  BUILD_COMMAND ${CMAKE_COMMAND} --build . --config Release
  CMAKE_ARGS -DCMAKE_BUILD_TYPE=Release
             ${PANTA_SDK_PLATFORM_CMAKE_ARGS}
             -DBUILD_SHARED_LIBS=ON
             -DBUILD_FOR_CONVERSION=OFF
             -DUSE_NATIVE_ARCH=OFF
             -DUSE_GUI=OFF
             -DUSE_PYTHON=OFF
             -DUSE_MPI=OFF
             -DUSE_JPEG=OFF
             -DUSE_MPEG=OFF
             -DUSE_OCC=ON
             # OCCT 安装树的 config 目录（lib/cmake/opencascade）。必须显式指定：
             # 否则 Netgen 的 SuperBuild 会先撞上 OCCT 构建树里的 config，而
             # *Targets.cmake 只在 install 时生成，会以缺文件失败。
             -DOpenCASCADE_DIR=${PANTA_OCCT_INSTALL_DIR}/lib/cmake/opencascade
             -DCMAKE_PREFIX_PATH=${PANTA_OCCT_INSTALL_DIR}
             # Netgen 在 Apple 上把 CMAKE_INSTALL_PREFIX FORCE 成 Netgen.app
             # （CMakeLists 的 INSTALL_DIR_DEFAULT），只能经其 INSTALL_DIR 变量接管；
             # NG_INSTALL_DIR_* 是 CACHE 变量，显式压平为标准 Unix 布局，避免
             # Contents/ bundle 结构破坏统一安装树。
             -DINSTALL_DIR=${PANTA_NETGEN_INSTALL_DIR}
             -DNG_INSTALL_DIR_BIN=bin
             -DNG_INSTALL_DIR_LIB=lib
             -DNG_INSTALL_DIR_CMAKE=lib/cmake/netgen
             -DNG_INSTALL_DIR_INCLUDE=include
             -DNG_INSTALL_DIR_RES=share
             -DNG_INSTALL_DIR_PYTHON=lib/python3/dist-packages
             -DCMAKE_INSTALL_PREFIX=${PANTA_NETGEN_INSTALL_DIR}
  INSTALL_COMMAND ${CMAKE_COMMAND} --install . --config Release
  COMMAND ${CMAKE_COMMAND} -E make_directory ${PANTA_NETGEN_INSTALL_DIR}/share/licenses/Netgen
  COMMAND ${CMAKE_COMMAND} -E copy_if_different <SOURCE_DIR>/LICENSE
          ${PANTA_NETGEN_INSTALL_DIR}/share/licenses/Netgen/LICENSE
  USES_TERMINAL_DOWNLOAD TRUE
  USES_TERMINAL_BUILD TRUE)

file(
  WRITE "${CMAKE_BINARY_DIR}/netgen-panta-sdk.json.in"
  [=[
{
  "name": "netgen",
  "version": "@PANTA_NETGEN_VERSION@",
  "triple": "@PANTA_SDK_TRIPLE@",
  "source": {
    "repository": "https://github.com/NGSolve/netgen.git",
    "tag": "v6.2.2604",
    "commit": "@PANTA_NETGEN_COMMIT@"
  },
  "build_type": "Release",
  "macos_deployment_target": @PANTA_SDK_MACOS_DEPLOYMENT_TARGET_JSON@,
  "shared": true,
  "options": {"USE_OCC": true, "USE_NATIVE_ARCH": false, "USE_GUI": false, "USE_PYTHON": false, "USE_MPI": false},
  "dependencies": ["occt 8.0.1 (same sdk-occt-netgen pipeline/release, same triple, paired upgrade)"],
  "cmake_package": "lib/cmake/netgen/NetgenConfig.cmake",
  "license": "share/licenses/Netgen/LICENSE (LGPL-2.1)"
}
]=])
configure_file("${CMAKE_BINARY_DIR}/netgen-panta-sdk.json.in"
               "${CMAKE_BINARY_DIR}/netgen-panta-sdk-configure.json" @ONLY)
ExternalProject_Add_Step(
  netgen_sdk metadata
  COMMAND ${CMAKE_COMMAND} -E copy_if_different ${CMAKE_BINARY_DIR}/netgen-panta-sdk-configure.json
          ${PANTA_NETGEN_INSTALL_DIR}/panta-sdk.json
  DEPENDEES install)
