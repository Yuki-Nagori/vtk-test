# CMake 最终链接 Rust staticlib，验证真实服务与网格 DTO 的 CXX 边界。
# 边界测试复用 staticlib、生成头与系统库链入口。
function(panta_add_ffi_test name source)
  add_executable(${name} ${source})
  panta_native_defaults(${name})
  target_include_directories(${name} PRIVATE "${PANTA_FFI_INCLUDE_DIR}")
  target_link_libraries(${name} PRIVATE "${PANTA_FFI_STATIC_LIB}" GTest::gtest_main)
  if(UNIX)
    find_package(Threads REQUIRED)
    target_link_libraries(${name} PRIVATE Threads::Threads ${CMAKE_DL_LIBS})
  elseif(WIN32)
    # Rust std 不会把这些 Windows 系统库作为 CMake target 依赖传播给
    # 外部 staticlib 消费者；边界测试是最终链接者，需在此显式声明。
    target_link_libraries(${name} PRIVATE ws2_32 userenv ntdll)
  endif()
endfunction()

panta_add_ffi_test(panta_ffi_boundary_test ../../tests/cpp/ffi/boundary_test.cpp)

add_test(NAME Ffi.RustCppBoundary COMMAND panta_ffi_boundary_test)
set_tests_properties(Ffi.RustCppBoundary PROPERTIES LABELS "ffi")
