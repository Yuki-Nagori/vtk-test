/// Native Mesh IR → Rust CXX DTO 的批量转换（任务 023）。
///
/// 只给已经依赖 panta_ffi 生成头的消费者（mesh 适配器、bridge）使用；
/// mesh_ir.hpp 保持零 FFI 依赖。
#pragma once

#include "panta_ffi.h"
#include <panta/mesh/mesh_ir.hpp>

namespace panta::mesh {

/// 按值复制节点、单元与边界到 Rust 的扁平缓冲；尺寸溢出抛 std::length_error，
/// 分配失败抛 std::bad_alloc。
panta::ffi::TetMeshData to_rust_tet_mesh_data(const NativeTetMeshDto& mesh);

} // namespace panta::mesh
