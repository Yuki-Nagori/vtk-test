# Build.SdkProvision 驱动（任务 031）：构造 fixture SDK 归档，以真实
# consumer configure 驱动 sdk-provision.cmake 的成功与失败路径。
# 不编译（project NONE）；下载走 file://。覆盖：038 布局供给、发布即删
# 归档、marker 离线复用、marker 损坏经 URL 重下载重建、版本隔离、哈希
# 不符清场、生产 manifest 缺资产诊断、OCCT 内层归档/包装目录布局、配置
# 歧义、未登记名、manifest 参数校验。
#
# 输入：TEST_BINARY_DIR、PANTA_SDK_MODULE、CONSUMER_SOURCE_DIR。

function(_make_url path out_var)
  string(REGEX REPLACE "\\\\" "/" _normalized "${path}")
  string(REGEX REPLACE " " "%20" _normalized "${_normalized}")
  if(_normalized MATCHES "^/")
    set(${out_var}
        "file://${_normalized}"
        PARENT_SCOPE)
  else()
    set(${out_var}
        "file:///${_normalized}"
        PARENT_SCOPE)
  endif()
endfunction()

function(_tar_directory dir archive)
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -E tar czf "${archive}" .
    WORKING_DIRECTORY "${dir}"
    RESULT_VARIABLE _result)
  if(NOT _result EQUAL 0)
    message(FATAL_ERROR "构造 fixture 归档失败：${archive}（退出码 ${_result}）")
  endif()
endfunction()

function(_configure_consumer binary_dir)
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -G Ninja "-DCMAKE_MAKE_PROGRAM=${TEST_MAKE_PROGRAM}"
            "-S${CONSUMER_SOURCE_DIR}" "-B${binary_dir}" ${ARGN}
    RESULT_VARIABLE _rc
    OUTPUT_VARIABLE _stdout
    ERROR_VARIABLE _stderr)
  set(consumer_rc
      "${_rc}"
      PARENT_SCOPE)
  set(consumer_stdout
      "${_stdout}"
      PARENT_SCOPE)
  set(consumer_stderr
      "${_stderr}"
      PARENT_SCOPE)
endfunction()

function(_expect_success label)
  if(NOT consumer_rc EQUAL 0)
    message(FATAL_ERROR "${label}：期望成功但退出码 ${consumer_rc}\n${consumer_stderr}")
  endif()
endfunction()

function(_expect_failure label)
  if(consumer_rc EQUAL 0)
    message(FATAL_ERROR "${label}：期望失败但 configure 成功\n${consumer_stdout}")
  endif()
  foreach(_pattern IN LISTS ARGN)
    if(NOT consumer_stderr MATCHES "${_pattern}")
      message(FATAL_ERROR "${label}：诊断未命中 '${_pattern}'\n${consumer_stderr}")
    endif()
  endforeach()
endfunction()

function(_read_result file key out_var)
  file(STRINGS "${file}" _lines)
  foreach(_line IN LISTS _lines)
    if(_line MATCHES "^${key}=(.*)$")
      set(${out_var}
          "${CMAKE_MATCH_1}"
          PARENT_SCOPE)
      return()
    endif()
  endforeach()
  message(FATAL_ERROR "RESULT_FILE 缺少 ${key}：${file}")
endfunction()

# 清场检查：失败路径不得留下 .tmp 解包目录或已发布 staging；归档在成功
# 发布后即被供给器删除，残留在任何路径都属异常。
function(_assert_no_residue root label)
  file(GLOB _tmp_hit "${root}/*/.tmp-*")
  file(GLOB _staging_hit "${root}/*/*/*/.panta-sdk-provisioned")
  if(_tmp_hit OR _staging_hit)
    message(FATAL_ERROR "${label}：失败后应清场，残留 ${_tmp_hit} ${_staging_hit}")
  endif()
endfunction()

