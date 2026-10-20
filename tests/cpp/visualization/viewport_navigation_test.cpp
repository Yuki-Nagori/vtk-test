// 视口导航的无窗口行为测试；1e-8 容差覆盖双精度姿态转换的舍入误差。
#include "navigation/viewport_camera.hpp"
#include "navigation/viewport_input.hpp"
#include <QtCore/qtmetamacros.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <array>
#include <cmath>
#include <vtkActor.h>
#include <vtkCamera.h>
#include <vtkCommand.h>
#include <vtkCubeSource.h>
#include <vtkNew.h>
#include <vtkPolyDataMapper.h>

class ViewportNavigationTest final : public QObject {
    Q_OBJECT

  private slots:
    void maps_viewport_buttons_to_separate_actions() {
        using Action = panta::visualization::ViewportInputAction;
        using panta::visualization::classify_viewport_input;
        QCOMPARE(classify_viewport_input(vtkCommand::LeftButtonPressEvent, false),
                 Action::PressCube);
        QCOMPARE(classify_viewport_input(vtkCommand::LeftButtonReleaseEvent, false),
                 Action::ReleaseCube);
        QCOMPARE(classify_viewport_input(vtkCommand::RightButtonPressEvent, false),
                 Action::BeginRotation);
        QCOMPARE(classify_viewport_input(vtkCommand::RightButtonReleaseEvent, true),
                 Action::EndRotation);
        QCOMPARE(classify_viewport_input(vtkCommand::MouseMoveEvent, false), Action::Ignore);
        QCOMPARE(classify_viewport_input(vtkCommand::MouseMoveEvent, true), Action::MoveRotation);
        QCOMPARE(classify_viewport_input(vtkCommand::MouseWheelForwardEvent, false),
                 Action::ZoomIn);
        QCOMPARE(classify_viewport_input(vtkCommand::MouseWheelBackwardEvent, false),
                 Action::ZoomOut);
    }

    void zoom_preserves_focus_direction_and_distance_bounds() {
        vtkNew<vtkCubeSource> cube;
        cube->SetXLength(2.0);
        cube->SetYLength(4.0);
        cube->SetZLength(6.0);
        vtkNew<vtkPolyDataMapper> mapper;
        mapper->SetInputConnection(cube->GetOutputPort());
        vtkNew<vtkActor> actor;
        actor->SetMapper(mapper);

        vtkNew<vtkCamera> camera;
        camera->SetPosition(4.0, 6.0, 15.0);
        camera->SetFocalPoint(1.0, 2.0, 3.0);
        camera->SetViewUp(0.0, 1.0, 0.0);
        const std::array<double, 3> direction = {3.0 / 13.0, 4.0 / 13.0, 12.0 / 13.0};
        const double diagonal = std::sqrt(56.0);
        const double minimum_distance = diagonal * 0.02;
        const double maximum_distance = diagonal * 100.0;
        const auto verify_camera = [&] {
            double focal_point[3];
            camera->GetFocalPoint(focal_point);
            QCOMPARE(focal_point[0], 1.0);
            QCOMPARE(focal_point[1], 2.0);
            QCOMPARE(focal_point[2], 3.0);
            double position[3];
            camera->GetPosition(position);
            const double distance = camera->GetDistance();
            QVERIFY(distance >= minimum_distance - 1e-8);
            QVERIFY(distance <= maximum_distance + 1e-8);
            QVERIFY(std::abs((position[0] - focal_point[0]) / distance - direction[0]) < 1e-8);
            QVERIFY(std::abs((position[1] - focal_point[1]) / distance - direction[1]) < 1e-8);
            QVERIFY(std::abs((position[2] - focal_point[2]) / distance - direction[2]) < 1e-8);
        };

        for (int i = 0; i < 1000; ++i) {
            panta::visualization::ViewportCamera::zoom(camera, actor, false);
        }
        verify_camera();
        QVERIFY(std::abs(camera->GetDistance() - maximum_distance) < 1e-8);
        for (int i = 0; i < 1000; ++i) {
            panta::visualization::ViewportCamera::zoom(camera, actor, true);
        }
        verify_camera();
        QVERIFY(std::abs(camera->GetDistance() - minimum_distance) < 1e-8);
    }

