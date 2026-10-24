// 最小工作台的工程、导入、资产列表与文档接线回归。
#include "../../support/qml/quick_item_helpers.hpp"
#include <QByteArray>
#include <QColor>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QMetaEnum>
#include <QMetaProperty>
#include <QPointF>
#include <QQmlApplicationEngine>
#include <QQuickItem>
#include <QQuickWindow>
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
        // task 033：统一播放栏随 Shell 加载；无播放来源时隐藏。
        auto* playback = root->findChild<QObject*>(QStringLiteral("playbackBar"));
        QVERIFY(playback);
        QVERIFY(!playback->property("visible").toBool());
        QVERIFY(!root->findChild<QObject*>(QStringLiteral("saveProjectButton"))
                     ->property("enabled")
                     .toBool());
        QVERIFY(!root->findChild<QObject*>(QStringLiteral("importStlButton"))
                     ->property("enabled")
                     .toBool());
#endif
    }
#ifdef PANTA_ENABLE_BRIDGE_MODULE
    void workspace_keeps_sidebar_bounded_and_viewport_visible() {
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
        QVERIFY(sidebar && pane && viewport && tabs);
        for (const QSize size : {QSize(1280, 800), QSize(1600, 1000)}) {
            window->resize(size);
            QTRY_COMPARE(sidebar->width(), 240.0);
            QTRY_VERIFY(pane->width() > window->width() * 0.7);
            QTRY_VERIFY(pane->height() > window->height() * 0.8);
            QTRY_COMPARE(viewport->width(), pane->width());
            QTRY_VERIFY(viewport->height() > pane->height() - 50.0);
            QTRY_COMPARE(tabs->width(), pane->width());
            QVERIFY(tabs->height() > 0.0);
            QVERIFY(viewport->isVisible());
            QVERIFY(tabs->isVisible());
            const auto sidebarRight = sidebar->mapToScene(QPointF(sidebar->width(), 0)).x();
            QVERIFY(pane->mapToScene(QPointF()).x() >= sidebarRight);
            QVERIFY(pane->mapToScene(QPointF(pane->width(), 0)).x() <= window->width());
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
                                              Q_ARG(bool, false)));
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
        auto* assets = root->findChild<QQuickItem*>(QStringLiteral("assetList"));
        auto* bar = root->findChild<QObject*>(QStringLiteral("documentTabBar"));
        QVERIFY(project && assets && bar);
        QVERIFY(project->createProject(QStringLiteral("Imported"), fixture.path()));
        QTRY_VERIFY(!project->projectOperationPending());
        const QString projectPath = project->currentPath();
        QVERIFY(project->inspectStl(stlPath));
        QTRY_VERIFY(project->importPreviewReady());
        QVERIFY(project->importStl(stlPath, project->defaultMeshType(),
                                   QStringLiteral("millimeters"), false));
        QTRY_COMPARE(assets->property("count").toInt(), 1);
        const QString importId = project->importedPartIds().constFirst();
        QTRY_COMPARE(project->activeDocumentId(), importId);
        QTRY_COMPARE(bar->property("activeDocumentId").toString(), importId);
        QVERIFY(QMetaObject::invokeMethod(bar, "closeDocument", Q_ARG(QString, importId)));
        QTRY_VERIFY(project->activeDocumentId() != importId);
        QTRY_VERIFY(!visual_items(assets, QStringLiteral("importedPartEntry")).isEmpty());
        auto* entry = visual_items(assets, QStringLiteral("importedPartEntry")).constFirst();
        QVERIFY(QMetaObject::invokeMethod(entry, "clicked"));
        QTRY_COMPARE(project->activeDocumentId(), importId);
        QVERIFY(project->saveProject());
        QTRY_VERIFY(!project->projectOperationPending());
        QVERIFY(project->createProject(QStringLiteral("Empty"), fixture.path()));
        QTRY_VERIFY(!project->projectOperationPending());
        QTRY_COMPARE(assets->property("count").toInt(), 0);
        QVERIFY(project->openProject(projectPath));
        QTRY_VERIFY(!project->projectOperationPending());
        QTRY_COMPARE(assets->property("count").toInt(), 1);
        QCOMPARE(project->importedPartIds().constFirst(), importId);
        QVERIFY(QFile::remove(stlPath));
        project->openImportRecord(importId);
        QTRY_COMPARE(project->activeDocumentId(), importId);
        QTRY_VERIFY(project->errorCode().isEmpty());
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
        for (const char* name : {"volumePanel", "fieldLegend", "fieldRangeModeRow",
                                 "sectionNormalZ", "openAnimationButton"}) {
            auto* item = visual_item(sidebar, QString::fromLatin1(name));
            QVERIFY2(item && item->isVisible(), name);
        }
        // task 036：诊断结果进入底部面板「诊断」页，并自动展开一次。
        auto* bottom = window->findChild<QQuickItem*>(QStringLiteral("bottomPanel"));
        QVERIFY(bottom && !bottom->property("collapsed").toBool());
        QCOMPARE(bottom->property("currentIndex").toInt(), 0);
        QVERIFY(bottom->isAncestorOf(quality));
        QVERIFY(!sidebar->isAncestorOf(quality));
        for (const QSize size : {QSize(1280, 720), QSize(1600, 900)}) {
            window->resize(size);
            QTRY_COMPARE(window->width(), size.width());
            QTRY_COMPARE(sidebar->width(), 240.0);
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
            auto* last = visual_item(sidebar, QStringLiteral("volumePanel"));
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

#endif
};

QTEST_MAIN(ShellModuleLoadTest)
#include "shell_module_load_test.moc"
