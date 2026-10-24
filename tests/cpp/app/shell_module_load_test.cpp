// 最小工作台的工程、导入、资产列表与文档接线回归。
#include "../../support/qml/quick_item_helpers.hpp"
#include <QFile>
#include <QFileInfo>
#include <QMetaEnum>
#include <QMetaProperty>
#include <QPointF>
#include <QQmlApplicationEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QSize>
#include <QTemporaryDir>
#include <QUrl>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <QtTest/qtest.h>
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
Q_IMPORT_QML_PLUGIN(Panta_BridgePlugin)
Q_IMPORT_QML_PLUGIN(Panta_VisualizationPlugin)
#endif
Q_IMPORT_QML_PLUGIN(Panta_ShellPlugin)

class ShellModuleLoadTest final : public QObject {
    Q_OBJECT
  private slots:
    void initTestCase() { QQuickStyle::setStyle(QStringLiteral("Basic")); }
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
        QVERIFY(root->findChild<QObject*>(QStringLiteral("shellCaption")));
#ifdef PANTA_ENABLE_BRIDGE_MODULE
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
        QCOMPARE(
            root->findChild<QObject*>(QStringLiteral("shellCaption"))->property("text").toString(),
            QStringLiteral("Renamed"));
        QVERIFY(QMetaObject::invokeMethod(import, "clicked"));
        QVERIFY(importDialog->property("visible").toBool());
        QVERIFY(QMetaObject::invokeMethod(importDialog, "close"));
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

#endif
};

QTEST_MAIN(ShellModuleLoadTest)
#include "shell_module_load_test.moc"