    void interpolates_camera_pose_and_retargets_without_a_jump() {
        vtkNew<vtkCubeSource> cube;
        vtkNew<vtkPolyDataMapper> mapper;
        mapper->SetInputConnection(cube->GetOutputPort());
        vtkNew<vtkActor> actor;
        actor->SetMapper(mapper);
        vtkNew<vtkCamera> camera;
        camera->SetPosition(0.0, 0.0, 10.0);
        camera->SetFocalPoint(0.0, 0.0, 0.0);
        camera->SetViewUp(0.0, 1.0, 0.0);

        const panta::visualization::CameraFrame right_frame{{1.0, 0.0, 0.0}, {0.0, 0.0, 1.0}};
        const std::array<double, 3> target_focal = {2.0, 0.0, 0.0};
        panta::visualization::ViewportCameraTransition transition;
        QVERIFY(transition.start(camera, right_frame, target_focal, actor));
        const auto original = panta::visualization::ViewportCamera::capture(camera);

        panta::visualization::ViewportCamera::apply(camera, transition.pose_at(0.0, 260.0));
        double position[3];
        camera->GetPosition(position);
        QVERIFY(std::abs(position[0]) < 1e-8);
        QVERIFY(std::abs(position[1]) < 1e-8);
        QVERIFY(std::abs(position[2] - 10.0) < 1e-8);

        const auto middle = transition.pose_at(130.0, 260.0);
        QCOMPARE(middle.focal_point[0], 1.0);
        QCOMPARE(middle.focal_point[1], 0.0);
        QCOMPARE(middle.focal_point[2], 0.0);
        QCOMPARE(middle.distance, original.distance);
        panta::visualization::ViewportCamera::apply(camera, middle);
        camera->GetPosition(position);
        QVERIFY(std::abs(camera->GetDistance() - original.distance) < 1e-8);
        QVERIFY(position[0] > middle.focal_point[0]);
        QVERIFY(position[2] > middle.focal_point[2]);

        const auto first_endpoint = transition.pose_at(260.0, 260.0);
        vtkNew<vtkCamera> endpoint_camera;
        panta::visualization::ViewportCamera::apply(endpoint_camera, first_endpoint);
        double focal_point[3];
        endpoint_camera->GetFocalPoint(focal_point);
        QCOMPARE(focal_point[0], 2.0);
        QCOMPARE(focal_point[1], 0.0);
        QCOMPARE(focal_point[2], 0.0);
        endpoint_camera->GetPosition(position);
        QVERIFY(std::abs(position[0] - (2.0 + original.distance)) < 1e-8);
        QVERIFY(std::abs(position[1]) < 1e-8);
        QVERIFY(std::abs(position[2]) < 1e-8);

        // 新目标在旧过渡中途到来时，零时刻必须与屏幕上当前相机姿态重合。
        const panta::visualization::CameraFrame back_frame{{0.0, 0.0, -1.0}, {0.0, 0.0, 1.0}};
        const std::array<double, 3> new_focal = {0.0, 3.0, 0.0};
        QVERIFY(transition.start(camera, back_frame, new_focal, actor));
        const auto retarget_start = panta::visualization::ViewportCamera::capture(camera);
        panta::visualization::ViewportCamera::apply(camera, transition.pose_at(0.0, 260.0));
        camera->GetPosition(position);
        vtkNew<vtkCamera> expected_camera;
        panta::visualization::ViewportCamera::apply(expected_camera, retarget_start);
        double expected_position[3];
        expected_camera->GetPosition(expected_position);
        for (int axis = 0; axis < 3; ++axis) {
            QVERIFY(std::abs(position[axis] - expected_position[axis]) < 1e-8);
        }
        camera->GetFocalPoint(focal_point);
        for (int axis = 0; axis < 3; ++axis) {
            QCOMPARE(focal_point[axis], retarget_start.focal_point[axis]);
        }
        QVERIFY(transition.active());
        transition.cancel();
        QVERIFY(!transition.active());
    }

    void reaches_all_six_directions_without_crossing_the_focus() {
        using panta::visualization::CameraFrame;
        using panta::visualization::ViewportCamera;
        const std::array frames = {
            CameraFrame{{1, 0, 0}, {0, 0, 1}}, CameraFrame{{-1, 0, 0}, {0, 0, 1}},
            CameraFrame{{0, 1, 0}, {0, 0, 1}}, CameraFrame{{0, -1, 0}, {0, 0, 1}},
            CameraFrame{{0, 0, 1}, {0, 1, 0}}, CameraFrame{{0, 0, -1}, {0, 1, 0}}};
        vtkNew<vtkCubeSource> cube;
        vtkNew<vtkPolyDataMapper> mapper;
        mapper->SetInputConnection(cube->GetOutputPort());
        vtkNew<vtkActor> actor;
        actor->SetMapper(mapper);
        for (const auto& frame : frames) {
            vtkNew<vtkCamera> camera;
            camera->SetPosition(0, 0, 10);
            const auto start = ViewportCamera::capture(camera);
            const auto target = ViewportCamera::make_target(frame, {0, 0, 0}, 10);
            for (int step = 0; step <= 20; ++step) {
                ViewportCamera::apply(camera,
                                      ViewportCamera::interpolate(start, target, step / 20.0));
                QVERIFY(std::abs(camera->GetDistance() - 10) < 1e-8);
            }
            for (int axis = 0; axis < 3; ++axis) {
                QVERIFY(std::abs(camera->GetPosition()[axis] - frame.backward[axis] * 10) < 1e-8);
                QVERIFY(std::abs(camera->GetViewUp()[axis] - frame.view_up[axis]) < 1e-8);
            }
            panta::visualization::ViewportCameraTransition transition;
            QVERIFY(!transition.start(camera, frame, {0, 0, 0}, actor));
            QVERIFY(!transition.active());
        }
    }

    void keeps_zoom_finite_for_degenerate_geometry() {
        vtkNew<vtkCubeSource> cube;
        cube->SetXLength(0);
        cube->SetYLength(0);
        cube->SetZLength(0);
        vtkNew<vtkPolyDataMapper> mapper;
        mapper->SetInputConnection(cube->GetOutputPort());
        vtkNew<vtkActor> actor;
        actor->SetMapper(mapper);
        vtkNew<vtkCamera> camera;
        for (const bool zoom_in : {true, false}) {
            for (int step = 0; step < 1000; ++step) {
                panta::visualization::ViewportCamera::zoom(camera, actor, zoom_in);
            }
            QVERIFY(std::isfinite(camera->GetDistance()));
            QVERIFY(camera->GetDistance() > 0);
        }
    }
};

QTEST_APPLESS_MAIN(ViewportNavigationTest)
#include "viewport_navigation_test.moc"
