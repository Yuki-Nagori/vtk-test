/// STEPControl 基础路径的 OCCT 适配器（任务 009）。本目录是仓库中唯一
/// 允许包含 OCCT 头的位置；装配/名称/颜色等 XDE 附属元数据不保留，取舍
/// 见公共头与任务记录。坐标在 OCCT 模型空间内一律毫米。
#include "shape_repository_internal.hpp"
#include <BRepBndLib.hxx>
#include <Bnd_Box.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <NCollection_Sequence.hxx>
#include <STEPControl_Reader.hxx>
#include <Standard_Failure.hxx>
#include <TCollection_AsciiString.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp.hxx>
#include <TopoDS_Shape.hxx>
#include <cstddef>
#include <cstdint>
#include <exception>
#include <filesystem>
#include <mutex>
#include <panta/geometry/step_import.hpp>
#include <string_view>
#include <unordered_map>
#include <utility>
#include <vector>

namespace panta::geometry {
namespace {

/// OCCT translator 参数是进程级状态（standards/occt.md），仓库中的 shape
/// 同样只能在这把锁内访问；导入、查询、网格借用和释放统一串行。
std::mutex& occt_repository_mutex() {
    static std::mutex mutex;
    return mutex;
}

struct ShapeEntry {
    TopoDS_Shape shape;
    detail::ShapeFaceMap faces;
    StepImportSummary summary;
    ShapeRevision revision = 1;
};

std::unordered_map<ShapeId, ShapeEntry>& shape_repository() {
    static std::unordered_map<ShapeId, ShapeEntry> entries;
    return entries;
}

ShapeId& next_shape_id() {
    static ShapeId id = 1;
    return id;
}

ShapeRef store_shape(const TopoDS_Shape& shape, const detail::ShapeFaceMap& faces,
                     const StepImportSummary& summary) {
    ShapeId& next_id = next_shape_id();
    if (next_id == 0)
        return {};

    ShapeEntry entry;
    entry.shape = shape;
    entry.faces = faces;
    entry.summary = summary;
    const ShapeRef ref{next_id, entry.revision};
    shape_repository().emplace(next_id, std::move(entry));
    ++next_id;
    return ref;
}

const ShapeEntry* find_shape(ShapeRef shape, ShapeRepositoryStatus& status) {
    const auto found = shape_repository().find(shape.id);
    if (found == shape_repository().end()) {
        status = ShapeRepositoryStatus::kNotFound;
        return {};
    }
    if (found->second.revision != shape.revision) {
        status = ShapeRepositoryStatus::kRevisionMismatch;
        return {};
    }
    status = ShapeRepositoryStatus::kNone;
    return &found->second;
}

/// 各长度单位到毫米的换算系数；kUnknown 记 0 表示无法给出换算元数据
/// （坐标仍由读取器转换为毫米，只是来源单位未知）。
double unit_to_mm(LengthUnit unit) {
    switch (unit) {
    case LengthUnit::kMillimetre:
        return 1.0;
    case LengthUnit::kCentimetre:
        return 10.0;
    case LengthUnit::kMetre:
        return 1000.0;
    case LengthUnit::kMicron:
        return 0.001;
    case LengthUnit::kInch:
        return 25.4;
    case LengthUnit::kFoot:
        return 304.8;
    case LengthUnit::kMile:
        return 1609344.0;
    case LengthUnit::kMil:
        return 0.0254;
    case LengthUnit::kMicroinch:
        return 0.0000254;
    case LengthUnit::kUnknown:
        return 0.0;
    }
    return 0.0;
}

/// FileUnits 输出的 STEP 长度单位名（小写 ISO 拼写，实测见任务 009）。
LengthUnit length_unit_from_name(std::string_view name) {
    if (name == "millimetre")
        return LengthUnit::kMillimetre;
    if (name == "centimetre")
        return LengthUnit::kCentimetre;
    if (name == "metre")
        return LengthUnit::kMetre;
    if (name == "micron")
        return LengthUnit::kMicron;
    if (name == "inch")
        return LengthUnit::kInch;
    if (name == "foot")
        return LengthUnit::kFoot;
    if (name == "mile")
        return LengthUnit::kMile;
    if (name == "mil")
        return LengthUnit::kMil;
    if (name == "microinch")
        return LengthUnit::kMicroinch;
    return LengthUnit::kUnknown;
}

/// 从 FileUnits 的长度单位名序列取源单位：全部条目一致且可识别才返回该
/// 单位；序列为空、名称未识别或混合单位均按 kUnknown 处理。
LengthUnit source_length_unit(const NCollection_Sequence<TCollection_AsciiString>& names) {
    if (names.IsEmpty())
        return LengthUnit::kUnknown;
    LengthUnit unit = length_unit_from_name(names.First().ToCString());
    if (unit == LengthUnit::kUnknown)
        return LengthUnit::kUnknown;
    for (const auto& name : names) {
        if (length_unit_from_name(name.ToCString()) != unit)
            return LengthUnit::kUnknown;
    }
    return unit;
}

/// 汇总去重拓扑计数与毫米包围盒；shape 为空时仅记录 has_geometry=false。
void fill_geometry_summary(const TopoDS_Shape& shape, StepImportSummary& summary,
                           detail::ShapeFaceMap& faces) {
    if (shape.IsNull())
        return;
    summary.has_geometry = true;

    detail::ShapeFaceMap solids;
    detail::ShapeFaceMap edges;
    TopExp::MapShapes(shape, TopAbs_SOLID, solids);
    TopExp::MapShapes(shape, TopAbs_FACE, faces);
    TopExp::MapShapes(shape, TopAbs_EDGE, edges);
    summary.solids = static_cast<std::uint32_t>(solids.Extent());
    summary.faces = static_cast<std::uint32_t>(faces.Extent());
    summary.edges = static_cast<std::uint32_t>(edges.Extent());
    Bnd_Box box;
    BRepBndLib::Add(shape, box);
    if (!box.IsVoid()) {
        box.Get(summary.bbox_min[0], summary.bbox_min[1], summary.bbox_min[2], summary.bbox_max[0],
                summary.bbox_max[1], summary.bbox_max[2]);
    }
}

} // namespace

StepImportResult import_step(const std::filesystem::path& file) {
    StepImportResult result;
    const std::lock_guard<std::mutex> lock(occt_repository_mutex());
    try {
        // 读取器逐次新建：失败路径不残留会话状态，重复导入互不影响。
        STEPControl_Reader reader;
        switch (reader.ReadFile(file.string().c_str())) {
        case IFSelect_RetDone:
            break;
        case IFSelect_RetVoid:
            result.status = StepImportStatus::kEmptyModel;
            result.detail = "ReadFile completed without producing a model";
            return result;
        case IFSelect_RetError:
            result.status = StepImportStatus::kFileUnreadable;
            result.detail = "ReadFile rejected the input";
            return result;
        case IFSelect_RetFail:
            result.status = StepImportStatus::kParseFailed;
            result.detail = "ReadFile ran but failed to parse the input";
            return result;
        case IFSelect_RetStop:
            result.status = StepImportStatus::kParseFailed;
            result.detail = "ReadFile stopped before producing a complete model";
            return result;
        }

        // 源单位来自文件内的长度单位名；FileUnits 只在读取成功的模型上
        // 安全（损坏文件上实测会段错误），因此置于 RetDone 分支之后。
        NCollection_Sequence<TCollection_AsciiString> length_names;
        NCollection_Sequence<TCollection_AsciiString> angle_names;
        NCollection_Sequence<TCollection_AsciiString> solid_angle_names;
        reader.FileUnits(length_names, angle_names, solid_angle_names);
        result.summary.source_unit = source_length_unit(length_names);
        result.summary.source_unit_to_mm = unit_to_mm(result.summary.source_unit);

        result.summary.roots_total = static_cast<std::uint32_t>(reader.NbRootsForTransfer());
        result.summary.roots_transferred = static_cast<std::uint32_t>(reader.TransferRoots());
        if (result.summary.roots_total == 0 || result.summary.roots_transferred == 0) {
            result.status = StepImportStatus::kEmptyModel;
            result.detail = "no transferable root produced a shape";
            return result;
        }
        if (result.summary.roots_transferred < result.summary.roots_total) {
            result.status = StepImportStatus::kTransferIncomplete;
            result.detail = "some transferable roots failed to convert";
        }

        const TopoDS_Shape shape = reader.OneShape();
        if (shape.IsNull()) {
            result.status = StepImportStatus::kEmptyResult;
            result.detail = "transferred shapes collapsed to a null shape";
            return result;
        }
        detail::ShapeFaceMap result_shape_faces;
        fill_geometry_summary(shape, result.summary, result_shape_faces);
        if (result.status != StepImportStatus::kNone)
            return result;
        result.shape = store_shape(shape, result_shape_faces, result.summary);
        if (result.shape.id == 0) {
            result.status = StepImportStatus::kInternalFailure;
            result.detail = "shape repository exhausted its identifier space";
            result.summary = {};
        }
        return result;
    } catch (const Standard_Failure& failure) {
        result.status = StepImportStatus::kInternalFailure;
        result.detail = failure.what();
        return result;
    } catch (const std::exception& error) {
        result.status = StepImportStatus::kInternalFailure;
        result.detail = error.what();
        return result;
    } catch (...) {
        result.status = StepImportStatus::kInternalFailure;
        result.detail = "unknown exception while importing STEP";
        return result;
    }
}

ShapeSummaryResult query_shape_summary(ShapeRef shape) {
    ShapeSummaryResult result;
    const std::lock_guard<std::mutex> lock(occt_repository_mutex());
    const ShapeEntry* entry = find_shape(shape, result.status);
    if (!entry) {
        result.detail = result.status == ShapeRepositoryStatus::kRevisionMismatch
                            ? "shape revision does not match"
                            : "shape handle was not found";
        return result;
    }
    result.summary = entry->summary;
    return result;
}

ShapeFacesResult query_shape_faces(ShapeRef shape) {
    ShapeFacesResult result;
    const std::lock_guard<std::mutex> lock(occt_repository_mutex());
    const ShapeEntry* entry = find_shape(shape, result.status);
    if (!entry) {
        result.detail = result.status == ShapeRepositoryStatus::kRevisionMismatch
                            ? "shape revision does not match"
                            : "shape handle was not found";
        return result;
    }
    try {
        result.face_summaries.reserve(static_cast<std::size_t>(entry->faces.Extent()));
        for (int face_index = 1; face_index <= entry->faces.Extent(); ++face_index) {
            Bnd_Box face_box;
            BRepBndLib::Add(entry->faces(face_index), face_box);
            ShapeFaceSummary face_summary;
            face_summary.id = static_cast<FaceId>(face_index - 1);
            if (!face_box.IsVoid()) {
                face_box.Get(face_summary.bbox_min[0], face_summary.bbox_min[1],
                             face_summary.bbox_min[2], face_summary.bbox_max[0],
                             face_summary.bbox_max[1], face_summary.bbox_max[2]);
                face_summary.has_bounds = true;
            }
            result.face_summaries.push_back(face_summary);
        }
    } catch (const Standard_Failure& error) {
        result.status = ShapeRepositoryStatus::kInternalFailure;
        result.detail = error.what();
        result.face_summaries.clear();
    } catch (const std::exception& error) {
        result.status = ShapeRepositoryStatus::kInternalFailure;
        result.detail = error.what();
        result.face_summaries.clear();
    } catch (...) {
        result.status = ShapeRepositoryStatus::kInternalFailure;
        result.detail = "unknown exception while querying shape faces";
        result.face_summaries.clear();
    }
    return result;
}

ShapeRepositoryStatus release_shape(ShapeRef shape) {
    const std::lock_guard<std::mutex> lock(occt_repository_mutex());
    ShapeRepositoryStatus status = ShapeRepositoryStatus::kNone;
    if (!find_shape(shape, status))
        return status;
    shape_repository().erase(shape.id);
    return ShapeRepositoryStatus::kNone;
}

namespace detail {

ShapeRepositoryStatus with_shape(ShapeRef shape, ShapeConsumer consumer, void* context) {
    if (consumer == nullptr)
        return ShapeRepositoryStatus::kNotFound;
    const std::lock_guard<std::mutex> lock(occt_repository_mutex());
    ShapeRepositoryStatus status = ShapeRepositoryStatus::kNone;
    const ShapeEntry* entry = find_shape(shape, status);
    if (!entry)
        return status;
    consumer(entry->shape, entry->faces, context);
    return ShapeRepositoryStatus::kNone;
}

} // namespace detail

} // namespace panta::geometry
