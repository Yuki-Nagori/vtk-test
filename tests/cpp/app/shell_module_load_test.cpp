// 最小工作台的工程、导入、资产列表与文档接线回归。
#include "../../support/qml/quick_item_helpers.hpp"
#include <QByteArray>
#include <QColor>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QKeySequence>
#include <QMetaEnum>
#include <QMetaProperty>
#include <QPointF>
#include <QQmlApplicationEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSet>
#include <QSignalSpy>
#include <QSize>
#include <QStandardPaths>
#include <QStringList>
#include <QTemporaryDir>
#include <QUrl>
#include <QVariantMap>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <QtTest/qtest.h>
#include <functional>
#include <icon_provider.hpp>
#include <qcontainerfwd.h>
#include <qevent.h>
#include <qhashfunctions.h>
#include <qnamespace.h>
#include <qobject.h>
#include <qobjectdefs.h>
#include <qtestcase.h>
#include <qtmetamacros.h>
#ifdef PANTA_ENABLE_BRIDGE_MODULE
#include <project_view_model.hpp>
#include <settings.hpp>
Q_IMPORT_QML_PLUGIN(Panta_BridgePlugin)
Q_IMPORT_QML_PLUGIN(Panta_VisualizationPlugin)
#endif
Q_IMPORT_QML_PLUGIN(Panta_ShellPlugin)

#define PANTA_STRINGIFY_IMPL(value) #value
#define PANTA_STRINGIFY(value) PANTA_STRINGIFY_IMPL(value)

class ShellModuleLoadTest final : public QObject {
    Q_OBJECT
  private slots:
    void initTestCase() {
        QQuickStyle::setStyle(QStringLiteral("Basic"));
        // 工作台会写布局偏好（task 036）；隔离到测试目录，不污染开发者配置。
        QStandardPaths::setTestModeEnabled(true);
    }
#ifdef PANTA_ENABLE_BRIDGE_MODULE
    // 每个用例都从默认布局开始（左栏展开、底部面板折叠）。
    void init() { QFile::remove(panta::bridge::Settings::layoutSettingsFile()); }
#endif
    void loads_shell_module() {
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
#ifdef PANTA_ENABLE_BRIDGE_MODULE
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
#else
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("AppNoBridge"));
#endif
        QVERIFY2(!engine.rootObjects().isEmpty(), "Shell failed to load");
        auto* root = engine.rootObjects().constFirst();
#ifndef PANTA_ENABLE_BRIDGE_MODULE
        QVERIFY(root->findChild<QObject*>(QStringLiteral("shellCaption")));
#else
        // task 036：工程名只在窗口标题显示，工具栏与状态栏为独立组件。
        QVERIFY(root->findChild<QObject*>(QStringLiteral("mainToolBar")));
        QVERIFY(root->findChild<QObject*>(QStringLiteral("statusBar")));
        QVERIFY(root->findChild<QObject*>(QStringLiteral("bottomPanel")));
        QVERIFY(root->findChild<QObject*>(QStringLiteral("caeViewport")));
        auto* bar = root->findChild<QObject*>(QStringLiteral("documentTabBar"));
        QVERIFY(bar);
        QVERIFY(bar->property("documents").toList().isEmpty());
        QVERIFY(bar->property("activeDocumentId").toString().isEmpty());
        // task 036（034 Q6）：无播放来源时播放栏显示为禁用细条，不再整栏隐藏。
        auto* playback = root->findChild<QObject*>(QStringLiteral("playbackBar"));
        auto* controls = root->findChild<QObject*>(QStringLiteral("playbackControls"));
        QVERIFY(playback && controls);
        QVERIFY(playback->property("visible").toBool());
        QVERIFY(!controls->property("enabled").toBool());
        QVERIFY(!root->findChild<QObject*>(QStringLiteral("playbackStatus"))
                     ->property("text")
                     .toString()
                     .isEmpty());
        QVERIFY(!root->findChild<QObject*>(QStringLiteral("saveProjectButton"))
                     ->property("enabled")
                     .toBool());
        QVERIFY(!root->findChild<QObject*>(QStringLiteral("importStlButton"))
                     ->property("enabled")
                     .toBool());
#endif
    }
