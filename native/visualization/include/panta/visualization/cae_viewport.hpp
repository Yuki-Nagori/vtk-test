/// CaeViewport：QML 中的 CAE 3D 视口（任务 007）。
///
/// 应用/QML 面：不含任何第三方渲染类型（standards/vtk.md——VTK 不是公共
/// API，后端可替换）。渲染经 ViewportBackend 适配层（V1 = VTK，实现见
/// src/vtk/），RenderScene 承载后端中立的 CPU 状态与场景修订。
/// 头文件保持自包含：QML 类型注册只解析本头，不依赖第三方头。
#pragma once

#include <QPointer>
#include <QQuickItem>
#include <QString>
#include <QUrl>
#include <memory>
#include <panta/visualization/mesh_source.hpp>

namespace panta::visualization {

class CaeViewport : public QQuickItem {
    Q_OBJECT
    QML_NAMED_ELEMENT(CaeViewport)
    Q_PROPERTY(QObject* meshSource READ meshSource WRITE setMeshSource NOTIFY meshSourceChanged)
    /// 模型显示开关（task 037 可见性眼睛）：只影响显示，不改网格来源。
    Q_PROPERTY(bool meshVisible READ meshVisible WRITE setMeshVisible NOTIFY meshVisibleChanged)

  public:
    explicit CaeViewport(QQuickItem* parent = nullptr);
    ~CaeViewport() override;

    [[nodiscard]] QObject* meshSource() const;
    void setMeshSource(QObject* source);
    [[nodiscard]] bool meshVisible() const;
    void setMeshVisible(bool visible);

    /// 把当前视口帧保存为 PNG（原子替换目标文件）；返回中文错误文案，空串表示成功。
    Q_INVOKABLE QString saveScreenshot(const QUrl& url);

    /// 平滑切换到预设视角：front/back/left/right/top/bottom/iso（task 017）。
    Q_INVOKABLE bool setViewPreset(const QString& name);
    /// 恢复默认相机并适配模型。
    Q_INVOKABLE void resetView();

  signals:
    /// 后端完成场景构建后通知 GUI；渲染错误经 qWarning 上报，不静默。
    void sceneReady();
    void meshSourceChanged();
    void meshVisibleChanged();
    /// 左键单击模型区域的拾取结果；x/y/z 为双精度世界坐标（工程单位 mm），未命中 hit=false。
    void pointPicked(bool hit, double x, double y, double z, quint64 triangle);

  protected:
    void componentComplete() override;
    void geometryChange(const QRectF& new_geometry, const QRectF& old_geometry) override;

  private:
    void refresh_mesh();
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace panta::visualization
