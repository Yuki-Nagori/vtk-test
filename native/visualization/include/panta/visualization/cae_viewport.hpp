/// CaeViewport：QML 中的 CAE 3D 视口（任务 007）。
///
/// 应用/QML 面：不含任何第三方渲染类型（standards/vtk.md——VTK 不是公共
/// API，后端可替换）。渲染经 ViewportBackend 适配层（V1 = VTK，实现见
/// src/vtk/），RenderScene 承载后端中立的 CPU 状态与场景修订。
/// 头文件保持自包含：QML 类型注册只解析本头，不依赖第三方头。
#pragma once

#include <QPointer>
#include <QQuickItem>
#include <memory>
#include <panta/visualization/mesh_source.hpp>

namespace panta::visualization {

class CaeViewport : public QQuickItem {
    Q_OBJECT
    QML_NAMED_ELEMENT(CaeViewport)
    Q_PROPERTY(QObject* meshSource READ meshSource WRITE setMeshSource NOTIFY meshSourceChanged)

  public:
    explicit CaeViewport(QQuickItem* parent = nullptr);
    ~CaeViewport() override;

    [[nodiscard]] QObject* meshSource() const;
    void setMeshSource(QObject* source);

  signals:
    /// 后端完成场景构建后通知 GUI；渲染错误经 qWarning 上报，不静默。
    void sceneReady();
    void meshSourceChanged();

  protected:
    void componentComplete() override;
    void geometryChange(const QRectF& new_geometry, const QRectF& old_geometry) override;

  private:
    void refresh_mesh();
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace panta::visualization