# 每次运行自清理工作目录：缓存复用/隔离在组内验证，跨运行不依赖旧状态。
set(_fixtures "${TEST_BINARY_DIR}/fixtures")
file(REMOVE_RECURSE "${TEST_BINARY_DIR}")
file(MAKE_DIRECTORY "${_fixtures}")

# fixture v1（038 建议布局：lib/cmake/<Package>/）
set(_v1 "${_fixtures}/sdk-v1")
file(WRITE "${_v1}/panta-sdk.json" "{\"name\":\"fixture\",\"version\":\"1.2.3\"}")
file(WRITE "${_v1}/include/panta_fixture/version.h" "#define PANTA_FIXTURE_VERSION \"1.2.3\"\n")
file(WRITE "${_v1}/lib/cmake/PantaFixture/PantaFixtureConfig.cmake"
     "add_library(PantaFixture::core SHARED IMPORTED)\nset(PantaFixture_VERSION 1.2.3)\n")
file(WRITE "${_v1}/share/licenses/PantaFixture/LICENSE" "fixture license\n")
set(_v1_archive "${_fixtures}/fixture-v1.tgz")
_tar_directory("${_v1}" "${_v1_archive}")
file(SHA256 "${_v1_archive}" _v1_sha)
_make_url("${_v1_archive}" _v1_url)
# 场景 2 会删除源归档证明 marker 复用不经 URL；场景 3 的重下载重建需要
# URL 可用，从字节级备份恢复，保证 SHA256 一致。
set(_v1_archive_backup "${_fixtures}/fixture-v1.keep.tgz")
configure_file("${_v1_archive}" "${_v1_archive_backup}" COPYONLY)

# fixture v2（同包名不同版本，验证版本目录隔离）
set(_v2 "${_fixtures}/sdk-v2")
file(WRITE "${_v2}/panta-sdk.json" "{\"name\":\"fixture\",\"version\":\"2.0.0\"}")
file(WRITE "${_v2}/include/panta_fixture/version.h" "#define PANTA_FIXTURE_VERSION \"2.0.0\"\n")
file(WRITE "${_v2}/lib/cmake/PantaFixture/PantaFixtureConfig.cmake"
     "add_library(PantaFixture::core SHARED IMPORTED)\nset(PantaFixture_VERSION 2.0.0)\n")
set(_v2_archive "${_fixtures}/fixture-v2.tgz")
_tar_directory("${_v2}" "${_v2_archive}")
file(SHA256 "${_v2_archive}" _v2_sha)
_make_url("${_v2_archive}" _v2_url)

# fixture OCCT 形态（外层归档内嵌内层归档；内层解出 sdk-root/cmake/ 布局）
set(_occt_inner "${_fixtures}/occt-inner")
file(WRITE "${_occt_inner}/sdk-root/cmake/FixtureOcctConfig.cmake"
     "add_library(FixtureOcct::kernel SHARED IMPORTED)\nset(FixtureOcct_VERSION 1.0.0)\n")
file(WRITE "${_occt_inner}/sdk-root/inc/fixture_occt.h" "#define FIXTURE_OCC 1\n")
set(_inner_archive "${_fixtures}/fixture-inner.tgz")
_tar_directory("${_occt_inner}" "${_inner_archive}")
file(SHA256 "${_inner_archive}" _inner_sha)
set(_occt_outer "${_fixtures}/occt-outer")
file(COPY "${_inner_archive}" DESTINATION "${_occt_outer}")
set(_outer_archive "${_fixtures}/fixture-occt-outer.tgz")
_tar_directory("${_occt_outer}" "${_outer_archive}")
file(SHA256 "${_outer_archive}" _outer_sha)
_make_url("${_outer_archive}" _outer_url)

# fixture 歧义归档（两处 PantaFixtureConfig.cmake）
set(_amb "${_fixtures}/ambiguous")
file(WRITE "${_amb}/a/lib/cmake/PantaFixture/PantaFixtureConfig.cmake"
     "add_library(PantaFixture::core SHARED IMPORTED)\n")
