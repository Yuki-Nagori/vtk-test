#include "../../support/qt/frame_submission_capture.hpp"
/// Panta.Visualization 运行时加载测试（任务 007）：验证静态模块注册与
/// CaeViewport 类型可创建。
///
/// 默认验证无头模块加载，并在 offscreen 平台确认缺少原生 surface 时只告警一次。
/// PANTA_TEST_NATIVE_VIEWPORT=1 时在真实桌面验证帧合并、隐藏恢复、跨窗口重建、析构，
/// 以及 WebGPU adapter 不可用时的失败清理。生命周期用例不能使用 offscreen 平台。

#include "panta/visualization/mesh_source.hpp"
#include <QGuiApplication>
#include <QLoggingCategory>
#include <QObject>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QString>
#include <QStringList>
#include <QUrl>
#include <QtCore/qtmetamacros.h>
#include <QtLogging>
#include <QtQml/qqmlextensionplugin.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <cstdio>
#include <cstring>
#include <memory>
#include <panta/visualization/cae_viewport.hpp>
#include <panta/visualization/render_scene.hpp>
#include <qtenvironmentvariables.h>
#include <qtestsupport_core.h>
#include <utility>
#include <vector>
#include <vtk_viewport.hpp>

Q_IMPORT_QML_PLUGIN(Panta_VisualizationPlugin)

namespace {
std::shared_ptr<const panta::visualization::SurfaceMeshSnapshot> triangle_snapshot() {
    auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
    mesh->vertices = {{{0.0, 0.0, 0.0}, {1.0, 0.0, 0.0}, {0.0, 1.0, 0.0}}};
    return mesh;
}
} // namespace

class TestMeshSource final : public panta::visualization::MeshSource {
  public:
    using MeshSource::MeshSource;

    std::shared_ptr<const panta::visualization::SurfaceMeshSnapshot> snapshot;

    [[nodiscard]] auto mesh_snapshot() const
        -> std::shared_ptr<const panta::visualization::SurfaceMeshSnapshot> override {
        return snapshot;
    }
};

namespace panta::visualization {
class ViewportTestAccess final {
  public:
    static const void* actor_identity(const VtkViewport& viewport) {
        return viewport.test_actor_identity();
    }
    static bool has_mapper(const VtkViewport& viewport) { return viewport.test_has_mapper(); }
    static bool previous_mapper_released(const VtkViewport& viewport) {
        return viewport.test_previous_mapper_released();
    }
    static bool interaction_observers_registered(const VtkViewport& viewport) {
        return viewport.test_interaction_observers_registered();
    }
    static bool previous_window_resources_released(const VtkViewport& viewport) {
        return viewport.test_previous_window_resources_released();
    }
    static bool camera_transition_active(const VtkViewport& viewport) {
        return viewport.test_camera_transition_active();
    }
    static bool camera_transition_timer_active(const VtkViewport& viewport) {
        return viewport.test_camera_transition_timer_active();
    }
    static QSize applied_pixel_size(const VtkViewport& viewport) {
        return viewport.test_applied_pixel_size();
    }
    static void start_camera_transition(VtkViewport& viewport) {
        viewport.test_start_camera_transition();
    }
    static bool render_resources_absent(const VtkViewport& viewport) {
        return viewport.test_render_resources_absent();
    }
    static void fail_webgpu_device_initialization(VtkViewport& viewport) {
        viewport.test_fail_webgpu_device_initialization();
    }
};
} // namespace panta::visualization

class WarningCapture final {
  public:
    WarningCapture() {
        previous_active_ = active_;
        active_ = this;
        if (previous_active_ == nullptr) {
            previous_handler_ = qInstallMessageHandler(&WarningCapture::handle);
        }
    }
    ~WarningCapture() {
        active_ = previous_active_;
        if (active_ == nullptr) {
            qInstallMessageHandler(previous_handler_);
        }
    }
    WarningCapture(const WarningCapture&) = delete;
    WarningCapture& operator=(const WarningCapture&) = delete;

