/// Netgen OCC 路径的体网格适配器（任务 010）。src/netgen/ 是仓库中唯一
/// 允许包含 Netgen 头的位置；nglib v1 的查询接口不暴露面/区域索引，边界
/// 与区域映射经 libnglib 同源发布的 netgen C++ 头读取（Ng_Mesh* 即
/// netgen::Mesh*）。netgen 内部索引 1 起，转换为 IR 时统一压成 0 起。
///
/// mystdlib.h 提供的 `using namespace std` 是 netgen 遗留头
/// （ngarray/table/hashtabl/optmem）编译的硬前提（上游契约，clang-format
/// 的包含排序规则确保它先于相关头文件引入）。该污染只存在于
/// 本适配器 TU；自有代码保持限定名。
#include <mystdlib.h>

#include <Standard_Failure.hxx>
#include <TopoDS_Shape.hxx>
#include <algorithm>
#include <array>
#include <cstddef>
#include <exception>
#include <memory>
#include <meshing/meshclass.hpp>
#include <meshing/meshtype.hpp>
#include <mutex>
// Netgen 的 OCC C++ API 只在启用此宏后由 occgeom.hpp 声明。
#define OCCGEOMETRY
#include <occ/occgeom.hpp>
#include <panta/geometry/step_import.hpp>
#include <panta/mesh/mesh_ir.hpp>
#include <panta/mesh/netgen_mesher.hpp>
#include <shape_repository_internal.hpp>
#include <string>
#include <utility>
#include <vector>

// 6.2.2604 制品的实现符号位于 namespace nglib（libnglib 只导出
// nglib::Ng_*，无全局符号），而上游 nglib.h/nglib_occ.h 声明在全局
// 命名空间；按制品 ABI 把两个头包含进 namespace nglib，使声明与符号
// 一致。包装只作用于本适配器 TU，调用处全部以 nglib:: 限定。
namespace nglib {
#include <nglib.h>
#include <nglib_occ.h>
} // namespace nglib

namespace panta::mesh {
namespace {

/// Netgen 与 OCCT 的网格生成路径使用进程级全局状态（standards/netgen.md），
/// 首期保守串行：全部生成共用一把进程级锁。
std::mutex& netgen_mutex() {
    static std::mutex mutex;
    return mutex;
}

/// nglib::Ng_Init 每进程一次；不调用 Ng_Exit——重复 Init/Exit 会清空全局
/// 状态，进程退出时由运行时回收。
void init_netgen_once() {
    static std::once_flag flag;
    std::call_once(flag, [] { nglib::Ng_Init(); });
}

/// 净化函数表：生成阶段失败统一转 kMeshingFailed 并携带阶段与返回码。
std::string meshing_failure_detail(const char* stage, nglib::Ng_Result result) {
    return std::string(stage) + " failed with Ng_Result " +
           std::to_string(static_cast<int>(result));
}

/// 排序去重：Netgen 的 1 起编号压缩为 IR 的 0 起连续 ID；返回压缩映射。
std::vector<MeshIndex> compact_ids(std::vector<int> values) {
    std::sort(values.begin(), values.end());
    values.erase(std::unique(values.begin(), values.end()), values.end());
    std::vector<MeshIndex> mapping;
    mapping.reserve(values.size());
    for (const int value : values)
        mapping.push_back(static_cast<MeshIndex>(value) - 1U);
    return mapping;
}

MeshIndex compressed_index(const std::vector<MeshIndex>& mapping, int one_based) {
    const auto found =
        std::lower_bound(mapping.begin(), mapping.end(), static_cast<MeshIndex>(one_based) - 1U);
    return static_cast<MeshIndex>(found - mapping.begin());
}

/// 仅用于 Netgen 节点顺序转换，Mesh IR 的度量计算由 Rust 所有。
double six_times_tet_volume(const std::array<double, 3>& p0, const std::array<double, 3>& p1,
                            const std::array<double, 3>& p2, const std::array<double, 3>& p3) {
    const auto u = std::array{p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]};
    const auto v = std::array{p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]};
    const auto w = std::array{p3[0] - p0[0], p3[1] - p0[1], p3[2] - p0[2]};
    const std::array cross = {u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2],
                              u[0] * v[1] - u[1] * v[0]};
    return cross[0] * w[0] + cross[1] * w[1] + cross[2] * w[2];
}