file(WRITE "${_amb}/b/lib/cmake/PantaFixture/PantaFixtureConfig.cmake"
     "add_library(PantaFixture::core SHARED IMPORTED)\n")
set(_amb_archive "${_fixtures}/fixture-ambiguous.tgz")
_tar_directory("${_amb}" "${_amb_archive}")
file(SHA256 "${_amb_archive}" _amb_sha)
_make_url("${_amb_archive}" _amb_url)

set(_common "-DPANTA_SDK_MODULE=${PANTA_SDK_MODULE}")

# ── 1. 038 布局成功路径：下载/校验/解包/原子发布/CONFIG 注入/target 自检 ──
set(_root1 "${TEST_BINARY_DIR}/case1-deps")
set(_result1 "${TEST_BINARY_DIR}/case1.result")
_configure_consumer(
  "${TEST_BINARY_DIR}/case1-consumer"
  ${_common}
  "-DRESULT_FILE=${_result1}"
  "-DCONSUMER_KIND=fixture"
  "-DSDK_NAME=fixture"
  "-DSDK_VERSION=1.2.3"
  "-DASSET_URL=${_v1_url}"
  "-DASSET_SHA=${_v1_sha}"
  "-DPACKAGE=PantaFixture"
  "-DCHECK_TARGET=PantaFixture::core"
  "-DPANTA_SDK_PROVISION_DIR=${_root1}")
_expect_success("038 布局供给")
_read_result("${_result1}" staging _staging1)
_read_result("${_result1}" target_found _found1)
if(NOT _found1 EQUAL 1)
  message(FATAL_ERROR "038 布局供给：imported target 未通过自检")
endif()
if(NOT EXISTS "${_staging1}/lib/cmake/PantaFixture/PantaFixtureConfig.cmake")
  message(FATAL_ERROR "038 布局供给：staging 布局不符：${_staging1}")
endif()
file(READ "${_staging1}/.panta-sdk-provisioned" _marker1)
if(NOT _marker1 MATCHES "sha256=${_v1_sha}")
  message(FATAL_ERROR "038 布局供给：marker 未记录归档哈希：${_marker1}")
endif()
file(GLOB _cached_archives "${_root1}/fixture/archives/*.archive")
list(LENGTH _cached_archives _archive_count)
if(NOT _archive_count EQUAL 0)
  message(FATAL_ERROR "038 布局供给：归档应发布即删，残留：${_cached_archives}")
endif()

# ── 2. 离线缓存复用：删除源归档后，新 consumer 用同一供给根成功 ──
file(REMOVE "${_v1_archive}")
set(_result2 "${TEST_BINARY_DIR}/case2.result")
_configure_consumer(
  "${TEST_BINARY_DIR}/case2-consumer"
  ${_common}
  "-DRESULT_FILE=${_result2}"
  "-DCONSUMER_KIND=fixture"
  "-DSDK_NAME=fixture"
  "-DSDK_VERSION=1.2.3"
  "-DASSET_URL=${_v1_url}"
  "-DASSET_SHA=${_v1_sha}"
  "-DPACKAGE=PantaFixture"
  "-DCHECK_TARGET=PantaFixture::core"
  "-DPANTA_SDK_PROVISION_DIR=${_root1}")
_expect_success("离线缓存复用")
_read_result("${_result2}" staging _staging2)
if(NOT _staging2 STREQUAL "${_staging1}")
  message(FATAL_ERROR "离线缓存复用：staging 漂移：${_staging2} != ${_staging1}")
endif()