    int count_containing(const QString& fragment) const {
        int count = 0;
        for (const QString& message : messages_) {
            if (message.contains(fragment)) {
                ++count;
            }
        }
        return count;
    }

  private:
    static void handle(QtMsgType type, const QMessageLogContext& context, const QString& message) {
        if (type == QtWarningMsg && active_ != nullptr) {
            active_->messages_.append(message);
        }
        if (previous_handler_ != nullptr) {
            previous_handler_(type, context, message);
        }
    }

    static WarningCapture* active_;
    static QtMessageHandler previous_handler_;
    WarningCapture* previous_active_ = nullptr;
    QStringList messages_;
};

WarningCapture* WarningCapture::active_ = nullptr;
QtMessageHandler WarningCapture::previous_handler_ = nullptr;

class ViewportModuleLoadTest final : public QObject {
    Q_OBJECT

  private slots:
    void creates_cae_viewport() {
        QQmlEngine engine;
        QQmlComponent component(&engine);
        component.setData("import Panta.Visualization\nCaeViewport {}\n",
                          QUrl(QStringLiteral("qrc:///qt/qml/panta-tests/viewport-load/main.qml")));
        QVERIFY2(component.isReady(), component.errorString().toUtf8().constData());
        std::unique_ptr<QObject> object(component.create());
        QVERIFY2(object != nullptr, "CaeViewport 创建失败");
    }

    void mesh_source_destruction_clears_scene_and_notifies() {
        panta::visualization::CaeViewport viewport;
        QSignalSpy source_changed(&viewport, &panta::visualization::CaeViewport::meshSourceChanged);
        QVERIFY(source_changed.isValid());

        auto snapshot = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        const std::weak_ptr<const panta::visualization::SurfaceMeshSnapshot> snapshot_lifetime =
            snapshot;
        auto source = std::make_unique<TestMeshSource>();
        source->snapshot = std::move(snapshot);
        viewport.setMeshSource(source.get());
        QCOMPARE(source_changed.count(), 1);
        QVERIFY(!snapshot_lifetime.expired());

        source.reset();
        QVERIFY(viewport.meshSource() == nullptr);
        QCOMPARE(source_changed.count(), 2);
        QTRY_VERIFY_WITH_TIMEOUT(snapshot_lifetime.expired(), 1000);

        auto replaced_source = std::make_unique<TestMeshSource>();
        auto current_source = std::make_unique<TestMeshSource>();
        viewport.setMeshSource(replaced_source.get());
        viewport.setMeshSource(current_source.get());
        const qsizetype notification_count = source_changed.count();
        replaced_source.reset();
        QCoreApplication::processEvents();
        QCOMPARE(viewport.meshSource(), static_cast<QObject*>(current_source.get()));
        QCOMPARE(source_changed.count(), notification_count);

        current_source.reset();
        QVERIFY(viewport.meshSource() == nullptr);
        QCOMPARE(source_changed.count(), notification_count + 1);
    }

