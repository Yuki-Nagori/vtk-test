// 私有场景构建的 CPU 回归；不创建图形 surface 或提交 GPU 帧。
#include "primitive_scene.hpp"
#include <QObject>
#include <QtCore/qtmetamacros.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <cmath>
#include <memory>
#include <panta/visualization/mesh_source.hpp>
#include <string>
#include <vtkActor.h>
#include <vtkCamera.h>
#include <vtkColorTransferFunction.h>
#include <vtkDataArray.h>
#include <vtkMapper.h>
#include <vtkNew.h>
#include <vtkPointData.h>
#include <vtkPolyData.h>
#include <vtkPolyDataMapper.h>
#include <vtkProperty.h>
#include <vtkScalarBarActor.h>
#include <vtkScalarsToColors.h>
#include <vtkTextProperty.h>
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
    void field_scalars_color_sections_with_shared_stops() {
        vtkNew<vtkActor> actor;
        const auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        mesh->vertices = {{{0, 0, 0}}, {{1, 0, 0}}, {{0, 1, 0}}};
        mesh->scalars = {2.0F, std::nanf(""), 6.0F};
        mesh->scalar_min = 2.0;
        mesh->scalar_max = 6.0;
        panta::visualization::replace_primitive_geometry(actor, nullptr, mesh);
        auto* mapper = vtkPolyDataMapper::SafeDownCast(actor->GetMapper());
        QVERIFY(mapper != nullptr);
        QVERIFY(mapper->GetScalarVisibility());
        QCOMPARE(mapper->GetScalarRange()[0], 2.0);
        QCOMPARE(mapper->GetScalarRange()[1], 6.0);
        auto* colors = vtkColorTransferFunction::SafeDownCast(mapper->GetLookupTable());
        QVERIFY(colors != nullptr);
        QCOMPARE(colors->GetSize(), 3);
        double rgb[3];
        colors->GetColor(2.0, rgb);
        QVERIFY(std::abs(rgb[2] - panta::visualization::cool_to_warm_stops[0].blue) < 1e-9);
        colors->GetColor(6.0, rgb);
        QVERIFY(std::abs(rgb[0] - panta::visualization::cool_to_warm_stops[2].red) < 1e-9);
        colors->GetColor(std::nan(""), rgb);
        QCOMPARE(rgb[2], panta::visualization::field_color_maps[0].missing_color[2]);

        auto* scalars = mapper->GetInput()->GetPointData()->GetScalars();
        QVERIFY(scalars != nullptr);
        QCOMPARE(scalars->GetNumberOfTuples(), vtkIdType{3});
        QVERIFY(std::isnan(scalars->GetTuple1(1)));

        // 颜色图预设（task 039）：控制点数与端点、缺失色随预设变化；越界索引回退默认。
        {
            auto viridis = std::make_shared<panta::visualization::SurfaceMeshSnapshot>(*mesh);
            viridis->color_map = 1;
            panta::visualization::replace_primitive_geometry(actor, nullptr, viridis);
            auto* lut = vtkColorTransferFunction::SafeDownCast(
                vtkPolyDataMapper::SafeDownCast(actor->GetMapper())->GetLookupTable());
            QVERIFY(lut != nullptr);
            QCOMPARE(lut->GetSize(), 5);
            lut->GetColor(6.0, rgb);
            QVERIFY(std::abs(rgb[2] - panta::visualization::viridis_stops[4].blue) < 1e-9);
            lut->GetColor(std::nan(""), rgb);
            QCOMPARE(rgb[0], 1.0);
            QCOMPARE(rgb[1], 0.0);
            QCOMPARE(&panta::visualization::field_color_map(99),
                     &panta::visualization::field_color_maps[0]);
        }

        // 常值场扩成非零区间；倒置或非有限区间退回 [0, 1]。
        const auto constant = panta::visualization::field_display_range(4.0, 4.0);
        QVERIFY(constant[0] < 4.0 && constant[1] > 4.0);
        QCOMPARE(panta::visualization::field_display_range(3.0, 1.0)[1], 1.0);
        QCOMPARE(panta::visualization::field_display_range(std::nan(""), 1.0)[0], 0.0);

        // 标量数与顶点数不符时按单色显示。
        mesh->scalars.pop_back();
        panta::visualization::replace_primitive_geometry(actor, nullptr, mesh);
        mapper = vtkPolyDataMapper::SafeDownCast(actor->GetMapper());
        QVERIFY(mapper != nullptr && !mapper->GetScalarVisibility());
    }
    void scalar_bar_tracks_field_lookup_table_and_title() {
        using panta::visualization::scalar_bar_title;
        panta::visualization::SurfaceMeshSnapshot plain;
        QVERIFY(scalar_bar_title(plain).empty());
        plain.scalar_title = "温度";
        QCOMPARE(scalar_bar_title(plain), std::string("温度"));
        plain.scalar_units = "K";
        plain.scalar_range_label = "全部时间步";
        QCOMPARE(scalar_bar_title(plain), std::string("温度 (K)\n全部时间步"));

        vtkNew<vtkScalarBarActor> bar;
        panta::visualization::style_scalar_bar(bar);
        QVERIFY(!bar->GetVisibility());
        QCOMPARE(bar->GetNumberOfLabels(), 5);
        const std::string font = panta::visualization::scalar_bar_font_file();
        if (!font.empty()) {
            QCOMPARE(bar->GetTitleTextProperty()->GetFontFamily(), VTK_FONT_FILE);
            QCOMPARE(std::string(bar->GetLabelTextProperty()->GetFontFile()), font);
        }

        vtkNew<vtkActor> actor;
        const auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        mesh->vertices = {{{0, 0, 0}}, {{1, 0, 0}}, {{0, 1, 0}}};
        mesh->scalars = {2.0F, 4.0F, 6.0F};
        mesh->scalar_min = 2.0;
        mesh->scalar_max = 6.0;
        mesh->scalar_title = "压力";
        mesh->scalar_units = "Pa";
        mesh->scalar_range_label = "当前步";
        panta::visualization::replace_primitive_geometry(actor, nullptr, mesh);
        panta::visualization::sync_scalar_bar(bar, actor, mesh.get());
        QVERIFY(bar->GetVisibility());
        QVERIFY(bar->GetLookupTable() == actor->GetMapper()->GetLookupTable());
        QCOMPARE(bar->GetLookupTable()->GetRange()[0], 2.0);
        QCOMPARE(bar->GetLookupTable()->GetRange()[1], 6.0);
        QCOMPARE(std::string(bar->GetTitle()), std::string("压力 (Pa)\n当前步"));
        QVERIFY(!bar->GetDrawNanAnnotation());

        mesh->scalars[1] = std::nanf("");
        panta::visualization::sync_scalar_bar(bar, actor, mesh.get());
        QVERIFY(bar->GetDrawNanAnnotation());

        // 无场名、无标量或无网格时隐藏并释放查找表引用。
        auto untitled = std::make_shared<panta::visualization::SurfaceMeshSnapshot>(*mesh);
        untitled->scalar_title.clear();
        panta::visualization::replace_primitive_geometry(actor, nullptr, untitled);
        panta::visualization::sync_scalar_bar(bar, actor, untitled.get());
        QVERIFY(!bar->GetVisibility());
        QVERIFY(bar->GetLookupTable() == nullptr);
        auto geometry_only = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        geometry_only->vertices = mesh->vertices;
        geometry_only->scalar_title = "压力";
        panta::visualization::replace_primitive_geometry(actor, nullptr, geometry_only);
        panta::visualization::sync_scalar_bar(bar, actor, geometry_only.get());
        QVERIFY(!bar->GetVisibility());
        panta::visualization::replace_primitive_geometry(actor, nullptr, {});
        panta::visualization::sync_scalar_bar(bar, actor, nullptr);
        QVERIFY(!bar->GetVisibility());
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