# ── 3. marker 损坏重建：清 staging 后经 URL 重新下载恢复（归档已随
# 发布删除，离线复用只由 marker 承担；源归档从备份恢复以保证 SHA256） ──
configure_file("${_v1_archive_backup}" "${_v1_archive}" COPYONLY)
file(WRITE "${_staging1}/.panta-sdk-provisioned" "corrupted\n")
_configure_consumer(
  "${TEST_BINARY_DIR}/case2-consumer"
  ${_common}
  "-DRESULT_FILE=${_result2}"
  "-DCONSUMER_KIND=fixture"
  "-DSDK_NAME=fixture"
  "-DSDK_VERSION=1.2.3"
  "-DASSET_URL=${_v1_url}"
  "-DASSET_SHA=${_v1_sha}"
  "-DPACKAGE=PantaFixture"
  "-DCHECK_TARGET=PantaFixture::core"
  "-DPANTA_SDK_PROVISION_DIR=${_root1}")
_expect_success("marker 损坏重建")
file(READ "${_staging1}/.panta-sdk-provisioned" _marker3)
if(NOT _marker3 MATCHES "sha256=${_v1_sha}")
  message(FATAL_ERROR "marker 损坏重建：marker 未恢复：${_marker3}")
endif()

# ── 4. 版本隔离：v2 与 v1 staging 并存，v1 不被覆盖 ──
set(_result4 "${TEST_BINARY_DIR}/case4.result")
_configure_consumer(
  "${TEST_BINARY_DIR}/case4-consumer"
  ${_common}
  "-DRESULT_FILE=${_result4}"
  "-DCONSUMER_KIND=fixture"
  "-DSDK_NAME=fixture"
  "-DSDK_VERSION=2.0.0"
  "-DASSET_URL=${_v2_url}"
  "-DASSET_SHA=${_v2_sha}"
  "-DPACKAGE=PantaFixture"
  "-DCHECK_TARGET=PantaFixture::core"
  "-DPANTA_SDK_PROVISION_DIR=${_root1}")
_expect_success("版本隔离")
_read_result("${_result4}" staging _staging4)
if(_staging4 STREQUAL "${_staging1}")
  message(FATAL_ERROR "版本隔离：v2 staging 应与 v1 分离：${_staging4}")
endif()
file(READ "${_staging1}/.panta-sdk-provisioned" _marker_v1_after)
if(NOT _marker_v1_after MATCHES "sha256=${_v1_sha}")
  message(FATAL_ERROR "版本隔离：v1 marker 被覆盖：${_marker_v1_after}")
endif()

# ── 5. 哈希不符：EXPECTED_HASH 拒收并清场 ──
set(_root5 "${TEST_BINARY_DIR}/case5-deps")
_configure_consumer(
  "${TEST_BINARY_DIR}/case5-consumer"
  ${_common}
  "-DRESULT_FILE=${TEST_BINARY_DIR}/case5.result"
  "-DCONSUMER_KIND=fixture"
  "-DSDK_NAME=fixture"
  "-DSDK_VERSION=1.2.3"
  "-DASSET_URL=${_v2_url}"
  "-DASSET_SHA=${_v1_sha}"
  "-DPACKAGE=PantaFixture"
  "-DCHECK_TARGET=PantaFixture::core"
  "-DPANTA_SDK_PROVISION_DIR=${_root5}")
_expect_failure("哈希不符" "SDK 失败" "fixture")
_assert_no_residue("${_root5}" "哈希不符")
file(GLOB _mismatched_archives "${_root5}/fixture/archives/*.archive")
if(_mismatched_archives)
  message(FATAL_ERROR "哈希不符：不符归档应已删除：${_mismatched_archives}")
endif()

# ── 6. 已登记名但本平台无资产：诊断报出固定版本并指向 SDK 供给说明 ──
# （生产 manifest 现已全依赖登记，负例走 fixture 的 missing-asset 模式，
# 不依赖 manifest 状态、不触网下载。）
_configure_consumer(
  "${TEST_BINARY_DIR}/case6-consumer" ${_common} "-DRESULT_FILE=${TEST_BINARY_DIR}/case6.result"
  "-DCONSUMER_KIND=missing-asset" "-DSDK_NAME=fixture" "-DSDK_VERSION=1.2.3")