    void native_refresh_and_window_lifecycle() {
        if (!qEnvironmentVariableIsSet("PANTA_TEST_NATIVE_VIEWPORT")) {
            QSKIP("Native GPU lifecycle requires PANTA_TEST_NATIVE_VIEWPORT=1 and a real desktop");
        }
        panta::test::FrameSubmissionCapture capture;
        QQuickWindow first;
        QQuickWindow second;
        first.resize(480, 360);
        second.resize(480, 360);
        QQuickItem parent(first.contentItem());
        panta::visualization::VtkViewport viewport(&parent);
        viewport.setSize(QSizeF(400, 300));
        QSignalSpy initialized(&viewport, &panta::visualization::VtkViewport::sceneInitialized);
        // 先消费尚未显示时的刷新，验证构造时已有窗口归属仍能监听后续 show。
        QCoreApplication::processEvents();
        first.show();
        QTRY_COMPARE_WITH_TIMEOUT(initialized.count(), 1, 10000);
        QVERIFY(!panta::visualization::ViewportTestAccess::has_mapper(viewport));
        QVERIFY(panta::visualization::ViewportTestAccess::actor_identity(viewport) == nullptr);
        const qreal dpr = first.devicePixelRatio();
        qInfo() << "Native viewport test DPR:" << dpr;
        const QByteArray expected_dpr = qgetenv("PANTA_TEST_EXPECTED_DPR");
        if (!expected_dpr.isEmpty()) {
            QCOMPARE(dpr, expected_dpr.toDouble());
        }
        QCOMPARE(panta::visualization::ViewportTestAccess::applied_pixel_size(viewport),
                 QSize(qRound(viewport.width() * dpr), qRound(viewport.height() * dpr)));
        QTest::qWait(100);
        capture.reset();
        panta::visualization::RenderScene scene;
        for (int i = 0; i < 100; ++i) {
            scene.revision = i + 1;
            scene.background = QColor(i, 80, 160);
            viewport.apply_state(scene);
            viewport.setWidth(400 + i);
        }
        QTest::qWait(100);
        QCOMPARE(capture.count(), 1);

        auto snapshot = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        snapshot->vertices = {{{0.0, 0.0, 0.0}, {1.0, 0.0, 0.0}, {0.0, 1.0, 0.0}}};
        const std::weak_ptr<const panta::visualization::SurfaceMeshSnapshot> snapshot_lifetime =
            snapshot;
        scene.mesh = snapshot;
        snapshot.reset();
        ++scene.revision;
        capture.reset();
        viewport.apply_state(scene);
        QTRY_VERIFY_WITH_TIMEOUT(capture.count() > 0, 10000);
        QVERIFY(!snapshot_lifetime.expired());
        const void* actor_identity =
            panta::visualization::ViewportTestAccess::actor_identity(viewport);
        QVERIFY(actor_identity != nullptr);
        QVERIFY(panta::visualization::ViewportTestAccess::has_mapper(viewport));
        QVERIFY(
            panta::visualization::ViewportTestAccess::interaction_observers_registered(viewport));

        std::vector<std::weak_ptr<const panta::visualization::SurfaceMeshSnapshot>> old_snapshots;
        old_snapshots.push_back(snapshot_lifetime);
        for (int cycle = 0; cycle < 12; ++cycle) {
            auto next = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
            const double x = static_cast<double>(cycle + 1);
            next->vertices = {{{x, 0.0, 0.0}, {x + 1.0, 0.0, 0.0}, {x, 1.0, 0.0}}};
            old_snapshots.emplace_back(next);
            scene.mesh = std::move(next);
            ++scene.revision;
            capture.reset();
            viewport.apply_state(scene);
            QTRY_VERIFY_WITH_TIMEOUT(capture.count() > 0, 10000);
            QCOMPARE(panta::visualization::ViewportTestAccess::actor_identity(viewport),
                     actor_identity);
            QVERIFY(panta::visualization::ViewportTestAccess::previous_mapper_released(viewport));
            QVERIFY(panta::visualization::ViewportTestAccess::has_mapper(viewport));
            QVERIFY(panta::visualization::ViewportTestAccess::interaction_observers_registered(
                viewport));
            for (std::size_t index = 0; index + 1 < old_snapshots.size(); ++index) {
                QVERIFY(old_snapshots[index].expired());
            }
        }

        panta::visualization::ViewportTestAccess::start_camera_transition(viewport);
        QVERIFY(panta::visualization::ViewportTestAccess::camera_transition_active(viewport));
        QVERIFY(panta::visualization::ViewportTestAccess::camera_transition_timer_active(viewport));
        viewport.setVisible(false);
        QVERIFY(!panta::visualization::ViewportTestAccess::camera_transition_active(viewport));
        QVERIFY(
            !panta::visualization::ViewportTestAccess::camera_transition_timer_active(viewport));
        scene.mesh.reset();
        ++scene.revision;
        viewport.apply_state(scene);
        QTRY_VERIFY_WITH_TIMEOUT(snapshot_lifetime.expired(), 5000);
        QVERIFY(panta::visualization::ViewportTestAccess::previous_mapper_released(viewport));
        capture.reset();
        viewport.setVisible(true);
        QTRY_VERIFY_WITH_TIMEOUT(capture.count() > 0, 10000);
        QVERIFY(!panta::visualization::ViewportTestAccess::has_mapper(viewport));

        scene.mesh = triangle_snapshot();
        ++scene.revision;
        viewport.apply_state(scene);
        QTRY_VERIFY_WITH_TIMEOUT(panta::visualization::ViewportTestAccess::has_mapper(viewport),
                                 10000);
        QTest::qWait(100);
        capture.reset();
        panta::visualization::ViewportTestAccess::start_camera_transition(viewport);
        first.showMinimized();
        QTRY_VERIFY_WITH_TIMEOUT(first.visibility() == QWindow::Minimized, 10000);
        QVERIFY(!panta::visualization::ViewportTestAccess::camera_transition_active(viewport));
        QVERIFY(
            !panta::visualization::ViewportTestAccess::camera_transition_timer_active(viewport));
        QTest::qWait(100);
        QCOMPARE(capture.count(), 0);
        first.showNormal();
        QTRY_VERIFY_WITH_TIMEOUT(capture.count() > 0, 10000);

        capture.reset();
        viewport.apply_state(scene);
        ++scene.revision;
        viewport.apply_state(scene);
        parent.setPosition(QPointF(20, 10));
        QTest::qWait(100);
        QCOMPARE(capture.count(), 0);
        QCOMPARE(capture.surface_sync_count(), 1);

        viewport.setVisible(false);
        scene.mesh.reset();
        ++scene.revision;
        viewport.apply_state(scene);
        QTest::qWait(100);
        QCOMPARE(capture.count(), 0);
        viewport.setVisible(true);
        QTest::qWait(100);
        QCOMPARE(capture.count(), 1);

        capture.reset();
        const qreal width = viewport.width();
        viewport.setWidth(0);
        QTest::qWait(100);
        QCOMPARE(capture.count(), 0);
        viewport.setWidth(width);
        QTest::qWait(100);
        QCOMPARE(capture.count(), 1);

        scene.mesh = triangle_snapshot();
        ++scene.revision;
        viewport.apply_state(scene);
        QTRY_VERIFY_WITH_TIMEOUT(panta::visualization::ViewportTestAccess::has_mapper(viewport),
                                 10000);
        panta::visualization::ViewportTestAccess::start_camera_transition(viewport);
        QVERIFY(panta::visualization::ViewportTestAccess::camera_transition_active(viewport));
        QVERIFY(panta::visualization::ViewportTestAccess::camera_transition_timer_active(viewport));
        second.show();
        parent.setParentItem(second.contentItem());
        QVERIFY(!panta::visualization::ViewportTestAccess::camera_transition_active(viewport));
        QVERIFY(
            !panta::visualization::ViewportTestAccess::camera_transition_timer_active(viewport));
        QTRY_COMPARE_WITH_TIMEOUT(initialized.count(), 2, 10000);
        QVERIFY(
            panta::visualization::ViewportTestAccess::previous_window_resources_released(viewport));
        // 原窗口关闭后新宿主仍可更新；排队刷新在条目析构后不会再执行。
        first.close();
        viewport.setWidth(420);
    }

