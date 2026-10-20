/// Native OCCT adapter 之间共享的 shape 访问契约，不进入 installed/public headers。
#pragma once

#include <NCollection_IndexedMap.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <TopoDS_Shape.hxx>
#include <panta/geometry/step_import.hpp>

namespace panta::geometry::detail {

using ShapeFaceMap = NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher>;
using ShapeConsumer = void (*)(const TopoDS_Shape&, const ShapeFaceMap&, void*);

/// 验证句柄并在 OCCT 全局锁内调用 consumer。shape 与 face map 仅在回调期间
/// 借用；调用不得保存引用或重入仓库 API，否则会再次获取同一把非递归锁。回调
/// 异常向外传播，由调用 adapter 转成结构化错误。
ShapeRepositoryStatus with_shape(ShapeRef shape, ShapeConsumer consumer, void* context);

} // namespace panta::geometry::detail