_expect_failure("生产缺资产" "native-dependency-supply.md" "1.2.3")

# ── 7. OCCT 形态：内层归档校验 + 包装目录前缀 + 外层残留不进 staging ──
set(_root7 "${TEST_BINARY_DIR}/case7-deps")
set(_result7 "${TEST_BINARY_DIR}/case7.result")
_configure_consumer(
  "${TEST_BINARY_DIR}/case7-consumer"
  ${_common}
  "-DRESULT_FILE=${_result7}"
  "-DCONSUMER_KIND=fixture"
  "-DSDK_NAME=occtshape"
  "-DSDK_VERSION=1.0.0"
  "-DASSET_URL=${_outer_url}"
  "-DASSET_SHA=${_outer_sha}"
  "-DPACKAGE=FixtureOcct"
  "-DINNER_ARCHIVE=fixture-inner.tgz"
  "-DINNER_SHA256=${_inner_sha}"
  "-DCHECK_TARGET=FixtureOcct::kernel"
  "-DPANTA_SDK_PROVISION_DIR=${_root7}")
_expect_success("OCCT 形态供给")
_read_result("${_result7}" staging _staging7)
_read_result("${_result7}" target_found _found7)
if(NOT _found7 EQUAL 1)
  message(FATAL_ERROR "OCCT 形态供给：imported target 未通过自检")
endif()
if(NOT EXISTS "${_staging7}/sdk-root/cmake/FixtureOcctConfig.cmake")
  message(FATAL_ERROR "OCCT 形态供给：包装目录布局不符：${_staging7}")
endif()
if(EXISTS "${_staging7}/fixture-inner.tgz")
  message(FATAL_ERROR "OCCT 形态供给：外层归档残留进入 staging：${_staging7}")
endif()

# ── 8. 配置歧义：归档内两处 config 命中被拒绝 ──
set(_root8 "${TEST_BINARY_DIR}/case8-deps")
_configure_consumer(
  "${TEST_BINARY_DIR}/case8-consumer"
  ${_common}
  "-DRESULT_FILE=${TEST_BINARY_DIR}/case8.result"
  "-DCONSUMER_KIND=fixture"
  "-DSDK_NAME=fixture"
  "-DSDK_VERSION=1.2.3"
  "-DASSET_URL=${_amb_url}"
  "-DASSET_SHA=${_amb_sha}"
  "-DPACKAGE=PantaFixture"
  "-DCHECK_TARGET=PantaFixture::core"
  "-DPANTA_SDK_PROVISION_DIR=${_root8}")
_expect_failure("配置歧义" "歧义")
_assert_no_residue("${_root8}" "配置歧义")

# ── 9. 未登记名：拼写错误给出已登记清单 ──
_configure_consumer(
  "${TEST_BINARY_DIR}/case9-consumer" ${_common} "-DRESULT_FILE=${TEST_BINARY_DIR}/case9.result"
  "-DCONSUMER_KIND=production" "-DSDK_NAME=nonexistent")
_expect_failure("未登记名" "未登记")

# ── 10. manifest 参数校验：非 64 位十六进制 SHA256 在登记即拒绝 ──
set(_root10 "${TEST_BINARY_DIR}/case10-deps")
_configure_consumer(
  "${TEST_BINARY_DIR}/case10-consumer"
  ${_common}
  "-DRESULT_FILE=${TEST_BINARY_DIR}/case10.result"
  "-DCONSUMER_KIND=fixture"
  "-DSDK_NAME=fixture"
  "-DSDK_VERSION=1.2.3"
  "-DASSET_URL=${_v2_url}"
  "-DASSET_SHA=deadbeef"
  "-DPACKAGE=PantaFixture"
  "-DCHECK_TARGET=PantaFixture::core"
  "-DPANTA_SDK_PROVISION_DIR=${_root10}")
_expect_failure("参数校验" "SHA256")

message(STATUS "Build.SdkProvision：10 组场景全部通过")
