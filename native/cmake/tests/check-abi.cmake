# 正例检查隐式生成目标的继承，反例确认审计能阻断目标级覆盖。
foreach(_case IN ITEMS valid runtime iterator)
  set(_runtime OFF)
  set(_iterator OFF)
  if(_case STREQUAL "runtime")
    set(_runtime ON)
  elseif(_case STREQUAL "iterator")
    set(_iterator ON)
  endif()
  execute_process(
    COMMAND
      "${CMAKE_COMMAND}" --fresh -G Ninja "-DCMAKE_MAKE_PROGRAM=${TEST_MAKE_PROGRAM}" -S
      "${CMAKE_CURRENT_LIST_DIR}/abi" -B "${TEST_BINARY_DIR}/${_case}"
      "-DCMAKE_CXX_COMPILER=${TEST_CXX_COMPILER}" "-DINJECT_BAD_RUNTIME=${_runtime}"
      "-DINJECT_BAD_ITERATOR=${_iterator}"
    RESULT_VARIABLE _result
    OUTPUT_VARIABLE _output
    ERROR_VARIABLE _error)
  if(_case STREQUAL "valid")
    if(NOT _result EQUAL 0)
      message(FATAL_ERROR "ABI inheritance failed: ${_output}\n${_error}")
    endif()
  elseif(_result EQUAL 0 OR NOT _error MATCHES "plugin_init: inconsistent MSVC ABI")
    message(FATAL_ERROR "ABI override was not rejected (${_case}): ${_output}\n${_error}")
  endif()
endforeach()