    void reports_missing_native_surface_once() {
        if (QGuiApplication::platformName() != QStringLiteral("offscreen")) {
            QSKIP("Missing native surface is reported on the offscreen platform");
        }
        WarningCapture warnings;
        QQuickWindow window;
        window.resize(320, 240);
        QQuickItem parent(window.contentItem());
        panta::visualization::VtkViewport viewport(&parent);
        viewport.setSize(QSizeF(200, 150));
        QSignalSpy initialized(&viewport, &panta::visualization::VtkViewport::sceneInitialized);
        window.show();
        QVERIFY(window.isVisible());
        viewport.apply_state({});
        QCoreApplication::processEvents();
        viewport.apply_state({});
        QCoreApplication::processEvents();
        QCOMPARE(initialized.count(), 0);
        QCOMPARE(warnings.count_containing(
                     QStringLiteral("offscreen 平台没有原生 surface，跳过 WebGPU 视口初始化")),
                 1);
        QVERIFY(panta::visualization::ViewportTestAccess::render_resources_absent(viewport));
    }

    void reports_webgpu_device_initialization_failure() {
        if (!qEnvironmentVariableIsSet("PANTA_TEST_NATIVE_VIEWPORT")) {
            QSKIP("WebGPU device failure requires PANTA_TEST_NATIVE_VIEWPORT=1 and a real desktop");
        }
        if (QGuiApplication::platformName() == QStringLiteral("offscreen")) {
            QSKIP("WebGPU device failure requires a native surface platform");
        }
        WarningCapture warnings;
        QQuickWindow window;
        window.resize(320, 240);
        QQuickItem parent(window.contentItem());
        panta::visualization::VtkViewport viewport(&parent);
        panta::visualization::ViewportTestAccess::fail_webgpu_device_initialization(viewport);
        viewport.setSize(QSizeF(200, 150));
        QSignalSpy initialized(&viewport, &panta::visualization::VtkViewport::sceneInitialized);
        window.show();
        QVERIFY(window.isVisible());
        QTRY_COMPARE_WITH_TIMEOUT(
            warnings.count_containing(QStringLiteral("VTK WebGPU device 初始化失败")), 1, 10000);
        viewport.apply_state({});
        QCoreApplication::processEvents();
        QCOMPARE(initialized.count(), 0);
        QCOMPARE(warnings.count_containing(QStringLiteral("VTK WebGPU device 初始化失败")), 1);
        QVERIFY(panta::visualization::ViewportTestAccess::render_resources_absent(viewport));
        QVERIFY(
            panta::visualization::ViewportTestAccess::previous_window_resources_released(viewport));
    }