#ifdef PANTA_ENABLE_BRIDGE_MODULE
    void workspace_keeps_sidebar_bounded_and_viewport_area_sized() {
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* window = qobject_cast<QQuickWindow*>(engine.rootObjects().constFirst());
        QVERIFY(window);
        auto* sidebar = window->findChild<QQuickItem*>(QStringLiteral("assetSidebar"));
        auto* pane = window->findChild<QQuickItem*>(QStringLiteral("viewportPane"));
        auto* viewport = window->findChild<QQuickItem*>(QStringLiteral("caeViewport"));
        auto* tabs = window->findChild<QQuickItem*>(QStringLiteral("documentTabBar"));
        auto* emptyState = window->findChild<QQuickItem*>(QStringLiteral("viewportEmptyState"));
        QVERIFY(sidebar && pane && viewport && tabs && emptyState);
        for (const QSize size : {QSize(1280, 800), QSize(1600, 1000)}) {
            window->resize(size);
            QTRY_COMPARE(sidebar->width(), 300.0);
            QTRY_VERIFY(pane->width() > window->width() * 0.7);
            QTRY_VERIFY(pane->height() > window->height() * 0.7);
            QTRY_COMPARE(viewport->width(), pane->width());
            QTRY_VERIFY(viewport->height() > pane->height() - 50.0);
            QTRY_COMPARE(tabs->width(), pane->width());
            QVERIFY(tabs->height() > 0.0);
            // task 038：无文档时原生视口隐藏，同一矩形显示空状态引导。
            QVERIFY(!viewport->isVisible());
            QVERIFY(emptyState->isVisible());
            QCOMPARE(emptyState->width(), viewport->width());
            QCOMPARE(emptyState->height(), viewport->height());
            QVERIFY(tabs->isVisible());
            // task 036：页签在视口上方，两者纵向不重叠。
            QVERIFY(tabs->mapToScene(QPointF(0, tabs->height())).y() <=
                    viewport->mapToScene(QPointF()).y() + 0.5);
            const auto sidebarRight = sidebar->mapToScene(QPointF(sidebar->width(), 0)).x();
            QVERIFY(pane->mapToScene(QPointF()).x() >= sidebarRight);
            QVERIFY(pane->mapToScene(QPointF(pane->width(), 0)).x() <= window->width());
        }
        if (const auto dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR"); !dir.isEmpty()) {
            QTest::qWait(50);
            QVERIFY(window->grabWindow().save(
                QDir(dir).filePath(QStringLiteral("viewport-empty-state.png"))));
        }
    }
    void playback_bar_uses_svg_icons_and_shape_coded_loop_state() {
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* bar =
            engine.rootObjects().constFirst()->findChild<QObject*>(QStringLiteral("playbackBar"));
        QVERIFY(bar);
        // 只驱动绑定读到的属性；不点击按钮，避免调用假模型上不存在的方法。
        QVariantMap model{{QStringLiteral("playbackActive"), true},
                          {QStringLiteral("playbackFrameCount"), 3},
                          {QStringLiteral("playbackFrame"), 0},
                          {QStringLiteral("playbackPlaying"), false},
                          {QStringLiteral("playbackLoop"), true},
                          {QStringLiteral("playbackFps"), 10},
                          {QStringLiteral("playbackTimeLabel"), QStringLiteral("t = 0")},
                          {QStringLiteral("playbackStatus"), QString()},
                          { QStringLiteral("playbackUnavailable"),
                            QString() }};
        QVERIFY(bar->setProperty("model", model));
        QTRY_VERIFY(bar->property("visible").toBool());
        const QList<QPair<QString, QString>> buttons{
            {QStringLiteral("playbackFirstButton"), QStringLiteral("playback-first")},
            {QStringLiteral("playbackPreviousButton"), QStringLiteral("playback-previous")},
            {QStringLiteral("playbackPlayButton"), QStringLiteral("playback-play")},
            {QStringLiteral("playbackNextButton"), QStringLiteral("playback-next")},
            {QStringLiteral("playbackLastButton"), QStringLiteral("playback-last")},
            { QStringLiteral("playbackLoopButton"),
              QStringLiteral("playback-loop") }};
        for (const auto& [name, icon] : buttons) {
            auto* button = bar->findChild<QObject*>(name);
            QVERIFY2(button, qPrintable(name));
            QCOMPARE(button->property("iconName").toString(), icon);
            QVERIFY2(button->property("text").toString().isEmpty(), qPrintable(name));
            QVERIFY2(!button->property("accessibleName").toString().isEmpty(), qPrintable(name));
            QVERIFY2(panta::monochrome_icon_names().contains(icon), qPrintable(icon));
        }
        auto* loop = bar->findChild<QObject*>(QStringLiteral("playbackLoopButton"));
        QVERIFY(loop->property("checked").toBool());
        QVERIFY(loop->property("borderColor").value<QColor>().alpha() > 0);
        QCOMPARE(loop->property("accessibleName").toString(), QStringLiteral("循环播放：开"));
        model.insert(QStringLiteral("playbackLoop"), false);
        model.insert(QStringLiteral("playbackPlaying"), true);
        QVERIFY(bar->setProperty("model", model));
        QTRY_VERIFY(!loop->property("checked").toBool());
        QCOMPARE(loop->property("borderColor").value<QColor>().alpha(), 0);
        QVERIFY(
            loop->property("accessibleName").toString().startsWith(QStringLiteral("循环播放：关")));
        QCOMPARE(bar->findChild<QObject*>(QStringLiteral("playbackPlayButton"))
                     ->property("iconName")
                     .toString(),
                 QStringLiteral("playback-pause"));
    }
    void toolbar_routes_project_commands() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* root = engine.rootObjects().constFirst();
        auto* project =
            root->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* create = root->findChild<QObject*>(QStringLiteral("newProjectButton"));
        auto* save = root->findChild<QObject*>(QStringLiteral("saveProjectButton"));
        auto* import = root->findChild<QObject*>(QStringLiteral("importStlButton"));
        auto* newDialog = root->findChild<QObject*>(QStringLiteral("newProjectDialog"));
        auto* importDialog = root->findChild<QObject*>(QStringLiteral("importDialog"));
        QVERIFY(project && create && save && import && newDialog && importDialog);
        QVERIFY(QMetaObject::invokeMethod(create, "clicked"));
        QVERIFY(newDialog->property("visible").toBool());
        QVERIFY(QMetaObject::invokeMethod(newDialog, "close"));
        QVERIFY(project->createProject(QStringLiteral("Workbench"), fixture.path()));
        QVERIFY(!save->property("enabled").toBool());
        QTRY_VERIFY(!project->projectOperationPending());
        QVERIFY(project->errorCode().isEmpty());
        QVERIFY(save->property("enabled").toBool());
        QVERIFY(import->property("enabled").toBool());
        QVERIFY(project->renameProject(QStringLiteral("Renamed")));
        QVERIFY(project->dirty());
        QVERIFY(QMetaObject::invokeMethod(save, "clicked"));
        QTRY_VERIFY(!project->projectOperationPending());
        QVERIFY(!project->dirty());
        QCOMPARE(root->property("title").toString(), QStringLiteral("Renamed · 可视化渲染测试"));
        QVERIFY(QMetaObject::invokeMethod(import, "clicked"));
        QVERIFY(importDialog->property("visible").toBool());
        QVERIFY(QMetaObject::invokeMethod(importDialog, "close"));
    }
    // task 036：工具栏为分组图标按钮；底部面板默认折叠，展开状态与高度跨会话保留。
    void toolbar_icons_and_bottom_panel_follow_layout_settings() {
        const QList<QPair<QString, QString>> buttons{
            {QStringLiteral("newProjectButton"), QStringLiteral("project-new")},
            {QStringLiteral("openProjectButton"), QStringLiteral("project-open")},
            {QStringLiteral("saveProjectButton"), QStringLiteral("project-save")},
            {QStringLiteral("importStlButton"), QStringLiteral("mesh-import")},
            {QStringLiteral("exportMeshButton"), QStringLiteral("mesh-export")},
            {QStringLiteral("viewPresetMenuButton"), QStringLiteral("view-preset")},
            {QStringLiteral("resetViewButton"), QStringLiteral("view-reset")},
            {QStringLiteral("toolbarSectionButton"), QStringLiteral("section")},
            {QStringLiteral("screenshotButton"), QStringLiteral("screenshot")},
            {QStringLiteral("bottomPanelToggle"), QStringLiteral("panel-bottom")},
            { QStringLiteral("taskCancelButton"),
              QStringLiteral("close") }};
        {
            QQmlApplicationEngine engine;
            panta::install_icon_provider(engine);
            engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
            QVERIFY(!engine.rootObjects().isEmpty());
            auto* window = qobject_cast<QQuickWindow*>(engine.rootObjects().constFirst());
            QVERIFY(window);
            for (const auto& [name, icon] : buttons) {
                auto* button = window->findChild<QObject*>(name);
                QVERIFY2(button, qPrintable(name));
                QCOMPARE(button->property("iconName").toString(), icon);
                QVERIFY2(button->property("text").toString().isEmpty(), qPrintable(name));
                QVERIFY2(!button->property("accessibleName").toString().isEmpty(),
                         qPrintable(name));
                QVERIFY2(panta::monochrome_icon_names().contains(icon), qPrintable(icon));
            }
            auto* bottom = window->findChild<QQuickItem*>(QStringLiteral("bottomPanel"));
            auto* body = window->findChild<QQuickItem*>(QStringLiteral("bottomPanelBody"));
            QVERIFY(bottom && body);
            QVERIFY(bottom->property("collapsed").toBool());
            const auto header = bottom->property("headerHeight").toReal();
            QTRY_COMPARE(bottom->height(), header);
            QVERIFY(!body->isVisible());
            QVERIFY(QMetaObject::invokeMethod(bottom, "appendMessage", Q_ARG(QString, "提示"),
                                              Q_ARG(QString, "info")));
            QCOMPARE(bottom->property("messageCount").toInt(), 1);
            QVERIFY(QMetaObject::invokeMethod(bottom, "reveal", Q_ARG(int, 2)));
            QVERIFY(!bottom->property("collapsed").toBool());
            QCOMPARE(bottom->property("currentIndex").toInt(), 2);
            QTRY_COMPARE(bottom->height(), 200.0);
            QVERIFY(body->isVisible());
        }
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* bottom = engine.rootObjects().constFirst()->findChild<QQuickItem*>(
            QStringLiteral("bottomPanel"));
        QVERIFY(bottom);
        QVERIFY(!bottom->property("collapsed").toBool());
        QTRY_COMPARE(bottom->height(), 200.0);
    }
    // task 036（034 Q4）：左栏宽度夹在 260–480 px，可折叠为窄轨；状态写入用户设置并跨引擎恢复。
    void sidebar_width_and_collapse_follow_layout_settings() {
        {
            QQmlApplicationEngine engine;
            panta::install_icon_provider(engine);
            engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
            QVERIFY(!engine.rootObjects().isEmpty());
            auto* window = qobject_cast<QQuickWindow*>(engine.rootObjects().constFirst());
            auto* settings = engine.singletonInstance<panta::bridge::Settings*>(
                QStringLiteral("Panta.Bridge"), QStringLiteral("Settings"));
            QVERIFY(window && settings);
            auto* sidebar = window->findChild<QQuickItem*>(QStringLiteral("assetSidebar"));
            auto* rail = window->findChild<QQuickItem*>(QStringLiteral("sidebarRail"));
            auto* collapse = window->findChild<QObject*>(QStringLiteral("sidebarCollapseButton"));
            auto* expand = window->findChild<QObject*>(QStringLiteral("sidebarExpandButton"));
            auto* menuItem = window->findChild<QObject*>(QStringLiteral("sidebarMenuItem"));
            auto* assets = window->findChild<QQuickItem*>(QStringLiteral("objectTreePane"));
            auto* properties = window->findChild<QQuickItem*>(QStringLiteral("propertiesPanel"));
            auto* pane = window->findChild<QQuickItem*>(QStringLiteral("viewportPane"));
            QVERIFY(sidebar && rail && collapse && expand && menuItem && assets && properties &&
                    pane);
            for (const auto* icon : {collapse, expand}) {
                QCOMPARE(icon->property("iconName").toString(), QStringLiteral("panel-left"));
                QVERIFY(panta::monochrome_icon_names().contains(QStringLiteral("panel-left")));
            }
            QVERIFY(sidebar->isAncestorOf(assets) && sidebar->isAncestorOf(properties));
            QTRY_COMPARE(sidebar->width(), 300.0);
            QVERIFY(!rail->isVisible());
            QVERIFY(menuItem->property("checked").toBool());
            // 越界写入被夹到允许范围。
            settings->setSidebarWidth(1000);
            QTRY_COMPARE(sidebar->width(), 480.0);
            settings->setSidebarWidth(100);
            QTRY_COMPARE(sidebar->width(), 260.0);
            settings->setSidebarWidth(360);
            QTRY_COMPARE(sidebar->width(), 360.0);
            const auto expandedPaneWidth = pane->width();
            QVERIFY(QMetaObject::invokeMethod(collapse, "clicked"));
            QVERIFY(settings->sidebarCollapsed());
            QTRY_VERIFY(!sidebar->isVisible());
            QTRY_VERIFY(rail->isVisible());
            QVERIFY(!menuItem->property("checked").toBool());
            QTRY_VERIFY(pane->width() > expandedPaneWidth + 300.0);
            if (const auto dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR");
                !dir.isEmpty()) {
                QTest::qWait(50);
                QVERIFY(window->grabWindow().save(
                    QDir(dir).filePath(QStringLiteral("sidebar-collapsed.png"))));
            }
        }
        // 新引擎读回折叠状态与宽度；展开后恢复 360 px。
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* window = engine.rootObjects().constFirst();
        auto* sidebar = window->findChild<QQuickItem*>(QStringLiteral("assetSidebar"));
        auto* expand = window->findChild<QObject*>(QStringLiteral("sidebarExpandButton"));
        QVERIFY(sidebar && expand);
        QVERIFY(!sidebar->isVisible());
        QVERIFY(QMetaObject::invokeMethod(expand, "clicked"));
        QTRY_VERIFY(sidebar->isVisible());
        QTRY_COMPARE(sidebar->width(), 360.0);
    }
    void assets_and_document_tabs_follow_saved_project() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        const QString stlPath = fixture.filePath(QStringLiteral("part.stl"));
        QFile stl(stlPath);
        QVERIFY(stl.open(QIODevice::WriteOnly));
        QVERIFY(stl.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n") > 0);
        stl.close();
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* root = engine.rootObjects().constFirst();
        auto* project =
            root->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* sidebar = root->findChild<QQuickItem*>(QStringLiteral("assetSidebar"));
        auto* bar = root->findChild<QObject*>(QStringLiteral("documentTabBar"));
        QVERIFY(project && sidebar && bar);
        auto* tree = project->objectTree();
        QVERIFY(tree);
        QCOMPARE(tree->nodeCount(), 0);
        QVERIFY(project->createProject(QStringLiteral("Imported"), fixture.path()));
        QTRY_VERIFY(!project->projectOperationPending());
        // 新工程只有根节点；导入后出现一个资产节点（task 037）。
        QTRY_COMPARE(tree->nodeCount(), 1);
        const QString projectPath = project->currentPath();
        QVERIFY(project->inspectStl(stlPath));
        QTRY_VERIFY(project->importPreviewReady());
        QVERIFY(project->importStl(stlPath, project->defaultMeshType(),
                                   QStringLiteral("millimeters"), false));
        QTRY_COMPARE(tree->nodeCount(), 2);
        const QString importId = project->importedPartIds().constFirst();
        const QString assetNode = QStringLiteral("asset:") + importId;
        QTRY_COMPARE(project->activeDocumentId(), importId);
        QTRY_COMPARE(bar->property("activeDocumentId").toString(), importId);
        QVERIFY(QMetaObject::invokeMethod(bar, "closeDocument", Q_ARG(QString, importId)));
        QTRY_VERIFY(project->activeDocumentId() != importId);
        QTRY_VERIFY(!visual_items(sidebar, QStringLiteral("objectTreeRow_") + assetNode).isEmpty());
        // 单击只切换属性面板，不打开页签；激活（双击 / Enter）才打开。
        QVERIFY(QMetaObject::invokeMethod(sidebar, "selectNode", Q_ARG(QString, assetNode)));
        QCOMPARE(sidebar->property("selectedNodeId").toString(), assetNode);
        QTest::qWait(20);
        QVERIFY(project->activeDocumentId() != importId);
        QVERIFY(QMetaObject::invokeMethod(sidebar, "activateNode", Q_ARG(QString, assetNode)));
        QTRY_COMPARE(project->activeDocumentId(), importId);
        QVERIFY(project->saveProject());
        QTRY_VERIFY(!project->projectOperationPending());
        QVERIFY(project->createProject(QStringLiteral("Empty"), fixture.path()));
        QTRY_VERIFY(!project->projectOperationPending());
        QTRY_COMPARE(tree->nodeCount(), 1);
        QVERIFY(project->openProject(projectPath));
        QTRY_VERIFY(!project->projectOperationPending());
        QTRY_COMPARE(tree->nodeCount(), 2);
        QCOMPARE(project->importedPartIds().constFirst(), importId);
        QVERIFY(QFile::remove(stlPath));
        project->openImportRecord(importId);
        QTRY_COMPARE(project->activeDocumentId(), importId);
        QTRY_VERIFY(project->errorCode().isEmpty());
    }

    // task 037：键盘上下 / 左右浏览与展开对象树，Enter 激活；可见性眼睛只影响显示，不改 dirty。
    void object_tree_keyboard_and_visibility_stay_display_only() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        QStringList stlPaths;
        for (const auto* name : {"first.stl", "second.stl"}) {
            stlPaths.append(fixture.filePath(QString::fromLatin1(name)));
            QFile stl(stlPaths.constLast());
            QVERIFY(stl.open(QIODevice::WriteOnly));
            QVERIFY(stl.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n") > 0);
        }
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* window = qobject_cast<QQuickWindow*>(engine.rootObjects().constFirst());
        QVERIFY(window);
        auto* project =
            window->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* sidebar = window->findChild<QQuickItem*>(QStringLiteral("assetSidebar"));
        auto* treeView = window->findChild<QQuickItem*>(QStringLiteral("objectTree"));
        auto* viewport = window->findChild<QQuickItem*>(QStringLiteral("caeViewport"));
        QVERIFY(project && sidebar && treeView && viewport);
        auto* tree = project->objectTree();
        QVERIFY(project->createProject(QStringLiteral("Tree"), fixture.path()));
        QTRY_VERIFY(!project->projectOperationPending());
        for (const auto& path : stlPaths) {
            QVERIFY(project->inspectStl(path));
            QTRY_VERIFY(project->importPreviewReady());
            QVERIFY(project->importStl(path, project->defaultMeshType(),
                                       QStringLiteral("millimeters"), false));
            QTRY_VERIFY(!project->importPending());
        }
        QTRY_COMPARE(tree->nodeCount(), 3);
        const auto ids = project->importedPartIds();
        QCOMPARE(ids.size(), 2);
        QVERIFY(project->saveProject());
        QTRY_VERIFY(!project->projectOperationPending());
        QVERIFY(!project->dirty());
        project->openImportRecord(ids.at(1));
        QTRY_COMPARE(project->activeDocumentId(), ids.at(1));
        // task 038：有文档时原生视口可见，空状态引导隐藏。
        QTRY_VERIFY(viewport->isVisible());
        QVERIFY(!window->findChild<QQuickItem*>(QStringLiteral("viewportEmptyState"))->isVisible());

        window->requestActivate();
        QVERIFY(QTest::qWaitForWindowExposed(window));
        treeView->forceActiveFocus();
        QVERIFY(QMetaObject::invokeMethod(sidebar, "selectNode",
                                          Q_ARG(QString, QStringLiteral("project"))));
        QTRY_COMPARE(treeView->property("rows").toInt(), 3);
        QTest::keyClick(window, Qt::Key_Down);
        QTRY_COMPARE(sidebar->property("selectedNodeId").toString(),
                     QStringLiteral("asset:") + ids.at(0));
        QCOMPARE(project->activeDocumentId(), ids.at(1));
        QTest::keyClick(window, Qt::Key_Return);
        QTRY_COMPARE(project->activeDocumentId(), ids.at(0));
        // 左键回到父节点并折叠，右键展开。
        QTest::keyClick(window, Qt::Key_Up);
        QTRY_COMPARE(sidebar->property("selectedNodeId").toString(), QStringLiteral("project"));
        QTest::keyClick(window, Qt::Key_Left);
        QTRY_COMPARE(treeView->property("rows").toInt(), 1);
        QTest::keyClick(window, Qt::Key_Right);
        QTRY_COMPARE(treeView->property("rows").toInt(), 3);

        // 眼睛：隐藏活动资产只隐藏视口网格，节点弱化；不写工程，dirty 不变。
        const QString active = project->activeDocumentId();
        QVERIFY(viewport->property("meshVisible").toBool());
        project->setAssetVisible(active, false);
        QVERIFY(!project->activeMeshVisible());
        QTRY_VERIFY(!viewport->property("meshVisible").toBool());
        QCOMPARE(
            tree->node(QStringLiteral("asset:") + active).value(QStringLiteral("visible")).toBool(),
            false);
        QVERIFY(!project->dirty());
        // 切到另一资产时视口按该资产的可见性显示。
        project->openImportRecord(ids.at(1));
        QTRY_COMPARE(project->activeDocumentId(), ids.at(1));
        QTRY_VERIFY(viewport->property("meshVisible").toBool());
        project->openImportRecord(active);
        QTRY_VERIFY(!viewport->property("meshVisible").toBool());
        project->setAssetVisible(active, true);
        QTRY_VERIFY(viewport->property("meshVisible").toBool());
        QVERIFY(!project->dirty());
        // 未知资产忽略。
        project->setAssetVisible(QStringLiteral("missing"), false);
        QVERIFY(project->activeMeshVisible());
        if (const auto dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR"); !dir.isEmpty()) {
            project->setAssetVisible(ids.at(1), false);
            QVERIFY(QMetaObject::invokeMethod(sidebar, "selectNode",
                                              Q_ARG(QString, QStringLiteral("asset:") + active)));
            sidebar->setProperty("propertiesTab", 2);
            QTest::qWait(50);
            QVERIFY(window->grabWindow().save(
                QDir(dir).filePath(QStringLiteral("tree-assets-info.png"))));
        }
    }
    void project_file_dialog_routes_final_selection_data() {
        QTest::addColumn<QString>("storedName");
        QTest::newRow("plain") << QStringLiteral("Stored");
        QTest::newRow("encoded") << QStringLiteral("已保存 项目 #%");
    }

    void project_file_dialog_routes_final_selection() {
        QFETCH(QString, storedName);
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        panta::bridge::ProjectViewModel stored;
        QVERIFY(stored.createProject(storedName, fixture.path()));
        QTRY_VERIFY(!stored.projectOperationPending());
        QVERIFY(stored.errorCode().isEmpty());
        const QUrl selectedUrl = QUrl::fromLocalFile(stored.currentPath());
        // Qt 解码本地 URL 后使用 /；Windows 新建工程快照可含原生反斜杠。
        const QString selectedPath = selectedUrl.toLocalFile();

        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* root = engine.rootObjects().constFirst();
        auto* project =
            root->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* dialog = root->findChild<QObject*>(QStringLiteral("openProjectFileDialog"));
        QVERIFY(project && dialog);

        const auto* meta = dialog->metaObject();
        const int modeIndex = meta->indexOfProperty("fileMode");
        QVERIFY(modeIndex >= 0);
        const int openFile = meta->property(modeIndex).enumerator().keyToValue("OpenFile");
        QVERIFY(openFile >= 0);
        QCOMPARE(dialog->property("fileMode").toInt(), openFile);
        QCOMPARE(dialog->property("options").toInt(), 0);
        QCOMPARE(dialog->property("currentFolder").toUrl(), project->defaultLocationUrl());
        const QStringList filters = dialog->property("nameFilters").toStringList();
        QCOMPARE(filters.size(), 1);
        QVERIFY(filters.constFirst().endsWith(QStringLiteral("(*.panta)")));

        QVERIFY(project->createProject(QStringLiteral("Current"), fixture.path()));
        QTRY_VERIFY(!project->projectOperationPending());
        QVERIFY(project->errorCode().isEmpty());
        QVERIFY(project->renameProject(QStringLiteral("Unsaved current")));
        const QString previousPath = project->currentPath();
        const QVariantList previousDocuments = project->openDocuments();
        QSignalSpy opened(project, &panta::bridge::ProjectViewModel::projectOpened);
        QVERIFY(opened.isValid());

        // 只驱动公开 QML 属性及信号，验证接线；不冒充 Cocoa 原生选择器验收。
        QVERIFY(dialog->setProperty("selectedFile", selectedUrl));
        QVERIFY(QMetaObject::invokeMethod(dialog, "rejected"));
        QCOMPARE(opened.count(), 0);
        QCOMPARE(project->currentPath(), previousPath);
        QCOMPARE(project->currentName(), QStringLiteral("Unsaved current"));
        QVERIFY(project->dirty());

        QCOMPARE(project->openDocuments(), previousDocuments);

        for (const bool disappears : {true, false}) {
            const QString path = fixture.filePath(disappears ? QStringLiteral("disappearing.panta")
                                                             : QStringLiteral("malformed.panta"));
            QFile file(path);
            QVERIFY(file.open(QIODevice::WriteOnly));
            QCOMPARE(file.write("{}"), 2);
            file.close();
            const QUrl invalid = QUrl::fromLocalFile(path);
            // FileDialog 拒绝预选不存在的文件；先选真实文件，再模拟选择后的删除。
            QVERIFY(dialog->setProperty("selectedFile", invalid));
            QCOMPARE(dialog->property("selectedFile").toUrl(), invalid);
            if (disappears)
                QVERIFY(QFile::remove(path));
            QVERIFY(QMetaObject::invokeMethod(dialog, "accepted"));
            QTRY_VERIFY(!project->projectOperationPending());
            QCOMPARE(opened.count(), 0);
            QVERIFY(!project->errorCode().isEmpty());
            QCOMPARE(project->currentPath(), previousPath);
            QCOMPARE(project->currentName(), QStringLiteral("Unsaved current"));
            QVERIFY(project->dirty());

            QCOMPARE(project->openDocuments(), previousDocuments);
        }

        QVERIFY(dialog->setProperty("selectedFile", selectedUrl));
        QVERIFY(QMetaObject::invokeMethod(dialog, "accepted"));
        QTRY_VERIFY(!project->projectOperationPending());
        QTRY_COMPARE(opened.count(), 1);
        QCOMPARE(opened.constFirst().constFirst().toString(), selectedPath);
        QCOMPARE(project->currentPath(), selectedPath);
        QCOMPARE(project->currentName(), storedName);
        QVERIFY(!project->dirty());
        QVERIFY(project->errorCode().isEmpty());
        QVERIFY(project->activeDocumentId().isEmpty());
        QVERIFY(project->openDocuments().isEmpty());
    }

    void import_preview_waits_for_background_result_and_close_cancels() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        const auto path = fixture.filePath(QStringLiteral("preview.stl"));
        QFile source(path);
        QVERIFY(source.open(QIODevice::WriteOnly));
        QVERIFY(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n") > 0);
        source.close();
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* root = engine.rootObjects().constFirst();
        auto* dialog = root->findChild<QObject*>(QStringLiteral("importDialog"));
        auto* model =
            root->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        QVERIFY(dialog && model);
        auto* accept = dialog->findChild<QObject*>(QStringLiteral("importAccept"));
        QVERIFY(accept);
        QSignalSpy finished(model, &panta::bridge::ProjectViewModel::importPreviewFinished);
        QVERIFY(finished.isValid());
        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        dialog->setProperty("sourcePath", path);
        QVERIFY(model->inspectStl(path));
        QVERIFY(model->importPreviewPending());
        QVERIFY(!accept->property("enabled").toBool());
        QVERIFY(QMetaObject::invokeMethod(dialog, "close"));
        QVERIFY(!model->importPreviewPending());
        QVERIFY(!model->importPreviewReady());
        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        dialog->setProperty("sourcePath", path);
        QVERIFY(model->inspectStl(path));
        QTRY_VERIFY(model->importPreviewReady());
        QVERIFY(accept->property("enabled").toBool());
        QCOMPARE(finished.count(), 1);
        QVERIFY(QMetaObject::invokeMethod(dialog, "close"));
        QVERIFY(!model->importPreviewReady());
        QVERIFY(!accept->property("enabled").toBool());
    }

    void import_completion_does_not_close_a_reopened_dialog() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        const auto path = fixture.filePath(QStringLiteral("reopen.stl"));
        QFile source(path);
        QVERIFY(source.open(QIODevice::WriteOnly));
        QVERIFY(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n") > 0);
        source.close();

        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* root = engine.rootObjects().constFirst();
        auto* dialog = root->findChild<QObject*>(QStringLiteral("importDialog"));
        auto* model =
            root->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        QVERIFY(dialog && model);
        QVERIFY(model->createProject(QStringLiteral("Reopen"), fixture.path()));
        QTRY_VERIFY(!model->projectOperationPending());
        QVERIFY(model->errorCode().isEmpty());
        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        dialog->setProperty("sourcePath", path);
        QVERIFY(model->inspectStl(path));
        QTRY_VERIFY(model->importPreviewReady());
        QVERIFY(QMetaObject::invokeMethod(dialog, "submit"));
        QVERIFY(model->importPending());
        const auto request = dialog->property("pendingImportRequestId").toULongLong();
        QVERIFY(request > 0);

        QVERIFY(QMetaObject::invokeMethod(dialog, "close"));
        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        QVERIFY(dialog->property("visible").toBool());
        QVERIFY(QMetaObject::invokeMethod(model, "projectImportFinished", Qt::DirectConnection,
                                          Q_ARG(quint64, request), Q_ARG(bool, true)));
        QVERIFY(dialog->property("visible").toBool());
        QTRY_VERIFY(!model->importPending());
        QVERIFY(dialog->property("visible").toBool());
        QVERIFY(dialog->property("pendingImportRequestId").toULongLong() == 0);
    }

    // task 035：体网格 + 切面上色 + 质量诊断全部展开时，侧栏内可见控件不越过侧栏右边界。
    // DPR 1.25 / 1.5 / 2.0 由 CTest 以 QT_SCALE_FACTOR 重跑本用例（见 native/app/CMakeLists.txt）。
    void sidebar_controls_stay_within_sidebar_bounds() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        const QString pvdPath =
            QDir::cleanPath(QDir(QString::fromUtf8(PANTA_STRINGIFY(PANTA_SHELL_MESH_FIXTURE_DIR)))
                                .filePath(QStringLiteral("pvd/flow.pvd")));
        QVERIFY(QFileInfo::exists(pvdPath));
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* window = qobject_cast<QQuickWindow*>(engine.rootObjects().constFirst());
        QVERIFY(window);
        auto* model =
            window->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* sidebar = window->findChild<QQuickItem*>(QStringLiteral("assetSidebar"));
        QVERIFY(model && sidebar);
        QVERIFY(model->createProject(QStringLiteral("Bounds"), fixture.path()));
        QTRY_VERIFY(!model->projectOperationPending());
        QVERIFY(
            model->importStl(pvdPath, QStringLiteral("solid-3d"), QStringLiteral("meters"), false));
        QTRY_VERIFY_WITH_TIMEOUT(model->volumeAvailable(), 15000);
        QVERIFY(model->setSectionField(QStringLiteral("T"), QStringLiteral("node"), -1));
        QVERIFY(model->setSection(true, 0.0, 0.0, 1.0, 0.25));
        QTRY_VERIFY_WITH_TIMEOUT(model->fieldLegendVisible(), 15000);
        model->beginDiagnoseActiveMesh();
        auto* quality = window->findChild<QQuickItem*>(QStringLiteral("meshQualityPanel"));
        QVERIFY(quality);
        QTRY_VERIFY_WITH_TIMEOUT(quality->isVisible(), 15000);
        // task 037：属性面板按节点类型与页签显示；逐一选中检查控件可见。
        const QString asset = QStringLiteral(":") + model->activeDocumentId();
        const auto show = [&](const QString& kind, int tab,
                              std::initializer_list<const char*> names) {
            QVERIFY(QMetaObject::invokeMethod(sidebar, "selectNode", Q_ARG(QString, kind + asset)));
            sidebar->setProperty("propertiesTab", tab);
            for (const char* name : names) {
                auto* item = visual_item(sidebar, QString::fromLatin1(name));
                QVERIFY2(item, name);
                QTRY_VERIFY2(item->isVisible(), name);
            }
        };
        show(QStringLiteral("field"), 1, {"fieldLegend"});
        show(QStringLiteral("field"), 0, {"fieldProperties", "fieldRangeModeRow"});
        show(QStringLiteral("asset"), 1, {"visibilityProperties", "nodeVisibleToggle"});
        show(QStringLiteral("asset"), 2, {"infoRow"});
        QVERIFY(QMetaObject::invokeMethod(sidebar, "selectNode",
                                          Q_ARG(QString, QStringLiteral("project"))));
        sidebar->setProperty("propertiesTab", 0);
        QTRY_VERIFY(visual_item(sidebar, QStringLiteral("openAnimationButton"))->isVisible());
        show(QStringLiteral("section"), 0,
             {"sectionProperties", "sectionNormalZ", "sectionSlider"});
        // task 036：诊断结果进入底部面板「诊断」页，并自动展开一次。
        auto* bottom = window->findChild<QQuickItem*>(QStringLiteral("bottomPanel"));
        QVERIFY(bottom && !bottom->property("collapsed").toBool());
        QCOMPARE(bottom->property("currentIndex").toInt(), 0);
        QVERIFY(bottom->isAncestorOf(quality));
        QVERIFY(!sidebar->isAncestorOf(quality));
        for (const QSize size : {QSize(1280, 720), QSize(1600, 900)}) {
            window->resize(size);
            QTRY_COMPARE(window->width(), size.width());
            QTRY_COMPARE(sidebar->width(), 300.0);
            // 等布局稳定后再遍历，避免读到 polish 之前的旧几何。
            QTest::qWait(50);
            const auto right = sidebar->mapToScene(QPointF(sidebar->width(), 0)).x() + 0.5;
            QStringList overflow;
            const std::function<void(QQuickItem*)> visit = [&](QQuickItem* item) {
                for (auto* child : item->childItems()) {
                    if (!child->isVisible() || child->width() <= 0.0)
                        continue;
                    const auto edge = child->mapToScene(QPointF(child->width(), 0)).x();
                    if (edge > right)
                        overflow.append(
                            QStringLiteral("%1(%2) 右缘 %3 > %4")
                                .arg(child->objectName(),
                                     QString::fromLatin1(child->metaObject()->className()))
                                .arg(edge)
                                .arg(right));
                    visit(child);
                }
            };
            visit(sidebar);
            QVERIFY2(overflow.isEmpty(), qPrintable(overflow.join(QStringLiteral("\n"))));
            // 纵向：侧栏直接子项都在窗口内，超出部分只能在详情滚动区内，滚到底能看到最后一项。
            for (auto* child : sidebar->childItems()) {
                if (child->isVisible())
                    QVERIFY(child->mapToScene(QPointF(0, child->height())).y() <=
                            window->height() + 0.5);
            }
            auto* scroll = visual_item(sidebar, QStringLiteral("sidebarDetailsScroll"));
            auto* details = visual_item(sidebar, QStringLiteral("sidebarDetails"));
            auto* scrollBar = visual_item(sidebar, QStringLiteral("sidebarDetailsScrollBar"));
            auto* last = visual_item(sidebar, QStringLiteral("sectionSlider"));
            QVERIFY(scroll && details && scrollBar && last);
            // 滚动条只在内容超出时出现；出现时滚到底能看到最后一项。
            const bool overflows = details->height() > scroll->height() + 0.5;
            QCOMPARE(scrollBar->isVisible(), overflows);
            if (overflows)
                scrollBar->setProperty("position", 1.0 - scrollBar->property("size").toReal());
            QTRY_VERIFY(last->mapToScene(QPointF(0, last->height())).y() <=
                        scroll->mapToScene(QPointF(0, scroll->height())).y() + 0.5);
            scrollBar->setProperty("position", 0.0);
            // 状态栏单行字段不越出窗口；拾取读数为单行。
            auto* pick = window->findChild<QQuickItem*>(QStringLiteral("pickReadout"));
            QVERIFY(pick && pick->isVisible());
            QVERIFY(!pick->property("text").toString().contains(QLatin1Char('\n')));
            QVERIFY(pick->mapToScene(QPointF(pick->width(), 0)).x() <= window->width() + 0.5);
            // 设置 PANTA_SHELL_SCREENSHOT_DIR 时保存离屏截图，供任务验收记录（不进 CI 断言）。
            if (const auto dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR");
                !dir.isEmpty()) {
                QTest::qWait(50);
                const auto image = window->grabWindow();
                QVERIFY(image.save(QDir(dir).filePath(QStringLiteral("sidebar-%1x%2-dpr%3.png")
                                                          .arg(size.width())
                                                          .arg(size.height())
                                                          .arg(window->devicePixelRatio()))));
            }
        }
    }

    // task 035：错误只在对话框内出现一次；占位文字用独立灰；导入单位来源可见；
    // 文件选择器优先使用系统原生对话框（不带 DontUseNativeDialog）。
    void dialogs_show_errors_once_and_prefer_native_file_pickers() {
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* root = engine.rootObjects().constFirst();
        auto* model =
            root->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* newDialog = root->findChild<QObject*>(QStringLiteral("newProjectDialog"));
        auto* importDialog = root->findChild<QObject*>(QStringLiteral("importDialog"));
        auto* status = root->findChild<QObject*>(QStringLiteral("statusMessage"));
        QVERIFY(model && newDialog && importDialog && status);

        auto* nameField = newDialog->findChild<QObject*>(QStringLiteral("projectNameField"));
        auto* errorLabel = newDialog->findChild<QObject*>(QStringLiteral("newProjectError"));
        QVERIFY(nameField && errorLabel);
        QCOMPARE(nameField->property("placeholderTextColor").value<QColor>(),
                 QColor(QStringLiteral("#8c959f")));

        QVERIFY(QMetaObject::invokeMethod(newDialog, "open"));
        QTRY_VERIFY(newDialog->property("visible").toBool());
        nameField->setProperty("text", QString());
        QVERIFY(QMetaObject::invokeMethod(newDialog, "submit"));
        QTRY_VERIFY(!model->error().isEmpty());
        QTRY_VERIFY(errorLabel->property("visible").toBool());
        QVERIFY(!status->property("text").toString().contains(model->error()));
        if (const QString dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR");
            !dir.isEmpty()) {
            auto* newWindow = qobject_cast<QQuickWindow*>(newDialog);
            QVERIFY(newWindow);
            QTest::qWait(100);
            QVERIFY(newWindow->grabWindow().save(
                QDir(dir).filePath(QStringLiteral("new-project-error.png"))));
        }
        QVERIFY(QMetaObject::invokeMethod(newDialog, "close"));
        QTRY_VERIFY(!newDialog->property("visible").toBool());
        if (!model->error().isEmpty())
            QCOMPARE(status->property("text").toString(), model->error());

        auto* unitSource = importDialog->findChild<QObject*>(QStringLiteral("importUnitSource"));
        QVERIFY(unitSource);
        QVERIFY(QMetaObject::invokeMethod(importDialog, "openWithFile",
                                          Q_ARG(QString, QStringLiteral("/missing/case.foam"))));
        QTRY_VERIFY(importDialog->property("visible").toBool());
        QVERIFY(unitSource->property("text").toString().contains(QStringLiteral("SI 米制")));
        QVERIFY(QMetaObject::invokeMethod(importDialog, "selectSource",
                                          Q_ARG(QString, QStringLiteral("/missing/part.stl"))));
        QVERIFY(unitSource->property("text").toString().contains(QStringLiteral("默认")));
        // 高度随内容收紧：确定按钮完整落在窗口内，底部不留大片空白。
        auto* accept = importDialog->findChild<QQuickItem*>(QStringLiteral("importAccept"));
        QVERIFY(accept);
        const qreal windowHeight = importDialog->property("height").toReal();
        QTRY_VERIFY(accept->mapToScene(QPointF(0, accept->height())).y() <= windowHeight + 0.5);
        QVERIFY(windowHeight - accept->mapToScene(QPointF(0, accept->height())).y() < 40);
        if (const QString dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR");
            !dir.isEmpty()) {
            auto* importWindow = qobject_cast<QQuickWindow*>(importDialog);
            QVERIFY(importWindow);
            QTest::qWait(100);
            QVERIFY(importWindow->grabWindow().save(
                QDir(dir).filePath(QStringLiteral("import-dialog.png"))));
        }
        QVERIFY(QMetaObject::invokeMethod(importDialog, "close"));

        int fileDialogs = 0;
        const auto objects = root->findChildren<QObject*>();
        for (auto* object : objects) {
            if (!QByteArray(object->metaObject()->className()).contains("FileDialog") &&
                !QByteArray(object->metaObject()->className()).contains("FolderDialog"))
                continue;
            const int index = object->metaObject()->indexOfProperty("options");
            if (index < 0)
                continue;
            ++fileDialogs;
            QVERIFY2(object->property("options").toInt() == 0,
                     qPrintable(object->objectName() + QStringLiteral(" 不应禁用原生对话框")));
        }
        QVERIFY(fileDialogs >= 5);
    }

    // task 040：快捷键表无重复、避开系统保留组合；按键驱动布局开关与帮助窗口。
    void keyboard_shortcuts_are_unique_and_open_help() {
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* window = qobject_cast<QQuickWindow*>(engine.rootObjects().constFirst());
        auto* settings = engine.singletonInstance<panta::bridge::Settings*>(
            QStringLiteral("Panta.Bridge"), QStringLiteral("Settings"));
        auto* instantiator = window->findChild<QObject*>(QStringLiteral("shortcutInstantiator"));
        QVERIFY(window && settings && instantiator);
        const QVariantList table = window->property("shortcutTable").toList();
        QCOMPARE(instantiator->property("count").toInt(), table.size());
        // 桌面环境 / 输入法 / 窗口管理器占用的组合（Linux、Windows、macOS 上 Ctrl 映射为 ⌘）。
        const QList<QKeySequence> reserved{QKeySequence(QStringLiteral("Ctrl+Space")),
                                           QKeySequence(QStringLiteral("Alt+Tab")),
                                           QKeySequence(QStringLiteral("Alt+F4")),
                                           QKeySequence(QStringLiteral("Ctrl+Esc")),
                                           QKeySequence(QStringLiteral("Ctrl+Alt+Del")),
                                           QKeySequence(QStringLiteral("Ctrl+Alt+T")),
                                           QKeySequence(QStringLiteral("Ctrl+Alt+Left")),
                                           QKeySequence(QStringLiteral("Ctrl+Alt+Right")),
                                           QKeySequence(QStringLiteral("Ctrl+Alt+Up")),
                                           QKeySequence(QStringLiteral("Ctrl+Alt+Down")),
                                           QKeySequence(QStringLiteral("Ctrl+H")),
                                           QKeySequence(QStringLiteral("Ctrl+M")),
                                           QKeySequence(QStringLiteral("Ctrl+Q")),
                                           QKeySequence(QStringLiteral("Ctrl+Tab")),
                                           QKeySequence(QStringLiteral("Ctrl+Shift+3")),
                                           QKeySequence(QStringLiteral("Ctrl+Shift+4")),
                                           QKeySequence(QStringLiteral("Ctrl+Shift+5")),
                                           QKeySequence(QStringLiteral("Ctrl+W"))};
        QSet<QString> seen;
        for (int i = 0; i < instantiator->property("count").toInt(); ++i) {
            QObject* shortcut = nullptr;
            QVERIFY(QMetaObject::invokeMethod(instantiator, "objectAt",
                                              Q_RETURN_ARG(QObject*, shortcut), Q_ARG(int, i)));
            QVERIFY(shortcut);
            const QString id = shortcut->property("actionId").toString();
            const QString text = shortcut->property("nativeText").toString();
            QVERIFY2(!text.isEmpty(), qPrintable(id));
            QVERIFY2(!seen.contains(text), qPrintable(id + QStringLiteral(" 重复：") + text));
            seen.insert(text);
            const QKeySequence sequence = QKeySequence::fromString(text, QKeySequence::NativeText);
            QCOMPARE(sequence.count(), 1);
            const auto combination = sequence[0];
            QVERIFY2(!combination.keyboardModifiers().testFlag(Qt::MetaModifier), qPrintable(id));
            // 单键只允许 F1（帮助）。
            if (combination.keyboardModifiers() == Qt::NoModifier)
                QCOMPARE(combination.key(), Qt::Key_F1);
            for (const auto& blocked : reserved)
                QVERIFY2(sequence != blocked,
                         qPrintable(id + QStringLiteral(" 占用系统组合 ") + text));
        }

        window->requestActivate();
        QVERIFY(QTest::qWaitForWindowExposed(window));
        QVERIFY(!settings->sidebarCollapsed());
        QTest::keyClick(window, Qt::Key_B, Qt::ControlModifier);
        QTRY_VERIFY(settings->sidebarCollapsed());
        QTest::keyClick(window, Qt::Key_B, Qt::ControlModifier);
        QTRY_VERIFY(!settings->sidebarCollapsed());
        QVERIFY(settings->bottomPanelCollapsed());
        QTest::keyClick(window, Qt::Key_J, Qt::ControlModifier);
        QTRY_VERIFY(!settings->bottomPanelCollapsed());
        QTest::keyClick(window, Qt::Key_J, Qt::ControlModifier);
        QTRY_VERIFY(settings->bottomPanelCollapsed());
        // 无工程时导出 / 诊断 / 视图快捷键不触发。
        auto* exportShortcut = window->findChild<QObject*>(QStringLiteral("shortcut_export"));
        QVERIFY(exportShortcut && !exportShortcut->property("enabled").toBool());

        auto* help = window->findChild<QQuickWindow*>(QStringLiteral("shortcutHelpDialog"));
        QVERIFY(help);
        QTest::keyClick(window, Qt::Key_F1);
        QTRY_VERIFY(help->isVisible());
        auto* rows = help->findChild<QObject*>(QStringLiteral("shortcutHelpRows"));
        QVERIFY(rows);
        QCOMPARE(rows->property("count").toInt(), table.size());
        const QVariantList listed = help->property("shortcuts").toList();
        for (const auto& entry : listed)
            QVERIFY(!entry.toMap().value(QStringLiteral("keys")).toString().isEmpty());
        if (const auto dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR"); !dir.isEmpty()) {
            QTest::qWait(100);
            QVERIFY(
                help->grabWindow().save(QDir(dir).filePath(QStringLiteral("shortcut-help.png"))));
        }
        QVERIFY(QMetaObject::invokeMethod(help, "close"));
        QTRY_VERIFY(!help->isVisible());
    }

    // task 040：拖入不支持的文件只给中文错误，不打开导入窗口，工程保持未修改。
    void dropping_unsupported_file_reports_error_and_keeps_project_clean() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        const QString notes = fixture.filePath(QStringLiteral("notes.txt"));
        QFile file(notes);
        QVERIFY(file.open(QIODevice::WriteOnly));
        QVERIFY(file.write("not a mesh") > 0);
        file.close();
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* root = engine.rootObjects().constFirst();
        auto* project =
            root->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* importDialog = root->findChild<QObject*>(QStringLiteral("importDialog"));
        auto* status = root->findChild<QObject*>(QStringLiteral("statusMessage"));
        QVERIFY(project && importDialog && status);
        QVERIFY(project->createProject(QStringLiteral("Drop"), fixture.path()));
        QTRY_VERIFY(!project->projectOperationPending());
        QVERIFY(!project->dirty());
        const QString path = project->currentPath();
        QVERIFY(QMetaObject::invokeMethod(
            root, "handleDrop", Q_ARG(QList<QUrl>, QList<QUrl>{QUrl::fromLocalFile(notes)})));
        QTRY_VERIFY(
            status->property("text").toString().startsWith(QStringLiteral("不支持的文件类型")));
        QCOMPARE(root->property("noticeLevel").toString(), QStringLiteral("error"));
        QVERIFY(!importDialog->property("visible").toBool());
        QVERIFY(!project->dirty());
        QCOMPARE(project->currentPath(), path);
        QCOMPARE(project->objectTree()->nodeCount(), 1);
    }

    // task 040：导入窗口显示识别格式；预检尺寸偏小时提示可能按米存储，但不自动改选单位。
    void import_dialog_shows_format_and_unit_hint() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        const QString stlPath = fixture.filePath(QStringLiteral("small.stl"));
        QFile stl(stlPath);
        QVERIFY(stl.open(QIODevice::WriteOnly));
        QVERIFY(stl.write("vertex 0 0 0\nvertex 0.5 0 0\nvertex 0 0.25 0\n") > 0);
        stl.close();
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* root = engine.rootObjects().constFirst();
        auto* project =
            root->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* dialog = root->findChild<QObject*>(QStringLiteral("importDialog"));
        QVERIFY(project && dialog);
        auto* format = dialog->findChild<QObject*>(QStringLiteral("importFormatText"));
        auto* hint = dialog->findChild<QObject*>(QStringLiteral("importUnitHint"));
        auto* field = dialog->findChild<QObject*>(QStringLiteral("importSourceField"));
        auto* units = dialog->findChild<QObject*>(QStringLiteral("importUnitsCombo"));
        QVERIFY(format && hint && field && units);
        QCOMPARE(project->importFormatText(QStringLiteral("/a/b.vtu")),
                 QStringLiteral("VTK XML 非结构网格（.vtu）"));
        QVERIFY(project->importFormatText(QStringLiteral("/a/b.step")).isEmpty());

        QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
        QCOMPARE(format->property("text").toString(), QStringLiteral("—"));
        QVERIFY(QMetaObject::invokeMethod(dialog, "openWithFile", Q_ARG(QString, stlPath)));
        QCOMPARE(field->property("text").toString(), stlPath);
        QCOMPARE(format->property("text").toString(), QStringLiteral("STL 表面网格"));
        QTRY_VERIFY(project->importPreviewReady());
        QCOMPARE(project->importPreviewSuggestedUnits(), QStringLiteral("meters"));
        QTRY_VERIFY(hint->property("visible").toBool());
        QVERIFY(hint->property("text").toString().contains(QStringLiteral("米")));
        // 只提示：当前选择仍为默认毫米。
        QCOMPARE(units->property("currentIndex").toInt(), 0);
        if (const auto dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR"); !dir.isEmpty()) {
            QTest::qWait(100);
            QVERIFY(qobject_cast<QQuickWindow*>(dialog)->grabWindow().save(
                QDir(dir).filePath(QStringLiteral("import-unit-hint.png"))));
        }
        units->setProperty("currentIndex", 2);
        QTRY_VERIFY(!hint->property("visible").toBool());
        // 手动输入路径：编辑完成后按新路径识别。
        QVERIFY(QMetaObject::invokeMethod(
            dialog, "editSource", Q_ARG(QString, fixture.filePath(QStringLiteral("x.txt")))));
        QCOMPARE(format->property("text").toString(), QStringLiteral("无法识别的扩展名"));
        QVERIFY(QMetaObject::invokeMethod(dialog, "editSource", Q_ARG(QString, QString())));
        QCOMPARE(format->property("text").toString(), QStringLiteral("—"));
        QVERIFY(QMetaObject::invokeMethod(dialog, "close"));
    }

    // task 040：有活动体网格时，诊断 / 切面 / 播放 /
    // 截图快捷键与对象树键盘菜单都可用；截图倍率越界被拒。
    void workflow_shortcuts_and_tree_menu_drive_active_mesh() {
        QTemporaryDir fixture;
        QVERIFY(fixture.isValid());
        const QString pvdPath =
            QDir::cleanPath(QDir(QString::fromUtf8(PANTA_STRINGIFY(PANTA_SHELL_MESH_FIXTURE_DIR)))
                                .filePath(QStringLiteral("pvd/flow.pvd")));
        QQmlApplicationEngine engine;
        panta::install_icon_provider(engine);
        engine.loadFromModule(QStringLiteral("Panta.Shell"), QStringLiteral("App"));
        QVERIFY(!engine.rootObjects().isEmpty());
        auto* window = qobject_cast<QQuickWindow*>(engine.rootObjects().constFirst());
        QVERIFY(window);
        auto* model =
            window->findChild<panta::bridge::ProjectViewModel*>(QStringLiteral("projectModel"));
        auto* sidebar = window->findChild<QQuickItem*>(QStringLiteral("assetSidebar"));
        auto* treeView = window->findChild<QQuickItem*>(QStringLiteral("objectTree"));
        auto* viewport = window->findChild<QObject*>(QStringLiteral("caeViewport"));
        auto* screenshot = window->findChild<QQuickWindow*>(QStringLiteral("screenshotDialog"));
        auto* menu = window->findChild<QObject*>(QStringLiteral("objectTreeMenu"));
        QVERIFY(model && sidebar && treeView && viewport && screenshot && menu);
        QVERIFY(model->createProject(QStringLiteral("Keys"), fixture.path()));
        QTRY_VERIFY(!model->projectOperationPending());
        QVERIFY(
            model->importStl(pvdPath, QStringLiteral("solid-3d"), QStringLiteral("meters"), false));
        QTRY_VERIFY_WITH_TIMEOUT(model->volumeAvailable(), 15000);
        QTRY_VERIFY(model->playbackActive());
        QVERIFY(model->playbackFrameCount() > 1);
        window->requestActivate();
        QVERIFY(QTest::qWaitForWindowExposed(window));

        // task 034 验收：拾取、单位、LOD / 流式、时间步只在状态栏显示，侧栏与播放栏不重复。
        auto* statusBar = window->findChild<QQuickItem*>(QStringLiteral("statusBar"));
        QVERIFY(statusBar);
        for (const char* name : {"pickReadout", "statusUnits", "statusTimeStep"}) {
            auto* field = visual_item(statusBar, QString::fromLatin1(name));
            QVERIFY2(field, name);
            QTRY_VERIFY2(field->isVisible(), name);
            QVERIFY2(!visual_item(sidebar, QString::fromLatin1(name)), name);
        }
        QVERIFY(!visual_item(sidebar, QStringLiteral("activeMeshLodStatus")));
        QCOMPARE(visual_item(statusBar, QStringLiteral("statusUnits"))->property("text").toString(),
                 QStringLiteral("单位 mm"));
        QCOMPARE(
            visual_item(statusBar, QStringLiteral("statusTimeStep"))->property("text").toString(),
            QStringLiteral("时间步 %1/%2")
                .arg(model->playbackFrame() + 1)
                .arg(model->playbackFrameCount()));
        // 播放中长状态文字归状态栏，播放栏状态行为空。
        QVERIFY(window->findChild<QObject*>(QStringLiteral("playbackStatus"))
                    ->property("text")
                    .toString()
                    .isEmpty());
        // LOD / 流式字段：有状态文字时出现在状态栏。
        auto* lod = visual_item(statusBar, QStringLiteral("activeMeshLodStatus"));
        QVERIFY(lod);
        QCOMPARE(lod->isVisible(), !model->activeMeshLodStatus().isEmpty());

        auto* quality = window->findChild<QQuickItem*>(QStringLiteral("meshQualityPanel"));
        QVERIFY(quality);
        QTest::keyClick(window, Qt::Key_D, Qt::ControlModifier);
        QTRY_VERIFY_WITH_TIMEOUT(quality->isVisible(), 15000);

        QVERIFY(!model->sectionActive());
        QTest::keyClick(window, Qt::Key_X, Qt::ControlModifier | Qt::ShiftModifier);
        QTRY_VERIFY(model->sectionActive());
        QTest::keyClick(window, Qt::Key_X, Qt::ControlModifier | Qt::ShiftModifier);
        QTRY_VERIFY(!model->sectionActive());

        // 导入后默认显示末帧；先回首帧，再逐帧前进，避免循环回绕影响断言。
        QTest::keyClick(window, Qt::Key_Home, Qt::ControlModifier | Qt::ShiftModifier);
        QTRY_COMPARE(model->playbackFrame(), 0);
        QTest::keyClick(window, Qt::Key_Right, Qt::ControlModifier | Qt::ShiftModifier);
        QTRY_COMPARE(model->playbackFrame(), 1);
        QTest::keyClick(window, Qt::Key_Left, Qt::ControlModifier | Qt::ShiftModifier);
        QTRY_COMPARE(model->playbackFrame(), 0);
        QTest::keyClick(window, Qt::Key_End, Qt::ControlModifier | Qt::ShiftModifier);
        QTRY_COMPARE(model->playbackFrame(), model->playbackFrameCount() - 1);

        QTest::keyClick(window, Qt::Key_P, Qt::ControlModifier | Qt::ShiftModifier);
        QTRY_VERIFY(screenshot->isVisible());
        QCOMPARE(screenshot->property("scale").toInt(), 1);
        QVERIFY(screenshot->property("includeLegend").toBool());
        if (const auto dir = qEnvironmentVariable("PANTA_SHELL_SCREENSHOT_DIR"); !dir.isEmpty()) {
            QTest::qWait(100);
            QVERIFY(screenshot->grabWindow().save(
                QDir(dir).filePath(QStringLiteral("screenshot-options.png"))));
        }
        QVERIFY(QMetaObject::invokeMethod(screenshot, "close"));
        const QUrl png = QUrl::fromLocalFile(fixture.filePath(QStringLiteral("view.png")));
        const auto save = [&](const QUrl& url, int scale) {
            QString failure;
            [&] {
                QVERIFY(QMetaObject::invokeMethod(viewport, "saveScreenshot",
                                                  Q_RETURN_ARG(QString, failure), Q_ARG(QUrl, url),
                                                  Q_ARG(int, scale), Q_ARG(bool, false)));
            }();
            return failure;
        };
        QCOMPARE(save(png, 0), QStringLiteral("截图倍率须为 1–3 倍。"));
        QCOMPARE(save(png, 4), QStringLiteral("截图倍率须为 1–3 倍。"));
        QCOMPARE(save(QUrl::fromLocalFile(fixture.filePath(QStringLiteral("view.jpg"))), 2),
                 QStringLiteral("视口截图仅支持 PNG 格式。"));
        // offscreen 没有原生 surface，合法倍率也只能得到「未就绪」；真实读回需目标平台 GPU 验证。
        QCOMPARE(save(png, 2), QStringLiteral("视口尚未就绪，无法截图。"));

        // 对象树：选中资产节点后 Shift+F10 打开节点菜单，命令随节点类型与活动状态启用。
        const QString assetNode = QStringLiteral("asset:") + model->activeDocumentId();
        treeView->forceActiveFocus();
        QVERIFY(QMetaObject::invokeMethod(sidebar, "selectNode", Q_ARG(QString, assetNode)));
        QTest::keyClick(window, Qt::Key_F10, Qt::ShiftModifier);
        QTRY_VERIFY(menu->property("opened").toBool());
        QCOMPARE(menu->property("nodeId").toString(), assetNode);
        const QVariantList entries = menu->property("entries").toList();
        QStringList commands;
        for (const auto& entry : entries)
            commands.append(entry.toList().constFirst().toString());
        QCOMPARE(commands, (QStringList{QStringLiteral("activate"), QStringLiteral("visibility"),
                                        QStringLiteral("diagnose"), QStringLiteral("export")}));
        QVERIFY(entries.at(3).toList().at(2).toBool());
        QVERIFY(QMetaObject::invokeMethod(menu, "close"));
        QTRY_VERIFY(!menu->property("opened").toBool());
        // 「隐藏」只改显示，工程不变脏（与 037 眼睛按钮同一路径）。
        const bool wasDirty = model->dirty();
        QVERIFY(QMetaObject::invokeMethod(sidebar, "runNodeCommand", Q_ARG(QString, assetNode),
                                          Q_ARG(QString, QStringLiteral("visibility"))));
        QTRY_VERIFY(!model->activeMeshVisible());
        QCOMPARE(model->dirty(), wasDirty);
        QVERIFY(QMetaObject::invokeMethod(sidebar, "runNodeCommand", Q_ARG(QString, assetNode),
                                          Q_ARG(QString, QStringLiteral("visibility"))));
        QTRY_VERIFY(model->activeMeshVisible());
        // 上色：属性面板的字段 / 颜色图下拉框可经 Tab 到达，方向键即切换（ComboBox activated）。
        auto* fieldCombo = window->findChild<QQuickItem*>(QStringLiteral("sectionField"));
        auto* colorMapCombo = window->findChild<QQuickItem*>(QStringLiteral("fieldColorMapCombo"));
        QVERIFY(fieldCombo);
        QVERIFY(colorMapCombo);
        QVERIFY(QMetaObject::invokeMethod(
            sidebar, "selectNode",
            Q_ARG(QString, QStringLiteral("field:") + model->activeDocumentId())));
        sidebar->setProperty("propertiesTab", 0);
        QVERIFY(fieldCombo->activeFocusOnTab());
        QVERIFY(colorMapCombo->activeFocusOnTab());
        QTRY_VERIFY(colorMapCombo->isVisible());
        const int colorMap = model->property("fieldColorMap").toInt();
        colorMapCombo->forceActiveFocus(Qt::TabFocusReason);
        QTest::keyClick(window, Qt::Key_Down);
        QTRY_COMPARE(model->property("fieldColorMap").toInt(), colorMap + 1);
        QTest::keyClick(window, Qt::Key_Up);
        QTRY_COMPARE(model->property("fieldColorMap").toInt(), colorMap);
        // 工程节点：导入 / 保存。焦点回到对象树后用 Menu 键打开。
        treeView->forceActiveFocus();
        QVERIFY(QMetaObject::invokeMethod(sidebar, "selectNode",
                                          Q_ARG(QString, QStringLiteral("project"))));
        QTest::keyClick(window, Qt::Key_Menu);
        QTRY_VERIFY(menu->property("opened").toBool());
        QCOMPARE(menu->property("entries").toList().size(), 2);
        QVERIFY(QMetaObject::invokeMethod(menu, "close"));
    }

#endif
};

QTEST_MAIN(ShellModuleLoadTest)
#include "shell_module_load_test.moc"