/// 把 netgen::Mesh 的节点、四面体与边界面元转换为 IR；区域取自体单元
/// 的域号，边界分组取自面描述子的 OCC 面序号（SurfNr）。
bool convert_to_native_dto(const netgen::Mesh& source,
                           const std::vector<panta::geometry::FaceId>& face_ids_by_netgen_index,
                           TetMeshGenerationResult& result) {
    result.mesh.nodes.reserve(static_cast<std::size_t>(source.GetNP()));
    for (std::size_t i = 1; i <= source.GetNP(); ++i) {
        const auto& point = source.Point(static_cast<int>(i));
        result.mesh.nodes.push_back({point(0), point(1), point(2)});
    }

    std::vector<int> domain_ids;
    std::vector<int> face_ids;
    domain_ids.reserve(static_cast<std::size_t>(source.GetNE()));
    face_ids.reserve(static_cast<std::size_t>(source.GetNSE()));
    for (const auto& element : source.VolumeElements()) {
        if (element.GetNP() != 4) {
            result.status = TetMeshGenerationStatus::kConversionFailed;
            result.detail = "volume element with " + std::to_string(element.GetNP()) +
                            " nodes is outside the smoke contract";
            return false;
        }
        domain_ids.push_back(element.GetIndex());
    }
    for (const auto& element : source.SurfaceElements()) {
        if (element.GetNP() != 3) {
            result.status = TetMeshGenerationStatus::kConversionFailed;
            result.detail = "surface element with " + std::to_string(element.GetNP()) +
                            " nodes is outside the smoke contract";
            return false;
        }
        const netgen::FaceDescriptor& descriptor = source.GetFaceDescriptor(element);
        if (descriptor.SurfNr() < 1 ||
            static_cast<std::size_t>(descriptor.SurfNr()) > face_ids_by_netgen_index.size()) {
            result.status = TetMeshGenerationStatus::kFaceMappingFailed;
            result.detail = "Netgen surface element refers to an unknown OCC face";
            return false;
        }
        face_ids.push_back(descriptor.SurfNr());
    }

    const std::vector<MeshIndex> domain_mapping = compact_ids(std::move(domain_ids));
    const std::vector<MeshIndex> face_mapping = compact_ids(std::move(face_ids));
    result.mesh.region_count = static_cast<MeshIndex>(domain_mapping.size());
    result.mesh.boundary_group_count = static_cast<MeshIndex>(face_mapping.size());
    result.mesh.boundary_group_face_ids.reserve(face_mapping.size());
    for (const MeshIndex netgen_face_index : face_mapping) {
        if (netgen_face_index >= face_ids_by_netgen_index.size()) {
            result.status = TetMeshGenerationStatus::kFaceMappingFailed;
            result.detail = "Netgen face descriptor refers outside its OCC face map";
            return false;
        }
        result.mesh.boundary_group_face_ids.push_back(face_ids_by_netgen_index[netgen_face_index]);
    }

    result.mesh.tets.reserve(static_cast<std::size_t>(source.GetNE()));
    for (const auto& element : source.VolumeElements()) {
        const MeshIndex region = compressed_index(domain_mapping, element.GetIndex());
        std::array<MeshIndex, 4> nodes = {
            static_cast<MeshIndex>(element[0]) - 1U, static_cast<MeshIndex>(element[1]) - 1U,
            static_cast<MeshIndex>(element[2]) - 1U, static_cast<MeshIndex>(element[3]) - 1U};
        // 方向归一化属于 Netgen →领域值转换：源库局部顺序可能为负；
        // Rust Mesh IR 统一要求正有向体积，所以此处只按契约调整节点顺序。
        if (six_times_tet_volume(result.mesh.nodes[nodes[0]], result.mesh.nodes[nodes[1]],
                                 result.mesh.nodes[nodes[2]], result.mesh.nodes[nodes[3]]) < 0.0) {
            std::swap(nodes[2], nodes[3]);
        }
        result.mesh.tets.push_back({nodes, region});
    }
    result.mesh.boundary.reserve(static_cast<std::size_t>(source.GetNSE()));
    for (const auto& element : source.SurfaceElements()) {
        const netgen::FaceDescriptor& descriptor = source.GetFaceDescriptor(element);
        const MeshIndex group = compressed_index(face_mapping, descriptor.SurfNr());
        result.mesh.boundary.push_back(
            {{static_cast<MeshIndex>(element[0]) - 1U, static_cast<MeshIndex>(element[1]) - 1U,
              static_cast<MeshIndex>(element[2]) - 1U},
             group});
    }
    return true;
}

