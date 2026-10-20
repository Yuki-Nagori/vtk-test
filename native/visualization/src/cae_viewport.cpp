/// CaeViewport 实现：持有后端中立的 RenderScene，经 ViewportBackend
/// 适配层驱动渲染；第三方实现细节全部在 src/vtk/。
#include "mesh_source.hpp"
#include "vtk/vtk_viewport.hpp"
#include <QObject>
#include <QPointF>
#include <QPointer>
#include <QRectF>
#include <QtCore/qtmetamacros.h>
#include <algorithm>
#include <memory>
#include <panta/visualization/cae_viewport.hpp>
#include <panta/visualization/render_scene.hpp>
#include <panta/visualization/viewport_backend.hpp>

namespace panta::visualization {

struct CaeViewport::Impl {
    RenderScene scene;
    QPointer<MeshSource> mesh_source;
    QMetaObject::Connection mesh_connection;
    QMetaObject::Connection mesh_destroyed_connection;
    std::unique_ptr<ViewportBackend> backend;
};

CaeViewport::CaeViewport(QQuickItem* parent) : QQuickItem(parent), impl_(std::make_unique<Impl>()) {
    impl_->backend = create_vtk_viewport_backend();
    if (auto* item = impl_->backend->item()) {
        item->setParentItem(this);
        // 宿主条目铺满本条目且位置恒为原点；后续只随几何变化更新尺寸。
        item->setPosition(QPointF(0, 0));
        // 具体适配器类型仅在本 cpp（模块内部）可见，公共头与 QML 面保持
        // 后端中立。
        if (auto* viewport = qobject_cast<VtkViewport*>(item)) {
            QObject::connect(viewport, &VtkViewport::sceneInitialized, this,
                             &CaeViewport::sceneReady);
        }
    }
}

CaeViewport::~CaeViewport() = default;

QObject* CaeViewport::meshSource() const { return impl_->mesh_source; }

void CaeViewport::setMeshSource(QObject* source) {
    auto* mesh_source = qobject_cast<MeshSource*>(source);
    if (impl_->mesh_source == mesh_source) {
        return;
    }
    QObject::disconnect(impl_->mesh_connection);
    QObject::disconnect(impl_->mesh_destroyed_connection);
    impl_->mesh_source = mesh_source;
    if (mesh_source != nullptr) {
        impl_->mesh_connection =
            connect(mesh_source, &MeshSource::meshChanged, this, &CaeViewport::refresh_mesh);
        impl_->mesh_destroyed_connection = connect(mesh_source, &QObject::destroyed, this, [this] {
            refresh_mesh();
            emit meshSourceChanged();
        });
    }
    refresh_mesh();
    emit meshSourceChanged();
}

void CaeViewport::refresh_mesh() {
    impl_->scene.mesh = impl_->mesh_source ? impl_->mesh_source->mesh_snapshot() : nullptr;
    impl_->scene.primitive_visible = impl_->scene.mesh != nullptr;
    ++impl_->scene.revision;
    impl_->backend->apply_state(impl_->scene);
}

void CaeViewport::componentComplete() {
    QQuickItem::componentComplete();
    // 初次状态提交后，后端在 GUI 线程创建原生 surface 和 WebGPU 管线；
    // 构建完成经 sceneReady 通知 GUI（渲染错误走 qWarning，不静默）。
    impl_->scene.revision = std::max<SceneRevision>(1, impl_->scene.revision + 1);
    impl_->backend->apply_state(impl_->scene);
}

void CaeViewport::geometryChange(const QRectF& new_geometry, const QRectF& old_geometry) {
    QQuickItem::geometryChange(new_geometry, old_geometry);
    // 后端把逻辑坐标转换为平台 surface 坐标，并按设备像素比更新 VTK
    // render window；宿主条目位置已在构造时固定为原点。
    if (auto* item = impl_->backend->item()) {
        item->setSize(new_geometry.size());
    }
}

} // namespace panta::visualization
