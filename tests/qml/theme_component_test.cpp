// 保留控件、工程创建窗口与文档页签的行为回归。
#include "../support/qml/quick_item_helpers.hpp"
#if defined(PANTA_TEST_WITH_BRIDGE)
#include "project_view_model.hpp"
#endif
#include <QColor>
#include <QDir>
#include <QFont>
#include <QImage>
#include <QImageReader>
#include <QObject>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickImageProvider>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QRect>
#include <QSignalSpy>
#include <QString>
#include <QStringList>
#include <QTemporaryDir>
#include <QUrl>
#include <QVariantMap>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qnamespace.h>
#include <QtCore/qobjectdefs.h>
#include <QtCore/qtmetamacros.h>
#include <QtQml/qqmlextensionplugin.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <icon_provider.hpp>
#include <qaccessible.h>
#include <qaccessible_base.h>
#include <qpoint.h>
#include <qtestkeyboard.h>
#include <qtestmouse.h>
#include <qtestsupport_gui.h>

Q_IMPORT_QML_PLUGIN(Panta_ShellPlugin)
#ifdef PANTA_TEST_WITH_BRIDGE
Q_IMPORT_QML_PLUGIN(Panta_BridgePlugin)
#endif

namespace {

QObject* create_component(QQmlEngine& engine, const QString& path, QObject& owner) {
    QQmlComponent component(&engine, QUrl(path));
    if (!component.isReady()) {
        QTest::qFail(qPrintable(component.errorString()), __FILE__, __LINE__);
        return nullptr;
    }
    QObject* object = component.create();
    if (object == nullptr) {
        QTest::qFail(qPrintable(component.errorString()), __FILE__, __LINE__);
        return nullptr;
    }
    object->setParent(&owner);
    return object;
}

} // namespace

class ThemeComponentTest final : public QObject {
    Q_OBJECT

  private slots:
    void defaults_use_theme() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        QObject* label = create_component(
            engine, QStringLiteral("qrc:/qt/qml/Panta/Shell/Components/Atoms/ThemedLabel.qml"),
            owner);
        QVERIFY(label != nullptr);
        QCOMPARE(label->property("textColor").value<QColor>(), QColor(QStringLiteral("#1c1c1c")));
        QCOMPARE(label->property("textSize").toInt(), 13);

        QObject* button = create_component(
            engine, QStringLiteral("qrc:/qt/qml/Panta/Shell/Components/Atoms/ThemedToolButton.qml"),
            owner);
        QVERIFY(button != nullptr);
        QCOMPARE(button->property("controlHeight").toInt(), 24);
        QCOMPARE(button->property("contentPadding").toInt(), 4);
        QCOMPARE(button->property("contentColor").value<QColor>(),
                 QColor(QStringLiteral("#4a4a4a")));
        QCOMPARE(button->property("implicitHeight").toInt(), 24);