bool map_netgen_faces(const netgen::OCCGeometry& geometry,
                      const panta::geometry::detail::ShapeFaceMap& source_faces,
                      std::vector<panta::geometry::FaceId>& face_ids,
                      TetMeshGenerationResult& result) {
    if (geometry.fmap.Extent() != source_faces.Extent()) {
        result.status = TetMeshGenerationStatus::kFaceMappingFailed;
        result.detail = "Netgen and source shape contain different face counts";
        return false;
    }

    face_ids.reserve(static_cast<std::size_t>(geometry.fmap.Extent()));
    for (int netgen_index = 1; netgen_index <= geometry.fmap.Extent(); ++netgen_index) {
        const int source_index = source_faces.FindIndex(geometry.fmap(netgen_index));
        if (source_index < 1) {
            result.status = TetMeshGenerationStatus::kFaceMappingFailed;
            result.detail = "Netgen face cannot be matched to the imported shape topology";
            face_ids.clear();
            return false;
        }
        face_ids.push_back(static_cast<panta::geometry::FaceId>(source_index - 1));
    }
    return true;
}

} // namespace

namespace {

/// COMPAT(task 001; remove-task 002): Netgen v6.2.2604 的 OCC
/// 名称表会与 FaceDescriptor 共享字符串指针。按地址识别并摘除借用项，
/// 不解引用名称字符串，避免将描述符内部存储误当作独立堆对象释放。
void delete_native_mesh(nglib::Ng_Mesh* native_mesh) {
    if (native_mesh == nullptr)
        return;

    auto& mesh = *reinterpret_cast<netgen::Mesh*>(native_mesh);
    const auto clear_descriptor_aliases = [&mesh] {
        for (int codimension = 0; codimension < 4; ++codimension) {
            auto& names = mesh.GetRegionNamesCD(codimension);
            for (std::size_t name_index = 0; name_index < names.Size(); ++name_index) {
                const std::string* const name = names[name_index];
                if (name == nullptr)
                    continue;

                for (int descriptor_index = 1; descriptor_index <= mesh.GetNFD();
                     ++descriptor_index) {
                    const std::string* const descriptor_name =
                        &mesh.GetFaceDescriptor(descriptor_index).GetBCName();
                    if (name == descriptor_name) {
                        names[name_index] = nullptr;
                        break;
                    }
                }
            }
        }
    };
    clear_descriptor_aliases();

    // v6.2.2604 的 Ng_DeleteMesh 先调用 DeleteMesh 再析构 Mesh；两段清理都会
    // 释放 codim-1/2 名称。保留这两个官方清理步骤，并在其间清空已释放的槽，
    // 避免析构二次释放；codim-0/3 名称仍由 Mesh 析构释放。
    mesh.DeleteMesh();
    for (int codimension = 1; codimension <= 2; ++codimension) {
        auto& names = mesh.GetRegionNamesCD(codimension);
        for (std::size_t name_index = 0; name_index < names.Size(); ++name_index)
            names[name_index] = nullptr;
    }

    delete &mesh;
}

void delete_occ_geometry(netgen::OCCGeometry* geometry) {
    if (geometry != nullptr)
        nglib::Ng_OCC_DeleteGeometry(reinterpret_cast<nglib::Ng_OCC_Geometry*>(geometry));
}

TetMeshGenerationResult
generate_for_shape(const TopoDS_Shape& shape,
                   const panta::geometry::detail::ShapeFaceMap& source_faces,
                   const TetMeshGenerationParameters& parameters) {
    TetMeshGenerationResult result;
    using OccGeometryOwner = std::unique_ptr<netgen::OCCGeometry, decltype(&delete_occ_geometry)>;
    OccGeometryOwner occ_geometry(new netgen::OCCGeometry(shape, 3, false), delete_occ_geometry);
    auto* const geometry = reinterpret_cast<nglib::Ng_OCC_Geometry*>(occ_geometry.get());
    using NativeMeshOwner = std::unique_ptr<nglib::Ng_Mesh, decltype(&delete_native_mesh)>;
    NativeMeshOwner native_mesh(nullptr, delete_native_mesh);

    try {
        std::vector<panta::geometry::FaceId> face_ids_by_netgen_index;
        if (map_netgen_faces(*occ_geometry, source_faces, face_ids_by_netgen_index, result)) {
            native_mesh.reset(nglib::Ng_NewMesh());
        }
        if (result.status == TetMeshGenerationStatus::kNone && native_mesh == nullptr) {
            result.status = TetMeshGenerationStatus::kInternalFailure;
            result.detail = "Ng_NewMesh returned no mesh";
        }

        if (result.status == TetMeshGenerationStatus::kNone) {
            nglib::Ng_Meshing_Parameters native_parameters;
            if (parameters.max_length_mm > 0.0)
                native_parameters.maxh = parameters.max_length_mm;
            // Netgen OCC 的四面体方向按 Mesh IR 契约在转换时归一化。
            const nglib::Ng_Result set_size =
                nglib::Ng_OCC_SetLocalMeshSize(geometry, native_mesh.get(), &native_parameters);
            if (set_size != nglib::NG_OK) {
                result.status = TetMeshGenerationStatus::kMeshingFailed;
                result.detail = meshing_failure_detail("Ng_OCC_SetLocalMeshSize", set_size);
            } else if (const nglib::Ng_Result edges = nglib::Ng_OCC_GenerateEdgeMesh(
                           geometry, native_mesh.get(), &native_parameters);
                       edges != nglib::NG_OK) {
                result.status = TetMeshGenerationStatus::kMeshingFailed;
                result.detail = meshing_failure_detail("Ng_OCC_GenerateEdgeMesh", edges);
            } else if (const nglib::Ng_Result surfaces = nglib::Ng_OCC_GenerateSurfaceMesh(
                           geometry, native_mesh.get(), &native_parameters);
                       surfaces != nglib::NG_OK) {
                result.status = TetMeshGenerationStatus::kMeshingFailed;
                result.detail = meshing_failure_detail("Ng_OCC_GenerateSurfaceMesh", surfaces);
            } else if (const nglib::Ng_Result volume =
                           nglib::Ng_GenerateVolumeMesh(native_mesh.get(), &native_parameters);
                       volume != nglib::NG_OK) {
                result.status = TetMeshGenerationStatus::kMeshingFailed;
                result.detail = meshing_failure_detail("Ng_GenerateVolumeMesh", volume);
            }
        }

        if (result.status == TetMeshGenerationStatus::kNone) {
            const auto* mesh = reinterpret_cast<const netgen::Mesh*>(native_mesh.get());
            if (mesh->GetNP() == 0 || mesh->GetNE() == 0) {
                result.status = TetMeshGenerationStatus::kEmptyMesh;
                result.detail = "generation finished without a non-empty mesh";
            } else if (!convert_to_native_dto(*mesh, face_ids_by_netgen_index, result)) {
                result.mesh = NativeTetMeshDto{};
            }
        }

        if (result.status == TetMeshGenerationStatus::kNone) {
            const RustMeshValidationResult report = validate_native_mesh_with_rust(result.mesh);
            if (!report.ok) {
                result.status = TetMeshGenerationStatus::kInvalidMesh;
                result.detail = report.issues.front();
                result.mesh = NativeTetMeshDto{};
            } else {
                result.volume_mm3 = report.volume_mm3;
            }
        }
    } catch (const Standard_Failure& error) {
        result.status = TetMeshGenerationStatus::kInternalFailure;
        result.detail = error.what();
    } catch (const std::exception& error) {
        result.status = TetMeshGenerationStatus::kInternalFailure;
        result.detail = error.what();
    } catch (...) {
        result.status = TetMeshGenerationStatus::kInternalFailure;
        result.detail = "unknown exception during Netgen generation";
    }

    // 声明顺序保证先销毁借用 OCC 几何的 Netgen mesh，再释放几何对象。
    if (result.status != TetMeshGenerationStatus::kNone) {
        result.mesh = NativeTetMeshDto{};
        result.volume_mm3 = 0.0;
    }
    return result;
}

struct ShapeGenerationContext {
    const TetMeshGenerationParameters& parameters;
    TetMeshGenerationResult result;
};

void generate_shape_callback(const TopoDS_Shape& shape,
                             const panta::geometry::detail::ShapeFaceMap& source_faces,
                             void* context) {
    auto& generation = *static_cast<ShapeGenerationContext*>(context);
    generation.result = generate_for_shape(shape, source_faces, generation.parameters);
}

} // namespace

