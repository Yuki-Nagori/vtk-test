// 私有场景构建的 CPU 回归；不创建图形 surface 或提交 GPU 帧。
#include "primitive_scene.hpp"
#include <QObject>
#include <QtCore/qtmetamacros.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <cmath>
#include <memory>
#include <panta/visualization/mesh_source.hpp>
#include <vtkActor.h>
#include <vtkCamera.h>
#include <vtkMapper.h>
#include <vtkNew.h>
#include <vtkPolyData.h>
#include <vtkPolyDataMapper.h>
#include <vtkProperty.h>
#include <vtkType.h>
#include <vtkWeakPointer.h>
#include <vtkWebGPURenderer.h>

class PrimitiveSceneTest final : public QObject {
    Q_OBJECT
  private slots:
    void replaces_geometry_and_preserves_styles() {
        vtkNew<vtkActor> actor;
        panta::visualization::replace_primitive_geometry(actor, nullptr, {});
        QVERIFY(actor->GetMapper() == nullptr);
        const auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        mesh->vertices = {{{0, 0, 0}}, {{10, 0, 0}}, {{0, 20, 0}}};
        panta::visualization::replace_primitive_geometry(actor, nullptr, mesh);
        for (int axis = 0; axis < 3; ++axis) {
            QCOMPARE(actor->GetOrientation()[axis], 0.0);
        }
        QCOMPARE(actor->GetProperty()->GetColor()[0], 0.72);
        QCOMPARE(actor->GetProperty()->GetSpecularPower(), 28.0);
        double bounds[6];
        actor->GetBounds(bounds);
        QCOMPARE(bounds[1], 10.0);
        QCOMPARE(bounds[3], 20.0);
        auto* mapper = vtkPolyDataMapper::SafeDownCast(actor->GetMapper());
        QVERIFY(mapper != nullptr);
        QCOMPARE(mapper->GetInput()->GetNumberOfCells(), vtkIdType{1});
        QVERIFY(!mapper->GetScalarVisibility());
        vtkWeakPointer<vtkMapper> previous = mapper;
        panta::visualization::replace_primitive_geometry(actor, nullptr, {});
        QVERIFY(actor->GetMapper() == nullptr);
        QVERIFY(previous == nullptr);
    }
    void empty_snapshot_has_no_geometry_or_camera_reset() {
        vtkNew<vtkActor> actor;
        vtkNew<vtkWebGPURenderer> renderer;
        const auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        panta::visualization::replace_primitive_geometry(actor, nullptr, mesh);
        QVERIFY(actor->GetMapper() == nullptr);
        auto* camera = renderer->GetActiveCamera();
        camera->SetPosition(2, 3, 4);
        panta::visualization::configure_default_camera(renderer, actor, 1.0, 1.25);
        QCOMPARE(camera->GetPosition()[0], 2.0);
        QCOMPARE(camera->GetPosition()[1], 3.0);
        QCOMPARE(camera->GetPosition()[2], 4.0);
    }
    void fits_camera_to_aspect_and_margin() {
        vtkNew<vtkActor> actor;
        vtkNew<vtkWebGPURenderer> renderer;
        const auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        mesh->vertices = {{{0, 0, 0}}, {{10, 0, 0}}, {{0, 20, 0}}};
        panta::visualization::replace_primitive_geometry(actor, nullptr, mesh);
        renderer->AddActor(actor);
        for (double aspect : {1.0, 0.25}) {
            panta::visualization::configure_default_camera(renderer, actor, aspect, 1.25);
            auto* camera = renderer->GetActiveCamera();
            QCOMPARE(camera->GetFocalPoint()[0], 5.0);
            QCOMPARE(camera->GetFocalPoint()[1], 10.0);
            const double expected =
                1.25 * (aspect == 1.0 ? 10.0 : 20.0) / std::tan(0.2617993877991494);
            QVERIFY(std::abs(camera->GetPosition()[2] - expected) < 1e-9);
            QCOMPARE(camera->GetViewAngle(), 30.0);
        }
    }
};
QTEST_GUILESS_MAIN(PrimitiveSceneTest)
#include "primitive_scene_test.moc"