    void destroys_attached_viewport_with_pending_refresh() {
        QQuickWindow window;
        window.resize(320, 240);
        {
            panta::visualization::VtkViewport viewport(window.contentItem());
            viewport.setSize(QSizeF(200, 100));
            panta::visualization::RenderScene initial_scene;
            initial_scene.mesh = triangle_snapshot();
            viewport.apply_state(initial_scene);
            if (qEnvironmentVariableIsSet("PANTA_TEST_NATIVE_VIEWPORT")) {
                QSignalSpy initialized(&viewport,
                                       &panta::visualization::VtkViewport::sceneInitialized);
                window.show();
                QTRY_COMPARE_WITH_TIMEOUT(initialized.count(), 1, 10000);
                panta::visualization::ViewportTestAccess::start_camera_transition(viewport);
                QVERIFY(
                    panta::visualization::ViewportTestAccess::camera_transition_active(viewport));
                QVERIFY(panta::visualization::ViewportTestAccess::camera_transition_timer_active(
                    viewport));
            }
            panta::visualization::RenderScene changed_scene;
            changed_scene.revision = 1;
            viewport.apply_state(changed_scene);
        }
        // 基类析构的 windowChanged 与待执行的刷新都不得访问已释放的 Impl。
        QTest::qWait(50);
    }
};

QTEST_MAIN(ViewportModuleLoadTest)
#include "viewport_module_load_test.moc"