TetMeshGenerationResult generate_tet_mesh(panta::geometry::ShapeRef shape,
                                          const TetMeshGenerationParameters& parameters) {
    ShapeGenerationContext context{parameters, {}};
    const std::lock_guard<std::mutex> lock(netgen_mutex());
    try {
        init_netgen_once();
        const auto access =
            panta::geometry::detail::with_shape(shape, generate_shape_callback, &context);
        if (access == panta::geometry::ShapeRepositoryStatus::kNotFound) {
            context.result.status = TetMeshGenerationStatus::kShapeNotFound;
            context.result.detail = "shape handle was not found";
        } else if (access == panta::geometry::ShapeRepositoryStatus::kRevisionMismatch) {
            context.result.status = TetMeshGenerationStatus::kShapeRevisionMismatch;
            context.result.detail = "shape revision does not match";
        }
    } catch (const Standard_Failure& error) {
        context.result.status = TetMeshGenerationStatus::kInternalFailure;
        context.result.detail = error.what();
    } catch (const std::exception& error) {
        context.result.status = TetMeshGenerationStatus::kInternalFailure;
        context.result.detail = error.what();
    } catch (...) {
        context.result.status = TetMeshGenerationStatus::kInternalFailure;
        context.result.detail = "unknown exception during shape lookup or meshing";
    }
    return context.result;
}

} // namespace panta::mesh