        QObject* surface = create_component(
            engine, QStringLiteral("qrc:/qt/qml/Panta/Shell/Components/Atoms/PanelSurface.qml"),
            owner);
        QVERIFY(surface != nullptr);
        QCOMPARE(surface->property("surfaceColor").value<QColor>(),
                 QColor(QStringLiteral("#ffffff")));
    }

    void explicit_values_override_theme() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        QObject* button = create_component(
            engine, QStringLiteral("qrc:/qt/qml/Panta/Shell/Components/Atoms/ThemedToolButton.qml"),
            owner);
        QVERIFY(button != nullptr);
        button->setProperty("controlHeight", 48);
        button->setProperty("contentPadding", 20);
        QCOMPARE(button->property("controlHeight").toInt(), 48);
        QCOMPARE(button->property("contentPadding").toInt(), 20);
        QCOMPARE(button->property("implicitHeight").toInt(), 48);
        QCOMPARE(button->property("leftPadding").toInt(), 20);
        QCOMPARE(button->property("rightPadding").toInt(), 20);
        // 未覆盖属性仍取 Theme 默认：显式覆盖不得破坏其余绑定。
        QCOMPARE(button->property("contentColor").value<QColor>(),
                 QColor(QStringLiteral("#4a4a4a")));
    }

    void disabled_state_is_visibly_weakened() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        QObject* button = create_component(
            engine, QStringLiteral("qrc:/qt/qml/Panta/Shell/Components/Atoms/ThemedToolButton.qml"),
            owner);
        QVERIFY(button != nullptr);
        QCOMPARE(button->property("opacity").toDouble(), 1.0);
        button->setProperty("enabled", false);
        QCOMPARE(button->property("opacity").toDouble(), 0.4);
    }

    void new_project_dialog_waits_for_creation_and_preserves_failed_input() {
#if !defined(PANTA_TEST_WITH_BRIDGE)
        QSKIP("Project commands require the enabled Bridge module");
#else
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        panta::bridge::ProjectViewModel model;
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QQmlComponent component(
            &engine, QUrl(QStringLiteral("qrc:/qt/qml/Panta/Shell/Dialogs/NewProjectDialog.qml")));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        QObject owner;
        auto* dialog = component.createWithInitialProperties(
            {{QStringLiteral("projectModel"), QVariant::fromValue(&model)}});
        QVERIFY2(dialog != nullptr, qPrintable(component.errorString()));
        dialog->setParent(&owner);
        auto* window = qobject_cast<QQuickWindow*>(dialog);
        QVERIFY(window != nullptr);
        auto* name = visual_item(window->contentItem(), QStringLiteral("projectNameField"));
        auto* location = visual_item(window->contentItem(), QStringLiteral("projectLocationField"));
        auto* accept = visual_item(window->contentItem(), QStringLiteral("newProjectAccept"));
        auto* cancel = visual_item(window->contentItem(), QStringLiteral("newProjectCancel"));
        QVERIFY(name && location && accept && cancel);
        QSignalSpy created(dialog, SIGNAL(projectCreated(QString)));
        QVERIFY(created.isValid());

        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        name->setProperty("text", QStringLiteral("Created"));
        location->setProperty("text", fixture.path());
        QVERIFY(QMetaObject::invokeMethod(dialog, "submit"));
        QVERIFY(window->isVisible());
        QVERIFY(dialog->property("submitting").toBool());
        QVERIFY(!name->isEnabled());
        QVERIFY(!location->isEnabled());
        QVERIFY(!accept->isEnabled());
        QVERIFY(!cancel->isEnabled());
        QVERIFY(!window->close());
        QVERIFY(QMetaObject::invokeMethod(dialog, "submit"));
        QCOMPARE(created.count(), 0);
        QTRY_COMPARE(created.count(), 1);
        QTRY_VERIFY(!window->isVisible());
        QVERIFY(!created.constFirst().constFirst().toString().isEmpty());

        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        name->setProperty("text", QStringLiteral("Created"));
        location->setProperty("text", fixture.path());
        QVERIFY(QMetaObject::invokeMethod(dialog, "submit"));
        QTRY_VERIFY(!model.projectOperationPending());
        QVERIFY(window->isVisible());
        QVERIFY(!dialog->property("submitting").toBool());
        QCOMPARE(created.count(), 1);
        QCOMPARE(name->property("text").toString(), QStringLiteral("Created"));
        QCOMPARE(location->property("text").toString(), fixture.path());
        QVERIFY(!model.error().isEmpty());
        QVERIFY(accept->isEnabled());
        QVERIFY(window->close());
#endif
    }

    void generate_volume_dialog_rejects_missing_step_and_stays_open() {
#if !defined(PANTA_TEST_WITH_BRIDGE)
        QSKIP("Volume generation requires the enabled Bridge module");
#else
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        panta::bridge::ProjectViewModel model;
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QQmlComponent component(
            &engine,
            QUrl(QStringLiteral("qrc:/qt/qml/Panta/Shell/Dialogs/GenerateVolumeDialog.qml")));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        QObject owner;
        auto* dialog = component.createWithInitialProperties(
            {{QStringLiteral("projectModel"), QVariant::fromValue(&model)}});
        QVERIFY2(dialog != nullptr, qPrintable(component.errorString()));
        dialog->setParent(&owner);
        auto* window = qobject_cast<QQuickWindow*>(dialog);
        QVERIFY(window != nullptr);
        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        QVERIFY(window->isVisible());
        auto* step = visual_item(window->contentItem(), QStringLiteral("generateStepField"));
        QVERIFY(step != nullptr);
        QVERIFY(step->setProperty("text",
                                  QDir(fixture.path()).filePath(QStringLiteral("missing.step"))));
        // 无工程或文件缺失时提交被拒绝，对话框保持打开并显示错误。
        QVERIFY(QMetaObject::invokeMethod(dialog, "submit"));
        QVERIFY(window->isVisible());
        QVERIFY(!model.generationPending());
        QVERIFY(!model.errorCode().isEmpty());
#endif
    }

    void large_mesh_settings_dialog_shows_wan_and_rejects_invalid_budget() {
#if !defined(PANTA_TEST_WITH_BRIDGE)
        QSKIP("Large mesh settings require the enabled Bridge module");
#else
        panta::bridge::ProjectViewModel model;
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QQmlComponent component(
            &engine,
            QUrl(QStringLiteral("qrc:/qt/qml/Panta/Shell/Dialogs/LargeMeshSettingsDialog.qml")));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        QObject owner;
        auto* dialog = component.createWithInitialProperties(
            {{QStringLiteral("projectModel"), QVariant::fromValue(&model)}});
        QVERIFY2(dialog != nullptr, qPrintable(component.errorString()));
        dialog->setParent(&owner);
        auto* window = qobject_cast<QQuickWindow*>(dialog);
        QVERIFY(window != nullptr);
        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        QVERIFY(window->isVisible());
        auto* triangles = visual_item(window->contentItem(), QStringLiteral("lodTrianglesField"));
        auto* budget = visual_item(window->contentItem(), QStringLiteral("lodBudgetField"));
        QVERIFY(triangles != nullptr);
        QVERIFY(budget != nullptr);
        QCOMPARE(triangles->property("text").toString(),
                 QString::number(model.lodActivationTriangles() / 10000));
        // 预算不小于阈值时被 Rust 拒绝：对话框保持打开、设置不变，不写持久化文件。
        const double before = model.lodBudgetTriangles();
        QVERIFY(budget->setProperty("text", triangles->property("text")));
        QVERIFY(QMetaObject::invokeMethod(dialog, "submit"));
        QVERIFY(window->isVisible());
        QCOMPARE(model.errorCode(), QStringLiteral("project.lod_settings_invalid"));
        QCOMPARE(model.lodBudgetTriangles(), before);
        QVERIFY(window->close());
#endif
    }

    void button_font_size_reaches_its_label() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        auto* button = create_component(
            engine, QStringLiteral("qrc:/qt/qml/Panta/Shell/Components/Atoms/ThemedToolButton.qml"),
            owner);
        QVERIFY(button != nullptr);
        button->setProperty("text", QStringLiteral("字号覆盖"));
        QObject* label = nullptr;
        for (auto* child : button->findChildren<QObject*>()) {
            if (child->property("text").toString() == QStringLiteral("字号覆盖")) {
                label = child;
                break;
            }
        }
        QVERIFY(label != nullptr);
        QFont font = button->property("font").value<QFont>();
        font.setPixelSize(19);
        button->setProperty("font", font);
        QCOMPARE(label->property("font").value<QFont>().pixelSize(), 19);
    }

    void icon_resolves_module_resource() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        QObject* icon = create_component(
            engine, QStringLiteral("qrc:/qt/qml/Panta/Shell/Components/Atoms/ThemedIcon.qml"),
            owner);
        QVERIFY(icon != nullptr);
        icon->setProperty("name", QStringLiteral("caret"));
        QCOMPARE(icon->property("source").value<QUrl>(),
                 QUrl(QStringLiteral("image://panta-icons/caret/ff4a4a4a")));
        // qtsvg 提供的 qsvg 插件可以读取模块打包的原始 SVG viewBox 尺寸。
        QImageReader reader(QStringLiteral(":/qt/qml/Panta/Shell/icons/caret.svg"));
        QVERIFY(reader.canRead());
        QCOMPARE(reader.size(), QSize(7, 5));
        icon->setProperty("color", QColor(QStringLiteral("#ffffff")));
        QCOMPARE(icon->property("source").value<QUrl>(),
                 QUrl(QStringLiteral("image://panta-icons/caret/ffffffff")));
        icon->setProperty("preserveSourceColors", true);
        icon->setProperty("name", QStringLiteral("open"));
        QCOMPARE(icon->property("source").value<QUrl>(),
                 QUrl(QStringLiteral("qrc:/qt/qml/Panta/Shell/icons/open.svg")));
        QImageReader assetReader(QStringLiteral(":/qt/qml/Panta/Shell/icons/stl-file.svg"));
        QVERIFY(assetReader.canRead());
        QVERIFY(!assetReader.read().isNull());
        icon->setProperty("name", QString());
        QVERIFY(icon->property("source").value<QUrl>().isEmpty());
    }

    void svg_resources_render_with_caller_color() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        auto* provider = static_cast<QQuickImageProvider*>(engine.imageProvider("panta-icons"));
        QVERIFY(provider != nullptr);
        const QString resource_root = QStringLiteral(":/qt/qml/Panta/Shell/icons/");
        const QStringList files =
            QDir(resource_root).entryList({QStringLiteral("*.svg")}, QDir::Files);
        QVERIFY(!files.isEmpty());
        for (const QString& file : files) {
            QImageReader reader(resource_root + file);
            QVERIFY2(reader.canRead(), qPrintable(file));
            QVERIFY2(reader.size().isValid(), qPrintable(file));
            QVERIFY2(!reader.read().isNull(), qPrintable(file));
        }

        QStringList monochrome_icons = panta::monochrome_icon_names().values();
        monochrome_icons.sort();
        for (const QString& name : monochrome_icons) {
            QImageReader reader(resource_root + name + QStringLiteral(".svg"));
            const QSize expected_size = reader.size();
            QVERIFY(expected_size.isValid());
            for (const int pixels : {16, 18, 24, 48}) {
                QSize original;
                const QImage frame = provider->requestImage(name + QStringLiteral("/ff2878b8"),
                                                            &original, QSize(pixels, pixels));
                QVERIFY2(!frame.isNull(), qPrintable(name));
                QCOMPARE(original, expected_size);
                QCOMPARE(frame.size(), QSize(pixels, pixels));
                int colored = 0;
                for (int y = 0; y < pixels; ++y) {
                    for (int x = 0; x < pixels; ++x) {
                        const QColor pixel = frame.pixelColor(x, y);
                        if (pixel.alpha() > 0) {
                            ++colored;
                        }
                        if (pixel.alpha() > 192) {
                            // Alpha mask 只保留几何覆盖，像素颜色必须完全来自调用方。
                            QVERIFY2(qAbs(pixel.red() - 40) <= 1, qPrintable(name));
                            QVERIFY2(qAbs(pixel.green() - 120) <= 1, qPrintable(name));
                            QVERIFY2(qAbs(pixel.blue() - 184) <= 1, qPrintable(name));
                        }
                    }
                }
                QVERIFY2(colored > 0, qPrintable(name));
            }
        }

        const QImage caret =
            provider->requestImage(QStringLiteral("caret/ff4a4a4a"), nullptr, QSize(24, 24));
        QVERIFY(!caret.isNull());
        QRect caret_bounds;
        for (int y = 0; y < caret.height(); ++y) {
            for (int x = 0; x < caret.width(); ++x) {
                if (caret.pixelColor(x, y).alpha() > 0) {
                    caret_bounds |= QRect(x, y, 1, 1);
                }
            }
        }
        QVERIFY(caret_bounds.isValid());
        QVERIFY(caret_bounds.width() < caret.width());
        QVERIFY(caret_bounds.height() < 16);

        QSize original;
        const QImage white =
            provider->requestImage(QStringLiteral("close/ffffffff"), &original, QSize(24, 24));
        QVERIFY(white.pixelColor(6, 6).lightness() > 240);
        const QImage transparent =
            provider->requestImage(QStringLiteral("close/00ffffff"), nullptr, QSize(24, 24));
        QCOMPARE(transparent.pixelColor(6, 6).alpha(), 0);
        const QSize fallback_size =
            QImageReader(resource_root + QStringLiteral("close.svg")).size();
        QSize fallback_original;
        const QImage fallback =
            provider->requestImage(QStringLiteral("close/ff2878b8"), &fallback_original, {});
        QVERIFY(!fallback.isNull());
        QCOMPARE(fallback_original, fallback_size);
        QCOMPARE(fallback.size(), fallback_size);
        QVERIFY(provider->requestImage(QStringLiteral("../new/ffffffff"), nullptr, {}).isNull());
        QVERIFY(provider->requestImage(QStringLiteral("open/ffffffff"), nullptr, {}).isNull());
        QVERIFY(provider->requestImage(QStringLiteral("close/not-a-color"), nullptr, {}).isNull());
    }

    void document_tab_connectors_and_accessibility() {
        // 采样凹弧和白线，防止连接件缺失或白色外溢。
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        QQmlComponent component(
            &engine, QUrl(QStringLiteral(
                         "qrc:/qt/qml/Panta/Shell/Components/Composites/DocumentTabBar.qml")));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));

        QVariantList documents;
        documents.append(QVariantMap{{QStringLiteral("id"), QStringLiteral("first-import")},
                                     {QStringLiteral("kind"), QStringLiteral("import")},
                                     {QStringLiteral("state"), QStringLiteral("ready")},
                                     {QStringLiteral("title"), QStringLiteral("first.stl")},
                                     {QStringLiteral("message"), QString{}}});
        documents.append(QVariantMap{{QStringLiteral("id"), QStringLiteral("import-1")},
                                     {QStringLiteral("kind"), QStringLiteral("import")},
                                     {QStringLiteral("state"), QStringLiteral("ready")},
                                     {QStringLiteral("title"), QStringLiteral("part.stl")},
                                     {QStringLiteral("message"), QString{}}});

        auto* bar = qobject_cast<QQuickItem*>(component.createWithInitialProperties(
            QVariantMap{{QStringLiteral("objectName"), QStringLiteral("documentTabBar")},
                        {QStringLiteral("documents"), documents},
                        {QStringLiteral("activeDocumentId"), QStringLiteral("first-import")}}));
        QVERIFY(bar != nullptr);
        bar->setParent(&owner);
        bar->setWidth(400);
        bar->setHeight(39);

        QQuickWindow window;
        window.resize(400, 39);
        bar->setParentItem(window.contentItem());
        window.show();
        QVERIFY2(QTest::qWaitForWindowExposed(&window), "window was not exposed");

        QAccessibleInterface* accessibleList = QAccessible::queryAccessibleInterface(bar);
        QVERIFY(accessibleList != nullptr);
        QCOMPARE(accessibleList->role(), QAccessible::PageTabList);
        QCOMPARE(accessibleList->text(QAccessible::Name), QStringLiteral("打开的文档"));
        QTest::mouseMove(&window, QPoint(350, 20));

        const auto result = bar->grabToImage();
        QVERIFY(result != nullptr);
        QSignalSpy ready(result.data(), &QQuickItemGrabResult::ready);
        QVERIFY(ready.wait(5000));
        const QImage image = result->image();
        QCOMPARE(image.size(), QSize(400, 39));

        // 取样避开文字和抗锯齿边界。
        const QColor band_gray(QStringLiteral("#e7e4e1"));
        const QColor panel_white(QStringLiteral("#ffffff"));
        const auto near_white = [](const QColor& color) {
            return color.alpha() == 255 && color.red() >= 254 && color.green() >= 254 &&
                   color.blue() >= 254;
        };
        QCOMPARE(image.pixelColor(130, 37), QColor(QStringLiteral("#ffffff")));
        QCOMPARE(image.pixelColor(270, 37), QColor(QStringLiteral("#ffffff")));
        QCOMPARE(image.pixelColor(60, 10), panel_white);
        QCOMPARE(image.pixelColor(350, 20), band_gray);
        QVERIFY(near_white(image.pixelColor(133, 33)));
        QCOMPARE(image.pixelColor(136, 30), band_gray);
        QCOMPARE(image.pixelColor(139, 29), band_gray);

        bar->setProperty("activeDocumentId", QStringLiteral("import-1"));
        QTest::mouseMove(&window, QPoint(350, 20));
        const auto secondResult = bar->grabToImage();
        QVERIFY(secondResult != nullptr);
        QSignalSpy secondReady(secondResult.data(), &QQuickItemGrabResult::ready);
        QVERIFY(secondReady.wait(5000));
        const QImage secondImage = secondResult->image();
        QCOMPARE(secondImage.pixelColor(127, 29), band_gray);
        QVERIFY(near_white(secondImage.pixelColor(133, 33)));
        QVERIFY(near_white(secondImage.pixelColor(265, 33)));
        QCOMPARE(secondImage.pixelColor(271, 29), band_gray);

        QQuickItem* importTab = visual_item(bar, QStringLiteral("documentTab-import-1"));
        QVERIFY(importTab != nullptr);
        QQuickItem* importClose = visual_item(importTab, QStringLiteral("documentTabClose"));
        QVERIFY(importClose != nullptr);
        QTest::mouseMove(&window, QPoint(251, 20));
        QTRY_COMPARE(importClose->property("color").value<QColor>(),
                     QColor(QStringLiteral("#e5f1fb")));
        QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, QPoint(251, 20));
        QTRY_COMPARE(importClose->property("color").value<QColor>(),
                     QColor(QStringLiteral("#d5e3ef")));
        QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, QPoint(251, 20));
        QTest::mouseMove(&window, QPoint(350, 20));
        QTRY_COMPARE(importClose->property("color").value<QColor>(), Qt::transparent);

        QQuickItem* firstTab = visual_item(bar, QStringLiteral("documentTab-first-import"));
        QVERIFY(firstTab != nullptr);
        QQuickItem* firstClose = visual_item(firstTab, QStringLiteral("documentTabClose"));
        QVERIFY(firstClose != nullptr);
        QTest::mouseMove(&window, QPoint(119, 20));
        QTRY_COMPARE(firstClose->property("color").value<QColor>(),
                     QColor(QStringLiteral("#e5f1fb")));
        QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, QPoint(119, 20));
        QTRY_COMPARE(firstClose->property("color").value<QColor>(),
                     QColor(QStringLiteral("#d5e3ef")));
        QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, QPoint(119, 20));
        QTest::mouseMove(&window, QPoint(350, 20));
        QTRY_COMPARE(firstClose->property("color").value<QColor>(), Qt::transparent);

        QSignalSpy activated(bar, SIGNAL(activateDocument(QString)));
        QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, QPoint(200, 20));
        QCOMPARE(activated.count(), 1);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("import-1"));
        QQuickItem* dragArea = visual_item(firstTab, QStringLiteral("documentTabDragArea"));
        QVERIFY(dragArea != nullptr);
        bar->setProperty("order",
                         QStringList{QStringLiteral("import-1"), QStringLiteral("first-import")});
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(firstTab->x() - 134.0) < 0.1, 10000);

        bar->setProperty("order",
                         QStringList{QStringLiteral("first-import"), QStringLiteral("import-1")});
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(firstTab->x() - 2.0) < 0.1, 10000);

        // 垂直手势不开始拖动，也不改变显示顺序。
        const QStringList initialOrder = bar->property("order").toStringList();
        QVERIFY(QMetaObject::invokeMethod(bar, "begin_press", Q_ARG(QVariant, "first-import"),
                                          Q_ARG(QVariant, 20.0), Q_ARG(QVariant, 20.0)));
        QVERIFY(QMetaObject::invokeMethod(bar, "detect_drag", Q_ARG(QVariant, "first-import"),
                                          Q_ARG(QVariant, 21.0), Q_ARG(QVariant, 50.0)));
        QCOMPARE(bar->property("draggedId").toString(), QString{});
        QCOMPARE(bar->property("order").toStringList(), initialOrder);

        // 关闭按钮独占按压；拖出按钮区域不能启动页签拖拽或关闭标签。
        QSignalSpy closed(bar, SIGNAL(closeDocument(QString)));
        const QPoint closeCenter =
            importClose->mapToScene(QPointF(importClose->width() / 2, importClose->height() / 2))
                .toPoint();
        QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, closeCenter);
        QTest::mouseMove(&window, closeCenter + QPoint(-40, 0));
        QCOMPARE(bar->property("draggedId").toString(), QString{});
        QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, closeCenter + QPoint(-40, 0));
        QCOMPARE(closed.count(), 0);

        QVERIFY(QMetaObject::invokeMethod(bar, "begin_press", Q_ARG(QVariant, "first-import"),
                                          Q_ARG(QVariant, 20.0), Q_ARG(QVariant, 20.0)));
        QVERIFY(QMetaObject::invokeMethod(bar, "detect_drag", Q_ARG(QVariant, "first-import"),
                                          Q_ARG(QVariant, 30.0), Q_ARG(QVariant, 20.0)));
        QCOMPARE(bar->property("draggedId").toString(), QStringLiteral("first-import"));
        QVERIFY(firstTab->x() > 2.0);
        QVERIFY(QMetaObject::invokeMethod(bar, "drag_move", Q_ARG(QVariant, 60.0)));
        QVERIFY(firstTab->x() > 2.0);
        QVERIFY(QMetaObject::invokeMethod(bar, "finish_drag", Q_ARG(QVariant, true)));
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(firstTab->x() - 2.0) < 0.1, 10000);
        QCOMPARE(activated.count(), 1);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("first-import"));

        // 悬停移动不能复用上一次按下坐标；真实左键拖动才进入拖拽态。
        QTest::mouseMove(&window, QPoint(20, 20));
        QTest::mouseMove(&window, QPoint(80, 20));
        QCOMPARE(bar->property("draggedId").toString(), QString{});
        QCOMPARE(dragArea->property("cursorShape").toInt(), int(Qt::OpenHandCursor));
        QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, QPoint(20, 20));
        QTest::mouseMove(&window, QPoint(40, 20));
        QTRY_COMPARE(bar->property("draggedId").toString(), QStringLiteral("first-import"));
        QCOMPARE(dragArea->property("cursorShape").toInt(), int(Qt::ClosedHandCursor));
        QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, QPoint(40, 20));
        QCOMPARE(bar->property("draggedId").toString(), QString{});
        QCOMPARE(dragArea->property("cursorShape").toInt(), int(Qt::OpenHandCursor));
        QCOMPARE(activated.count(), 1);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("first-import"));
        QTest::mouseMove(&window, QPoint(80, 20));
        QCOMPARE(bar->property("draggedId").toString(), QString{});

        QSignalSpy moved(bar, SIGNAL(moveDocument(int, int)));
        QVERIFY(QMetaObject::invokeMethod(bar, "begin_press", Q_ARG(QVariant, "first-import"),
                                          Q_ARG(QVariant, 20.0), Q_ARG(QVariant, 20.0)));
        QVERIFY(QMetaObject::invokeMethod(bar, "detect_drag", Q_ARG(QVariant, "first-import"),
                                          Q_ARG(QVariant, 40.0), Q_ARG(QVariant, 20.0)));
        QCOMPARE(activated.count(), 1);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("first-import"));
        QVERIFY(QMetaObject::invokeMethod(bar, "drag_move", Q_ARG(QVariant, 220.0)));
        QVERIFY(QMetaObject::invokeMethod(bar, "finish_drag", Q_ARG(QVariant, true)));
        QCOMPARE(moved.count(), 1);
        QCOMPARE(moved.takeFirst(), (QList<QVariant>{0, 1}));
        QVariantList reordered{documents[1], documents[0]};
        bar->setProperty("documents", reordered);
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(firstTab->x() - 134.0) < 0.1, 10000);

        bar->setProperty("reducedMotion", true);
        bar->setProperty("order",
                         QStringList{QStringLiteral("first-import"), QStringLiteral("import-1")});
        QCOMPARE(firstTab->x(), 2.0);
        bar->setProperty("order",
                         QStringList{QStringLiteral("import-1"), QStringLiteral("first-import")});
        QCOMPARE(firstTab->x(), 134.0);
    }

    void duplicate_document_tabs_are_visibly_and_accessibly_distinguished() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        QQmlComponent component(
            &engine, QUrl(QStringLiteral(
                         "qrc:/qt/qml/Panta/Shell/Components/Composites/DocumentTabBar.qml")));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));

        const auto document = [](const QString& id, const QString& kind, const QString& title,
                                 int ordinal = 1, int count = 1) {
            return QVariantMap{{QStringLiteral("id"), id},
                               {QStringLiteral("kind"), kind},
                               {QStringLiteral("state"), QStringLiteral("ready")},
                               {QStringLiteral("title"), title},
                               {QStringLiteral("displayOrdinal"), ordinal},
                               {QStringLiteral("displayCount"), count},
                               {QStringLiteral("message"), QString{}}};
        };
        const QString longTitle = QStringLiteral("exceptionally-long-source-name");
        const QVariantList documents{
            document(QStringLiteral("first-import"), QStringLiteral("import"),
                     QStringLiteral("first.stl")),
            document(QStringLiteral("import-1"), QStringLiteral("import"), longTitle, 1, 2),
            document(QStringLiteral("import-2"), QStringLiteral("import"), longTitle, 2, 2)};
        auto* bar = qobject_cast<QQuickItem*>(component.createWithInitialProperties(
            QVariantMap{{QStringLiteral("objectName"), QStringLiteral("documentTabBar")},
                        {QStringLiteral("documents"), documents},
                        {QStringLiteral("activeDocumentId"), QStringLiteral("import-1")}}));
        QVERIFY(bar != nullptr);
        bar->setParent(&owner);
        bar->setWidth(240);
        bar->setHeight(39);

        QQuickWindow window;
        window.resize(240, 39);
        bar->setParentItem(window.contentItem());
        window.show();
        QVERIFY2(QTest::qWaitForWindowExposed(&window), "window was not exposed");

        QQuickItem* firstTab = visual_item(bar, QStringLiteral("documentTab-import-1"));
        QQuickItem* secondTab = visual_item(bar, QStringLiteral("documentTab-import-2"));
        QVERIFY(firstTab != nullptr);
        QVERIFY(secondTab != nullptr);
        QQuickItem* firstTitle = visual_item(firstTab, QStringLiteral("documentTabTitle"));
        QQuickItem* secondTitle = visual_item(secondTab, QStringLiteral("documentTabTitle"));
        QVERIFY(firstTitle != nullptr);
        QVERIFY(secondTitle != nullptr);
        QCOMPARE(firstTab->width(), 130.0);
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(firstTab->x() - 134.0) < 0.1, 10000);
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(secondTab->x() - 266.0) < 0.1, 10000);
        QCOMPARE(firstTitle->property("text").toString(), QStringLiteral("1 · ") + longTitle);
        QCOMPARE(secondTitle->property("text").toString(), QStringLiteral("2 · ") + longTitle);
        QTRY_VERIFY(firstTitle->property("truncated").toBool());
        QTRY_VERIFY(secondTitle->property("truncated").toBool());

        QAccessibleInterface* accessibleTab = QAccessible::queryAccessibleInterface(firstTab);
        QVERIFY(accessibleTab != nullptr);
        QCOMPARE(accessibleTab->role(), QAccessible::PageTab);
        QCOMPARE(accessibleTab->text(QAccessible::Name),
                 longTitle + QStringLiteral("（第 1 个导入）"));
        QAccessibleInterface* accessibleSecondTab =
            QAccessible::queryAccessibleInterface(secondTab);
        QVERIFY(accessibleSecondTab != nullptr);
        QCOMPARE(accessibleTab->state().selected, true);
        QCOMPARE(accessibleSecondTab->state().selected, false);
        bar->setProperty("activeDocumentId", QStringLiteral("import-2"));
        QTRY_VERIFY(!accessibleTab->state().selected);
        QTRY_VERIFY(accessibleSecondTab->state().selected);
        bar->setProperty("activeDocumentId", QStringLiteral("import-1"));
        QTRY_VERIFY(accessibleTab->state().selected);
        QTRY_VERIFY(!accessibleSecondTab->state().selected);

        QSignalSpy activated(bar, SIGNAL(activateDocument(QString)));
        QCOMPARE(firstTab->property("activeFocusOnTab").toBool(), true);
        firstTab->forceActiveFocus();
        QTRY_VERIFY(firstTab->hasActiveFocus());
        const auto focusedImageResult = bar->grabToImage();
        QVERIFY(focusedImageResult);
        QSignalSpy focusedImageReady(focusedImageResult.data(), &QQuickItemGrabResult::ready);
        QVERIFY(focusedImageReady.wait(5000));
        QCOMPARE(focusedImageResult->image().pixelColor(150, 3), QColor(Qt::white));
        QTest::keyClick(&window, Qt::Key_Return);
        QCOMPARE(activated.count(), 1);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("import-1"));

        QQuickItem* closeButton = visual_item(secondTab, QStringLiteral("documentTabClose"));
        QVERIFY(closeButton != nullptr);
        closeButton->forceActiveFocus();
        QTRY_VERIFY(closeButton->hasActiveFocus());
        QTest::mouseClick(
            &window, Qt::LeftButton, Qt::NoModifier,
            firstTab->mapToScene(QPointF(firstTab->width() / 2, firstTab->height() / 2)).toPoint());
        QTRY_VERIFY(firstTab->hasActiveFocus());
        QTRY_VERIFY(accessibleTab->state().focused);
        QCOMPARE(activated.count(), 1);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("import-1"));

        QQuickItem* scroller = visual_item(bar, QStringLiteral("documentTabScroller"));
        QVERIFY(scroller != nullptr);
        secondTab->forceActiveFocus();
        QSignalSpy closed(bar, SIGNAL(closeDocument(QString)));
        QAccessibleInterface* accessibleClose = QAccessible::queryAccessibleInterface(
            visual_item(secondTab, QStringLiteral("documentTabClose")));
        QVERIFY(accessibleClose != nullptr);
        QCOMPARE(accessibleClose->role(), QAccessible::Button);
        QCOMPARE(accessibleClose->text(QAccessible::Name),
                 QStringLiteral("关闭") + longTitle + QStringLiteral("（第 2 个导入）"));
        QTest::keyClick(&window, Qt::Key_Left);
        QTRY_VERIFY(firstTab->hasActiveFocus());
        QTRY_VERIFY(scroller->property("contentX").toReal() > 0.0);
        const qreal contentAfterLeft = scroller->property("contentX").toReal();
        QTest::keyClick(&window, Qt::Key_Right);
        QTRY_VERIFY(secondTab->hasActiveFocus());
        QTRY_VERIFY(scroller->property("contentX").toReal() > contentAfterLeft);
        QTRY_VERIFY(secondTab->x() + secondTab->width() <=
                    scroller->property("contentX").toReal() + scroller->width() + 0.1);
        QCOMPARE(activated.count(), 2);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("import-1"));
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("import-2"));
        QTest::keyClick(&window, Qt::Key_Space);
        QCOMPARE(activated.count(), 1);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("import-2"));

        QCOMPARE(closeButton->property("activeFocusOnTab").toBool(), true);
        closeButton->forceActiveFocus();
        QTRY_VERIFY(closeButton->hasActiveFocus());
        QTest::keyClick(&window, Qt::Key_Space);
        QCOMPARE(closed.count(), 1);
        QCOMPARE(closed.takeFirst().at(0).toString(), QStringLiteral("import-2"));
    }

    void document_tab_drag_crosses_multiple_neighbors_without_changing_identity() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        QQmlComponent component(
            &engine, QUrl(QStringLiteral(
                         "qrc:/qt/qml/Panta/Shell/Components/Composites/DocumentTabBar.qml")));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));

        const auto document = [](const QString& id) {
            return QVariantMap{{QStringLiteral("id"), id},
                               {QStringLiteral("kind"), QStringLiteral("import")},
                               {QStringLiteral("state"), QStringLiteral("ready")},
                               {QStringLiteral("title"), id + QStringLiteral(".stl")},
                               {QStringLiteral("message"), QString{}}};
        };
        const QVariantList documents{
            document(QStringLiteral("doc-1")), document(QStringLiteral("doc-2")),
            document(QStringLiteral("doc-3")), document(QStringLiteral("doc-4"))};
        auto* bar = qobject_cast<QQuickItem*>(component.createWithInitialProperties(
            QVariantMap{{QStringLiteral("objectName"), QStringLiteral("documentTabBar")},
                        {QStringLiteral("documents"), documents},
                        {QStringLiteral("activeDocumentId"), QStringLiteral("doc-4")}}));
        QVERIFY(bar != nullptr);
        bar->setParent(&owner);
        bar->setWidth(600);
        bar->setHeight(39);

        QQuickWindow window;
        window.resize(600, 80);
        bar->setParentItem(window.contentItem());
        window.show();
        QVERIFY2(QTest::qWaitForWindowExposed(&window), "window was not exposed");

        QQuickItem* draggedTab = visual_item(bar, QStringLiteral("documentTab-doc-1"));
        QVERIFY(draggedTab != nullptr);
        QCOMPARE(draggedTab->y(), 3.0);
        QSignalSpy activated(bar, SIGNAL(activateDocument(QString)));
        QSignalSpy moved(bar, SIGNAL(moveDocument(int, int)));
        const QStringList initialOrder = bar->property("order").toStringList();
        QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, QPoint(20, 20));
        QTest::mouseMove(&window, QPoint(21, 55));
        QCOMPARE(bar->property("draggedId").toString(), QString{});
        QCOMPARE(bar->property("order").toStringList(), initialOrder);
        QCOMPARE(activated.count(), 0);
        QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, QPoint(21, 55));

        QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, QPoint(20, 20));
        QTest::mouseMove(&window, QPoint(200, 20));
        QTRY_COMPARE(bar->property("draggedId").toString(), QStringLiteral("doc-1"));
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(draggedTab->x() - 182.0) < 0.1, 10000);
        QCOMPARE(draggedTab->opacity(), 1.0);
        QCOMPARE(activated.count(), 1);
        QCOMPARE(activated.takeFirst().at(0).toString(), QStringLiteral("doc-1"));
        bar->setProperty("activeDocumentId", QStringLiteral("doc-1"));

        QTest::mouseMove(&window, QPoint(520, 20));
        QTRY_COMPARE(bar->property("order").toStringList(),
                     (QStringList{QStringLiteral("doc-2"), QStringLiteral("doc-3"),
                                  QStringLiteral("doc-4"), QStringLiteral("doc-1")}));
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(draggedTab->x() - 470.0) < 0.1, 10000);
        QCOMPARE(draggedTab->y(), 3.0);
        QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, QPoint(520, 20));
        QCOMPARE(bar->property("draggedId").toString(), QString{});
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(draggedTab->x() - 398.0) < 0.1, 10000);
        QCOMPARE(moved.count(), 1);
        QCOMPARE(moved.takeFirst(), (QList<QVariant>{0, 3}));

        QVariantList reordered{documents[1], documents[2], documents[3], documents[0]};
        bar->setProperty("documents", reordered);
        QQuickItem* movedTab = visual_item(bar, QStringLiteral("documentTab-doc-1"));
        QVERIFY(movedTab != nullptr);
        QTRY_VERIFY_WITH_TIMEOUT(qAbs(movedTab->x() - 398.0) < 0.1, 10000);
        QVERIFY(movedTab->property("isActive").toBool());
        QQuickItem* previousActiveTab = visual_item(bar, QStringLiteral("documentTab-doc-4"));
        QVERIFY(previousActiveTab != nullptr);
        QCOMPARE(previousActiveTab->property("isActive").toBool(), false);
    }

    void document_tab_scroll_preserves_active_edge_connectors() {
        QQmlEngine engine;
        panta::install_icon_provider(engine);
        QObject owner;
        QQmlComponent component(
            &engine, QUrl(QStringLiteral(
                         "qrc:/qt/qml/Panta/Shell/Components/Composites/DocumentTabBar.qml")));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));

        const auto document = [](const QString& id, const QString& kind, const QString& title) {
            return QVariantMap{{QStringLiteral("id"), id},
                               {QStringLiteral("kind"), kind},
                               {QStringLiteral("state"), QStringLiteral("ready")},
                               {QStringLiteral("title"), title},
                               {QStringLiteral("message"), QString{}}};
        };
        const QVariantList documents{document(QStringLiteral("first-import"),
                                              QStringLiteral("import"),
                                              QStringLiteral("first.stl")),
                                     document(QStringLiteral("import-1"), QStringLiteral("import"),
                                              QStringLiteral("one.stl")),
                                     document(QStringLiteral("import-2"), QStringLiteral("import"),
                                              QStringLiteral("two.stl"))};
        auto* bar = qobject_cast<QQuickItem*>(component.createWithInitialProperties(
            QVariantMap{{QStringLiteral("objectName"), QStringLiteral("documentTabBar")},
                        {QStringLiteral("documents"), documents},
                        {QStringLiteral("activeDocumentId"), QStringLiteral("first-import")}}));
        QVERIFY(bar != nullptr);
        bar->setParent(&owner);
        bar->setWidth(240);
        bar->setHeight(39);

        QQuickWindow window;
        window.resize(240, 39);
        bar->setParentItem(window.contentItem());
        window.show();
        QVERIFY2(QTest::qWaitForWindowExposed(&window), "window was not exposed");

        const QColor bandGray(QStringLiteral("#e7e4e1"));
        const QColor panelWhite(QStringLiteral("#ffffff"));
        const auto grab = [bar]() {
            const auto result = bar->grabToImage();
            if (!result) {
                return QImage{};
            }
            QSignalSpy ready(result.data(), &QQuickItemGrabResult::ready);
            if (!ready.wait(5000)) {
                return QImage{};
            }
            return result->image();
        };

        const QImage firstImage = grab();
        QCOMPARE(firstImage.size(), QSize(240, 39));
        QVERIFY(firstImage.pixelColor(0, 34).lightness() > bandGray.lightness());

        bar->setProperty("activeDocumentId", QStringLiteral("import-2"));
        QQuickItem* scroller = visual_item(bar, QStringLiteral("documentTabScroller"));
        QQuickItem* lastTab = visual_item(bar, QStringLiteral("documentTab-import-2"));
        QVERIFY(scroller != nullptr);
        QVERIFY(lastTab != nullptr);
        const qreal endContentX = scroller->property("contentWidth").toReal() - scroller->width();
        scroller->setProperty("contentX", endContentX);
        QTRY_VERIFY(qAbs(scroller->property("contentX").toReal() - endContentX) < 0.1);
        const QPointF lastTabPosition = lastTab->mapToItem(bar, 0, 0);
        QVERIFY(qAbs(lastTabPosition.x() + lastTab->width() - 232.0) < 0.1);

        const QImage lastImage = grab();
        QCOMPARE(lastImage.size(), QSize(240, 39));
        QVERIFY(lastImage.pixelColor(234, 34).lightness() > bandGray.lightness());
        QCOMPARE(lastImage.pixelColor(236, 34), bandGray);
        QCOMPARE(lastImage.pixelColor(130, 37), panelWhite);
    }
};

QTEST_MAIN(ThemeComponentTest)
#include "theme_component_test.moc"
