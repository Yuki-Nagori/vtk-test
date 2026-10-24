/// ProjectViewModel 回归：工程命令、错误映射及任务 080 的视口文档生命周期。

#include "panta/visualization/mesh_source.hpp"
#include "project_operation_helpers.hpp"
#include "project_view_model.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QIODevice>
#include <QPair>
#include <QSettings>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QString>
#include <QTemporaryDir>
#include <QTimer>
#include <QUrl>
#include <QVariantMap>
#include <QtCore/qcontainerfwd.h>
#include <QtTest/qtest.h>
#include <array>
#include <cmath>
#include <functional>
#include <gtest/gtest.h>
#include <memory>
#include <panta/qt_adapter/standard_paths.hpp>
#include <qtestcase.h>
#include <system_error>
#include <thread>
#include <utility>
#include <vector>

#ifndef PANTA_STRINGIFY
#define PANTA_STRINGIFY_(value) (#value)
#define PANTA_STRINGIFY(value) (PANTA_STRINGIFY_(value))
#endif

using panta::bridge::ProjectViewModel;
using panta::test::finish_project_operation;
using panta::test::wait_for_active_mesh;

namespace {

class BackgroundWaiterFailureViewModel : public ProjectViewModel {
  public:
    bool failNextStart = false;

  protected:
    void startBackgroundWaiter(std::function<void()> work) override {
        if (std::exchange(failNextStart, false)) {
            throw std::system_error(std::make_error_code(std::errc::resource_unavailable_try_again),
                                    "injected waiter startup failure");
        }
        ProjectViewModel::startBackgroundWaiter(std::move(work));
    }
};

class HeldBackgroundViewModel : public ProjectViewModel {
  public:
    bool holdWork = false;
    std::vector<std::function<void()>> held;

    void releaseFirst() {
        auto work = std::move(held.front());
        held.erase(held.begin());
        std::thread(std::move(work)).join();
    }

  protected:
    void startBackgroundWaiter(std::function<void()> work) override {
        if (holdWork) {
            held.push_back(std::move(work));
        } else {
            ProjectViewModel::startBackgroundWaiter(std::move(work));
        }
    }
};

} // namespace

TEST(ProjectViewModelTest, CreatesOpensRenamesAndSavesThroughRustService) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());

    ProjectViewModel view_model;
    QSignalSpy created(&view_model, &ProjectViewModel::projectCreated);
    const QString location = QDir(fixture.path()).absolutePath();
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), location)));
    ASSERT_EQ(created.count(), 1);
    EXPECT_EQ(view_model.currentName(), QStringLiteral("Demo"));
    EXPECT_FALSE(view_model.dirty());

    const QString project_path = view_model.currentPath();
    EXPECT_TRUE(
        QDir::fromNativeSeparators(project_path).endsWith(QStringLiteral("/Demo/Demo.panta")));
    EXPECT_TRUE(QFile::exists(project_path));
    EXPECT_TRUE(view_model.renameProject(QStringLiteral("Renamed")));
    EXPECT_TRUE(view_model.dirty());
    EXPECT_TRUE(finish_project_operation(view_model, view_model.saveProject()));
    EXPECT_FALSE(view_model.dirty());

    ProjectViewModel reopened;
    QSignalSpy opened(&reopened, &ProjectViewModel::projectOpened);
    ASSERT_TRUE(finish_project_operation(
        reopened, reopened.openProjectUrl(QUrl::fromLocalFile(project_path))));
    ASSERT_EQ(opened.count(), 1);
    EXPECT_EQ(reopened.currentName(), QStringLiteral("Renamed"));
    EXPECT_FALSE(reopened.dirty());
}

TEST(ProjectViewModelTest, ProjectCommandsReturnAdmissionAndPublishWithoutBlockingGuiEvents) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    HeldBackgroundViewModel model;
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("First"), fixture.path())));
    ASSERT_TRUE(model.renameProject(QStringLiteral("Renamed")));
    model.holdWork = true;
    QSignalSpy finished(&model, &ProjectViewModel::projectOperationFinished);
    ASSERT_TRUE(model.saveProject());
    EXPECT_TRUE(model.projectOperationPending());
    EXPECT_TRUE(model.dirty());
    EXPECT_TRUE(finished.isEmpty());
    ASSERT_EQ(model.held.size(), 1U);
    EXPECT_FALSE(model.createProject(QStringLiteral("Conflicting"), fixture.path()));
    EXPECT_FALSE(model.renameProject(QStringLiteral("Conflicting")));
    EXPECT_FALSE(QDir(fixture.filePath(QStringLiteral("Conflicting"))).exists());
    bool guiEventDelivered = false;
    QTimer::singleShot(0, &model, [&guiEventDelivered] { guiEventDelivered = true; });
    QTRY_VERIFY(guiEventDelivered);
    EXPECT_TRUE(model.projectOperationPending());
    model.releaseFirst();
    QTRY_COMPARE(finished.count(), 1);
    EXPECT_EQ(finished.at(0).at(0).toString(), QStringLiteral("save"));
    EXPECT_TRUE(finished.at(0).at(1).toBool());
    EXPECT_FALSE(model.projectOperationPending());
    EXPECT_FALSE(model.dirty());
    EXPECT_TRUE(model.errorCode().isEmpty());
}

TEST(ProjectViewModelTest, ProjectWaiterStartupFailureNeverAdmitsFilesystemWork) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    BackgroundWaiterFailureViewModel model;
    model.failNextStart = true;
    EXPECT_FALSE(model.createProject(QStringLiteral("NotCreated"), fixture.path()));
    EXPECT_FALSE(model.projectOperationPending());
    EXPECT_EQ(model.errorCode(), QStringLiteral("project.io"));
    EXPECT_FALSE(QDir(fixture.filePath(QStringLiteral("NotCreated"))).exists());
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("Existing"), fixture.path())));
    QFile manifest(model.currentPath());
    ASSERT_TRUE(manifest.open(QIODevice::ReadOnly));
    const auto original = manifest.readAll();
    manifest.close();
    ASSERT_TRUE(model.renameProject(QStringLiteral("Dirty")));
    model.failNextStart = true;
    EXPECT_FALSE(model.saveProject());
    EXPECT_FALSE(model.projectOperationPending());
    EXPECT_TRUE(model.dirty());
    ASSERT_TRUE(manifest.open(QIODevice::ReadOnly));
    EXPECT_EQ(manifest.readAll(), original);
    manifest.close();
    model.failNextStart = true;
    EXPECT_FALSE(model.openProject(model.currentPath()));
    EXPECT_EQ(model.currentName(), QStringLiteral("Dirty"));
    ASSERT_TRUE(finish_project_operation(model, model.saveProject()));
}

TEST(ProjectViewModelTest, CompletionDoesNotEraseAnErrorFromTheNextReentrantCommand) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    BackgroundWaiterFailureViewModel model;
    QSignalSpy finished(&model, &ProjectViewModel::projectOperationFinished);
    QSignalSpy created(&model, &ProjectViewModel::projectCreated);
    bool attemptedSave = false;
    bool acceptedSave = true;
    QObject::connect(&model, &ProjectViewModel::projectOperationPendingChanged, &model, [&] {
        if (model.projectOperationPending() || attemptedSave) {
            return;
        }
        attemptedSave = true;
        model.failNextStart = true;
        acceptedSave = model.saveProject();
    });
    ASSERT_TRUE(model.createProject(QStringLiteral("Completed"), fixture.path()));
    EXPECT_TRUE(model.projectOperationPending());
    QTRY_COMPARE(finished.count(), 1);
    EXPECT_EQ(finished.at(0).at(0).toString(), QStringLiteral("create"));
    EXPECT_TRUE(finished.at(0).at(1).toBool());
    EXPECT_TRUE(attemptedSave);
    EXPECT_FALSE(acceptedSave);
    EXPECT_FALSE(model.projectOperationPending());
    EXPECT_EQ(model.errorCode(), QStringLiteral("project.io"));
    EXPECT_TRUE(model.errorDetail().contains(QStringLiteral("injected waiter startup failure")));
    EXPECT_EQ(created.count(), 1);
    EXPECT_EQ(model.currentName(), QStringLiteral("Completed"));
    EXPECT_TRUE(QFile::exists(fixture.filePath(QStringLiteral("Completed/Completed.panta"))));
}

TEST(ProjectViewModelTest, DestroyingProjectRequestOwnerDoesNotAccessDeletedQtObjects) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    auto model = std::make_unique<HeldBackgroundViewModel>();
    model->holdWork = true;
    ASSERT_TRUE(model->createProject(QStringLiteral("Accepted"), fixture.path()));
    ASSERT_EQ(model->held.size(), 1U);
    auto work = std::move(model->held.front());
    model.reset();
    std::thread(std::move(work)).join();
    QCoreApplication::processEvents();
    EXPECT_TRUE(QFile::exists(fixture.filePath(QStringLiteral("Accepted/Accepted.panta"))));
}

TEST(ProjectViewModelTest, DisplayPreparationCoalescesSelectionAndRejectsTheOldDisplayResult) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const auto sourcePath = fixture.filePath(QStringLiteral("part.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n"), 0);
    source.close();
    HeldBackgroundViewModel model;
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("Display"), fixture.path())));
    model.holdWork = true;
    ASSERT_TRUE(model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                QStringLiteral("millimeters"), false));
    ASSERT_EQ(model.held.size(), 1U);
    EXPECT_EQ(model.mesh_snapshot(), nullptr);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Truncate));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 2 0 0\nvertex 0 1 0\n"), 0);
    source.close();
    ASSERT_TRUE(model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                QStringLiteral("millimeters"), false));
    EXPECT_EQ(model.activeDocumentId(), QStringLiteral("import-2"));
    ASSERT_EQ(model.held.size(), 1U);
    model.releaseFirst();
    QTRY_COMPARE(model.held.size(), 1U);
    EXPECT_EQ(model.mesh_snapshot(), nullptr);
    model.releaseFirst();
    ASSERT_TRUE(wait_for_active_mesh(model, QStringLiteral("import-2")));
    EXPECT_EQ(model.mesh_snapshot()->vertices[1], (std::array<double, 3>{2.0, 0.0, 0.0}));
}

TEST(ProjectViewModelTest, ProjectSwitchRejectsOldDisplayWithTheSameImportIdAndRevision) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const auto sourcePath = fixture.filePath(QStringLiteral("part.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n"), 0);
    source.close();
    HeldBackgroundViewModel model;
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("First"), fixture.path())));
    model.holdWork = true;
    ASSERT_TRUE(model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                QStringLiteral("millimeters"), false));
    ASSERT_EQ(model.held.size(), 1U);

    model.holdWork = false;
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("Second"), fixture.path())));
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Truncate));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 3 0 0\nvertex 0 1 0\n"), 0);
    source.close();
    model.holdWork = true;
    ASSERT_TRUE(model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                QStringLiteral("millimeters"), false));
    EXPECT_EQ(model.activeDocumentId(), QStringLiteral("import-1"));
    ASSERT_EQ(model.held.size(), 1U);
    model.releaseFirst();
    QTRY_COMPARE(model.held.size(), 1U);
    EXPECT_EQ(model.mesh_snapshot(), nullptr);
    model.releaseFirst();
    ASSERT_TRUE(wait_for_active_mesh(model, QStringLiteral("import-1")));
    EXPECT_EQ(model.mesh_snapshot()->vertices[1], (std::array<double, 3>{3.0, 0.0, 0.0}));
}

TEST(ProjectViewModelTest, DestroyingDisplayPreparationOwnerDoesNotAccessDeletedQtObjects) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const auto sourcePath = fixture.filePath(QStringLiteral("part.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n"), 0);
    source.close();
    auto model = std::make_unique<HeldBackgroundViewModel>();
    ASSERT_TRUE(finish_project_operation(
        *model, model->createProject(QStringLiteral("Display"), fixture.path())));
    model->holdWork = true;
    ASSERT_TRUE(model->importStl(sourcePath, QStringLiteral("solid-3d"),
                                 QStringLiteral("millimeters"), false));
    ASSERT_EQ(model->held.size(), 1U);
    auto work = std::move(model->held.front());
    model.reset();
    std::thread(std::move(work)).join();
    QCoreApplication::processEvents();
}

TEST(ProjectViewModelTest, CreatesAndOpensProjectThroughUnicodeFilesystemPaths) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());

    const QString location = QDir(fixture.path()).filePath(QStringLiteral("项目资料"));
    ProjectViewModel created;
    ASSERT_TRUE(finish_project_operation(
        created, created.createProject(QStringLiteral("工程-测试"), location)))
        << created.errorCode().toStdString() << ": " << created.errorDetail().toStdString();

    const QString projectPath = created.currentPath();
    ASSERT_TRUE(QFile::exists(projectPath));
    EXPECT_EQ(QDir::fromNativeSeparators(projectPath),
              QDir::fromNativeSeparators(
                  QDir(location).filePath(QStringLiteral("工程-测试/工程-测试.panta"))));

    ProjectViewModel reopened;
    const QUrl projectUrl = QUrl::fromLocalFile(projectPath);
    ASSERT_TRUE(finish_project_operation(reopened, reopened.openProjectUrl(projectUrl)))
        << reopened.errorCode().toStdString() << ": " << reopened.errorDetail().toStdString();
    EXPECT_EQ(reopened.currentName(), QStringLiteral("工程-测试"));
    EXPECT_EQ(QFileInfo(reopened.currentPath()).canonicalFilePath(),
              QFileInfo(projectPath).canonicalFilePath());
}

TEST(ProjectViewModelTest, MapsRustErrorsWithoutCreatingInvalidTargets) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    ProjectViewModel view_model;
    const QString location = QDir(fixture.path()).absolutePath();

    EXPECT_FALSE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("../escape"), location)));
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.invalid_name"));
    EXPECT_EQ(view_model.errorCategory(), QStringLiteral("validation"));
    EXPECT_EQ(view_model.errorDetail(), QStringLiteral("../escape"));
    EXPECT_FALSE(view_model.error().isEmpty());
    EXPECT_FALSE(QDir(fixture.path()).exists(QStringLiteral("escape")));

    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), location)));
    EXPECT_TRUE(view_model.errorCategory().isEmpty());
    EXPECT_TRUE(view_model.errorDetail().isEmpty());
    EXPECT_FALSE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), location)));
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.already_exists"));
    EXPECT_EQ(view_model.errorCategory(), QStringLiteral("conflict"));
    EXPECT_FALSE(view_model.errorDetail().isEmpty());

    ProjectViewModel empty;
    EXPECT_FALSE(finish_project_operation(empty, empty.saveProject()));
    EXPECT_EQ(empty.errorCode(), QStringLiteral("project.no_project"));
    EXPECT_EQ(empty.errorCategory(), QStringLiteral("state"));
    EXPECT_TRUE(empty.errorDetail().isEmpty());
}

TEST(ProjectViewModelTest, StructuredImportFailuresPreserveTheCommittedProjectAndViewport) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const QString sourcePath = fixture.filePath(QStringLiteral("零件 sample.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n"), 0);
    source.close();

    ProjectViewModel model;
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("Demo"), fixture.path())));
    ASSERT_TRUE(model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(model, model.activeDocumentId()));
    const auto mesh = model.mesh_snapshot();
    ASSERT_NE(mesh, nullptr);
    const auto documents = model.openDocuments();
    const auto parts = model.importedPartNames();
    const auto active = model.activeDocumentId();
    QFile manifest(model.currentPath());
    ASSERT_TRUE(manifest.open(QIODevice::ReadOnly));
    const auto before = manifest.readAll();
    manifest.close();
    QSignalSpy imported(&model, &ProjectViewModel::projectImported);

    // 参数本身包含冒号及中文，不能通过拼接文案还原诊断字段。
    EXPECT_FALSE(model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                 QStringLiteral("unknown:单位"), false));
    EXPECT_EQ(model.errorCode(), QStringLiteral("project.import_unsupported_units"));
    EXPECT_EQ(model.errorCategory(), QStringLiteral("format"));
    EXPECT_EQ(model.errorDetail(), QStringLiteral("unknown:单位"));
    EXPECT_EQ(model.error(), QStringLiteral("请选择有效的 STL 导入选项。"));
    EXPECT_EQ(imported.count(), 0);
    EXPECT_EQ(model.mesh_snapshot(), mesh);
    EXPECT_EQ(model.openDocuments(), documents);
    EXPECT_EQ(model.importedPartNames(), parts);
    EXPECT_EQ(model.activeDocumentId(), active);
    ASSERT_TRUE(manifest.open(QIODevice::ReadOnly));
    EXPECT_EQ(manifest.readAll(), before);
    manifest.close();

    const QString missing = fixture.filePath(QStringLiteral("不存在 sample.stl"));
    ASSERT_TRUE(model.inspectStl(missing));
    QTRY_VERIFY(!model.importPreviewPending());
    EXPECT_EQ(model.errorCode(), QStringLiteral("project.import_file_missing"));
    EXPECT_EQ(model.errorCategory(), QStringLiteral("missing"));
    EXPECT_EQ(model.errorDetail(), missing);
    EXPECT_FALSE(model.importPreviewReady());
    EXPECT_EQ(model.mesh_snapshot(), mesh);

    EXPECT_FALSE(finish_project_operation(
        model, model.openProject(fixture.filePath(QStringLiteral("missing.panta")))));
    EXPECT_EQ(model.errorCode(), QStringLiteral("project.file_missing"));
    EXPECT_EQ(model.errorCategory(), QStringLiteral("missing"));
    EXPECT_EQ(model.mesh_snapshot(), mesh);
    EXPECT_EQ(model.openDocuments(), documents);

    // 临时把资产目录替换为普通文件，稳定触发 sidecar 目录创建失败。
    const QString projectRoot = QFileInfo(model.currentPath()).absolutePath();
    const QString assetsDirectory = projectRoot + QStringLiteral("/assets");
    const QString parkedAssetsDirectory = projectRoot + QStringLiteral("/assets-parked");
    ASSERT_TRUE(QDir().rename(assetsDirectory, parkedAssetsDirectory));
    QFile assetsBlocker(assetsDirectory);
    ASSERT_TRUE(assetsBlocker.open(QIODevice::WriteOnly));
    assetsBlocker.write("not a directory");
    assetsBlocker.close();
    EXPECT_FALSE(model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                 QStringLiteral("millimeters"), false));
    EXPECT_EQ(model.errorCode(), QStringLiteral("project.import_asset_write_failed"));
    EXPECT_EQ(model.errorCategory(), QStringLiteral("io"));
    EXPECT_EQ(imported.count(), 0);
    EXPECT_EQ(model.mesh_snapshot(), mesh);
    EXPECT_EQ(model.importedPartNames(), parts);
    ASSERT_TRUE(manifest.open(QIODevice::ReadOnly));
    EXPECT_EQ(manifest.readAll(), before);
    manifest.close();
    ASSERT_TRUE(QFile::remove(assetsDirectory));
    ASSERT_TRUE(QDir().rename(parkedAssetsDirectory, assetsDirectory));

    ASSERT_TRUE(model.inspectStl(sourcePath));
    EXPECT_TRUE(model.importPreviewPending());
    QTRY_VERIFY(!model.importPreviewPending());
    EXPECT_TRUE(model.error().isEmpty());
    EXPECT_TRUE(model.errorCode().isEmpty());
    EXPECT_TRUE(model.errorCategory().isEmpty());
    EXPECT_TRUE(model.errorDetail().isEmpty());
    EXPECT_TRUE(model.importPreviewReady());
}

TEST(ProjectViewModelTest, PreviewsImportsAndPersistsLatestRecord) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());

    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("sample.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid sample\n"
                           "facet normal 0 0 1\n"
                           " outer loop\n"
                           "  vertex 0 0 0\n"
                           "  vertex 1 0 0\n"
                           "  vertex 0 1 0\n"
                           " endloop\n"
                           "endfacet\n"
                           "endsolid sample\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), fixture.path())));
    EXPECT_TRUE(view_model.importedPartNames().isEmpty());
    ASSERT_TRUE(view_model.inspectStl(sourcePath));
    EXPECT_TRUE(view_model.importPreviewPending());
    QTRY_VERIFY(!view_model.importPreviewPending());
    EXPECT_TRUE(view_model.importPreviewReady());
    EXPECT_EQ(view_model.importPreviewName(), QStringLiteral("sample.stl"));
    EXPECT_EQ(view_model.importPreviewDimensions(), QStringLiteral("1.00 × 1.00 × 0.00"));
    EXPECT_EQ(view_model.importPreviewTriangleCount(), 1U);

    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("dual-domain"),
                                     QStringLiteral("millimeters"), true));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    EXPECT_EQ(view_model.importedPartNames(), QStringList{QStringLiteral("sample")});
    EXPECT_EQ(view_model.importedPartName(), QStringLiteral("sample"));
    EXPECT_EQ(view_model.importedMeshType(), QStringLiteral("dual-domain"));
    EXPECT_EQ(view_model.importedUnits(), QStringLiteral("millimeters"));
    EXPECT_EQ(view_model.importedDimensions(), QStringLiteral("1.00 × 1.00 × 0.00 mm"));
    EXPECT_EQ(view_model.importedTriangleCount(), 1U);
    EXPECT_TRUE(QFile::exists(view_model.importedAssetPath()));
    const auto mesh = view_model.mesh_snapshot();
    ASSERT_NE(mesh, nullptr);
    EXPECT_EQ(mesh->vertices.size(), 3U);
    EXPECT_EQ(mesh->vertices[1], (std::array<double, 3>{1.0, 0.0, 0.0}));

    const auto firstId = view_model.importedPartIds().constFirst();

    const QString secondSourcePath =
        QDir(fixture.path()).filePath(QStringLiteral("sample-second.stl"));
    ASSERT_TRUE(QFile::copy(sourcePath, secondSourcePath));
    ASSERT_TRUE(view_model.importStl(secondSourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    EXPECT_EQ(view_model.importedPartNames(),
              QStringList({QStringLiteral("sample"), QStringLiteral("sample-second")}));
    EXPECT_EQ(view_model.importedPartName(), QStringLiteral("sample-second"));

    view_model.activateDocument(firstId);
    ASSERT_TRUE(wait_for_active_mesh(view_model, firstId));
    view_model.closeDocument(QStringLiteral("import-1"));
    view_model.closeDocument(QStringLiteral("import-2"));

    ProjectViewModel reopened;
    ASSERT_TRUE(finish_project_operation(reopened, reopened.openProject(view_model.currentPath())));
    EXPECT_EQ(reopened.importedPartNames(),
              QStringList({QStringLiteral("sample"), QStringLiteral("sample-second")}));
    EXPECT_EQ(reopened.importedPartName(), QStringLiteral("sample-second"));
    EXPECT_EQ(reopened.importedAssetPath(), view_model.importedAssetPath());
    EXPECT_EQ(reopened.importedDimensions(), QStringLiteral("1.00 × 1.00 × 0.00 mm"));
    // 打开工程只读清单（073/080）：已保存网格须经只读 FSM 激活异步恢复，
    // 重开后视口快照为空；记录投影与修订不变。
    const auto reopened_mesh = reopened.mesh_snapshot();
    EXPECT_EQ(reopened_mesh, nullptr);

    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Truncate | QIODevice::Text));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 2 0 0\nvertex 0 1 0\n"), 0);
    source.close();
    QSignalSpy mesh_changed(&view_model, &panta::visualization::MeshSource::meshChanged);
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("dual-domain"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    EXPECT_EQ(view_model.importedPartNames(),
              QStringList({QStringLiteral("sample"), QStringLiteral("sample-second"),
                           QStringLiteral("sample")}));
    EXPECT_EQ(view_model.importedPartName(), QStringLiteral("sample"));
    EXPECT_EQ(mesh_changed.count(), 2);
    const auto replacement = view_model.mesh_snapshot();
    ASSERT_NE(replacement, nullptr);
    EXPECT_EQ(replacement->vertices[1], (std::array<double, 3>{2.0, 0.0, 0.0}));
    EXPECT_EQ(mesh->vertices[1], (std::array<double, 3>{1.0, 0.0, 0.0}));
    ASSERT_TRUE(view_model.inspectStl(sourcePath));
    EXPECT_TRUE(view_model.importPreviewPending());
    QTRY_VERIFY(!view_model.importPreviewPending());
    ASSERT_TRUE(view_model.inspectStl(sourcePath + QStringLiteral(".missing")));
    QTRY_VERIFY(!view_model.importPreviewPending());
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.import_file_missing"));
    EXPECT_FALSE(view_model.importPreviewReady());
    EXPECT_EQ(view_model.importedPartNames().size(), 3);
}

TEST(ProjectViewModelTest, PreviewCancellationReplacementAndProjectSwitchIgnoreOldResults) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const auto a = fixture.filePath(QStringLiteral("a.stl"));
    const auto b = fixture.filePath(QStringLiteral("b.stl"));
    for (const auto& path : {a, b}) {
        QFile source(path);
        ASSERT_TRUE(source.open(QIODevice::WriteOnly));
        ASSERT_GT(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n"), 0);
    }
    ProjectViewModel model;
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("Demo"), fixture.path())));
    QSignalSpy finished(&model, &ProjectViewModel::importPreviewFinished);
    ASSERT_TRUE(model.inspectStl(a));
    EXPECT_TRUE(model.importPreviewPending());
    EXPECT_FALSE(model.importPreviewReady());
    model.cancelImportPreview();
    EXPECT_FALSE(model.importPreviewPending());
    EXPECT_FALSE(model.importPreviewReady());
    ASSERT_TRUE(model.inspectStl(a));
    ASSERT_TRUE(model.inspectStl(b));
    QTRY_VERIFY(!model.importPreviewPending());
    ASSERT_TRUE(model.importPreviewReady());
    EXPECT_EQ(model.importPreviewName(), QStringLiteral("b.stl"));
    ASSERT_EQ(finished.count(), 1);
    EXPECT_TRUE(finished.at(0).at(1).toBool());
    EXPECT_TRUE(model.importedPartNames().isEmpty());
    ASSERT_TRUE(model.inspectStl(a));
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("Other"), fixture.path())));
    EXPECT_FALSE(model.importPreviewPending());
    EXPECT_FALSE(model.importPreviewReady());
    finished.clear();
    ASSERT_TRUE(model.inspectStl(b));
    QTRY_VERIFY(!model.importPreviewPending());
    EXPECT_EQ(model.importPreviewName(), QStringLiteral("b.stl"));
    EXPECT_EQ(finished.count(), 1);
    EXPECT_EQ(model.currentName(), QStringLiteral("Other"));
}

TEST(ProjectViewModelTest, ConfirmedImportCompletesThroughBackgroundViewModelRequest) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const auto path = fixture.filePath(QStringLiteral("part.stl"));
    QFile source(path);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n"), 0);
    source.close();

    ProjectViewModel model;
    ASSERT_TRUE(finish_project_operation(
        model, model.createProject(QStringLiteral("Demo"), fixture.path())));
    ASSERT_TRUE(model.inspectStl(path));
    QTRY_VERIFY(!model.importPreviewPending());
    ASSERT_TRUE(model.importPreviewReady());
    QSignalSpy finished(&model, &ProjectViewModel::projectImportFinished);

    ASSERT_TRUE(model.beginImportStl(path, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    EXPECT_TRUE(model.importPending() || finished.count() == 1);
    QTRY_COMPARE(finished.count(), 1);
    EXPECT_TRUE(finished.at(0).at(1).toBool());
    EXPECT_FALSE(model.importPending());
    EXPECT_EQ(model.importedPartNames(), QStringList{QStringLiteral("part")});
    EXPECT_EQ(model.openDocuments().size(), 1);
    EXPECT_EQ(model.activeDocumentId(), QStringLiteral("import-1"));
    ASSERT_TRUE(wait_for_active_mesh(model, QStringLiteral("import-1")));
    EXPECT_NE(model.mesh_snapshot(), nullptr);
    EXPECT_EQ(model.errorCode(), QString{});

    QFile manifest(model.currentPath());
    ASSERT_TRUE(manifest.open(QIODevice::ReadOnly));
    const auto data = manifest.readAll();
    EXPECT_TRUE(data.contains("\"id\": \"import-1\""));
    EXPECT_TRUE(
        QFile::exists(QDir(fixture.filePath(QStringLiteral("Demo")))
                          .filePath(QStringLiteral("assets/meshes/mesh-import-1-r1.pmesh"))));
}

TEST(ProjectViewModelTest, PreviewNotificationsCanReplaceOrCancelTheCompletedRequest) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const auto path = fixture.filePath(QStringLiteral("preview.stl"));
    QFile source(path);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n"), 0);
    source.close();
    ProjectViewModel model;
    QSignalSpy finished(&model, &ProjectViewModel::importPreviewFinished);
    bool replaced = false;
    const auto replacement =
        QObject::connect(&model, &ProjectViewModel::importPreviewChanged, &model, [&] {
            if (!replaced && model.importPreviewReady()) {
                replaced = true;
                EXPECT_TRUE(model.inspectStl(path));
            }
        });
    ASSERT_TRUE(model.inspectStl(path));
    QTRY_VERIFY(!model.importPreviewPending());
    EXPECT_TRUE(replaced);
    EXPECT_TRUE(model.importPreviewReady());
    EXPECT_EQ(finished.count(), 1);
    QObject::disconnect(replacement);
    const auto cancellation =
        QObject::connect(&model, &ProjectViewModel::errorChanged, &model, [&] {
            if (!model.errorCode().isEmpty())
                model.cancelImportPreview();
        });
    ASSERT_TRUE(model.inspectStl(path + QStringLiteral(".missing")));
    QTRY_VERIFY(!model.errorCode().isEmpty());
    EXPECT_FALSE(model.importPreviewPending());
    EXPECT_FALSE(model.importPreviewReady());
    EXPECT_EQ(finished.count(), 1);
    QObject::disconnect(cancellation);
}

int main(int argc, char** argv) {
    QCoreApplication app(argc, argv);
    // 最近工程写入用户配置目录；测试模式把它重定向到隔离目录。
    QStandardPaths::setTestModeEnabled(true);
    ::testing::InitGoogleTest(&argc, argv);
    return RUN_ALL_TESTS();
}

TEST(ProjectViewModelTest, RemembersRecentProjectsAndClassifiesDrops) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    ProjectViewModel model;
    model.clearRecentProjects();
    EXPECT_TRUE(model.recentProjects().isEmpty());
    QSignalSpy recent(&model, &ProjectViewModel::recentProjectsChanged);

    const QString location = QDir(fixture.path()).absolutePath();
    ASSERT_TRUE(
        finish_project_operation(model, model.createProject(QStringLiteral("A"), location)));
    const QString first = model.currentPath();
    ASSERT_TRUE(
        finish_project_operation(model, model.createProject(QStringLiteral("B"), location)));
    const QString second = model.currentPath();
    EXPECT_GE(recent.count(), 2);
    ASSERT_EQ(model.recentProjects().size(), 2);
    EXPECT_EQ(model.recentProjects().front(), QFileInfo(second).absoluteFilePath());

    ProjectViewModel other;
    ASSERT_TRUE(finish_project_operation(other, other.openProject(first)));
    EXPECT_EQ(model.recentProjects().front(), QFileInfo(first).absoluteFilePath());
    EXPECT_EQ(model.recentProjects().size(), 2);

    // 已删除的工程不再出现在列表中。
    ASSERT_TRUE(QFile::remove(second));
    EXPECT_EQ(model.recentProjects(), QStringList{QFileInfo(first).absoluteFilePath()});

    EXPECT_EQ(model.classifyDroppedUrl(QUrl::fromLocalFile(QStringLiteral("/x/a.PANTA"))),
              QStringLiteral("project"));
    EXPECT_EQ(model.classifyDroppedUrl(QUrl::fromLocalFile(QStringLiteral("/x/a.stl"))),
              QStringLiteral("mesh"));
    EXPECT_EQ(model.classifyDroppedUrl(QUrl::fromLocalFile(QStringLiteral("/x/a.Ply"))),
              QStringLiteral("mesh"));
    // CFD 来源（task 032）：OpenFOAM 标记文件 / system/controlDict 与 CGNS。
    EXPECT_EQ(model.classifyDroppedUrl(QUrl::fromLocalFile(QStringLiteral("/x/case.foam"))),
              QStringLiteral("mesh"));
    EXPECT_EQ(
        model.classifyDroppedUrl(QUrl::fromLocalFile(QStringLiteral("/x/case/system/controlDict"))),
        QStringLiteral("mesh"));
    EXPECT_EQ(model.classifyDroppedUrl(QUrl::fromLocalFile(QStringLiteral("/x/flow.CGNS"))),
              QStringLiteral("mesh"));
    EXPECT_TRUE(
        model.classifyDroppedUrl(QUrl::fromLocalFile(QStringLiteral("/x/a.txt"))).isEmpty());
    EXPECT_TRUE(model.classifyDroppedUrl(QUrl(QStringLiteral("https://e.com/a.stl"))).isEmpty());
    model.clearRecentProjects();
    EXPECT_TRUE(model.recentProjects().isEmpty());
}

TEST(ProjectViewModelTest, LargeMeshLimitsPersistAndFallBackToDefaults) {
    const QString directory =
        panta::qt_adapter::standard_location(panta::qt_adapter::StandardLocation::UserConfig);
    ASSERT_FALSE(directory.isEmpty());
    const QString file = QDir(directory).filePath(QStringLiteral("large-mesh.ini"));
    QFile::remove(file);

    ProjectViewModel model;
    const double defaultTriangles = model.lodActivationTriangles();
    const double defaultBudget = model.lodBudgetTriangles();
    const double defaultCells = model.volumeStreamCells();
    EXPECT_EQ(defaultTriangles, 20'000'000.0);
    EXPECT_EQ(defaultBudget, 2'000'000.0);
    EXPECT_EQ(defaultCells, 10'000'000.0);
    EXPECT_EQ(model.volumeMode(), 0);
    EXPECT_EQ(model.chunkCacheMib(), 1024.0);

    QSignalSpy changed(&model, &ProjectViewModel::largeMeshLimitsChanged);
    ASSERT_TRUE(model.setLargeMeshLimits(30'000'000.0, 3'000'000.0, 50'000'000.0, 1, 4096.0));
    EXPECT_EQ(changed.count(), 1);
    // 预算不低于阈值、非整数与超范围都在 Rust/适配层被拒绝，原设置不变。
    EXPECT_FALSE(model.setLargeMeshLimits(30'000'000.0, 30'000'000.0, 50'000'000.0, 1, 4096.0));
    EXPECT_EQ(model.errorCode(), QStringLiteral("project.lod_settings_invalid"));
    EXPECT_FALSE(model.setLargeMeshLimits(30'000'000.5, 3'000'000.0, 50'000'000.0, 1, 4096.0));
    EXPECT_FALSE(model.setLargeMeshLimits(30'000'000.0, 3'000'000.0, 1.0, 1, 4096.0));
    EXPECT_EQ(model.lodActivationTriangles(), 30'000'000.0);
    EXPECT_EQ(model.volumeStreamCells(), 50'000'000.0);
    // 体网格模式与块缓存预算（task 028）：未知模式、负数与超范围预算被拒绝。
    EXPECT_FALSE(model.setLargeMeshLimits(30'000'000.0, 3'000'000.0, 50'000'000.0, 3, 4096.0));
    EXPECT_EQ(model.errorDetail(), QStringLiteral("volume_mode"));
    EXPECT_FALSE(model.setLargeMeshLimits(30'000'000.0, 3'000'000.0, 50'000'000.0, -1, 4096.0));
    EXPECT_FALSE(model.setLargeMeshLimits(30'000'000.0, 3'000'000.0, 50'000'000.0, 1, 32.0));
    EXPECT_EQ(model.errorDetail(), QStringLiteral("chunk_cache_mib"));
    EXPECT_EQ(model.volumeMode(), 1);
    EXPECT_EQ(model.chunkCacheMib(), 4096.0);

    {
        ProjectViewModel restored;
        EXPECT_EQ(restored.lodActivationTriangles(), 30'000'000.0);
        EXPECT_EQ(restored.lodBudgetTriangles(), 3'000'000.0);
        EXPECT_EQ(restored.volumeStreamCells(), 50'000'000.0);
        EXPECT_EQ(restored.volumeMode(), 1);
        EXPECT_EQ(restored.chunkCacheMib(), 4096.0);
    }
    {
        // task 026 写出的文件没有模式与块缓存键：其余设置生效，这两项取默认值。
        QSettings settings(file, QSettings::IniFormat);
        settings.remove(QStringLiteral("large_mesh/volume_mode"));
        settings.remove(QStringLiteral("large_mesh/chunk_cache_mib"));
        settings.sync();
    }
    {
        ProjectViewModel legacy;
        EXPECT_EQ(legacy.volumeStreamCells(), 50'000'000.0);
        EXPECT_EQ(legacy.volumeMode(), 0);
        EXPECT_EQ(legacy.chunkCacheMib(), 1024.0);
    }

    {
        // 手工改坏的持久化值不生效，回退默认值。
        QSettings settings(file, QSettings::IniFormat);
        settings.setValue(QStringLiteral("large_mesh/surface_budget"), QStringLiteral("abc"));
        settings.sync();
    }
    {
        ProjectViewModel broken;
        EXPECT_EQ(broken.lodActivationTriangles(), defaultTriangles);
        EXPECT_EQ(broken.lodBudgetTriangles(), defaultBudget);
    }
    {
        QSettings settings(file, QSettings::IniFormat);
        settings.setValue(QStringLiteral("large_mesh/surface_budget"), 40'000'000ULL);
        settings.sync();
    }
    {
        ProjectViewModel outOfRange;
        EXPECT_EQ(outOfRange.lodBudgetTriangles(), defaultBudget);
        EXPECT_EQ(outOfRange.volumeStreamCells(), defaultCells);
    }

    model.resetLargeMeshLimits();
    EXPECT_EQ(model.lodActivationTriangles(), defaultTriangles);
    ProjectViewModel afterReset;
    EXPECT_EQ(afterReset.volumeStreamCells(), defaultCells);
}

TEST(ProjectViewModelTest, DocumentTabsFollowImportAndClose) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());

    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("sample.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid sample\n"
                           "facet normal 0 0 1\n"
                           " outer loop\n"
                           "  vertex 0 0 0\n"
                           "  vertex 1 0 0\n"
                           "  vertex 0 1 0\n"
                           " endloop\n"
                           "endfacet\n"
                           "endsolid sample\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    QSignalSpy documentsChanged(&view_model, &ProjectViewModel::documentsChanged);
    QSignalSpy activeChanged(&view_model, &ProjectViewModel::activeDocumentChanged);
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), fixture.path())));

    // 新工程不创建默认文档，尚无可显示的网格。
    EXPECT_TRUE(view_model.openDocuments().isEmpty());
    EXPECT_TRUE(view_model.activeDocumentId().isEmpty());
    EXPECT_TRUE(view_model.activeDocumentTitle().isEmpty());
    EXPECT_EQ(view_model.mesh_snapshot(), nullptr);

    // 导入成功：自动新增并激活就绪导入页签，快照复用导入产物。
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("dual-domain"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    ASSERT_EQ(view_model.openDocuments().size(), 1);
    const auto imported = view_model.openDocuments()[0].toMap();
    EXPECT_EQ(imported[QStringLiteral("id")].toString(), QStringLiteral("import-1"));
    EXPECT_EQ(imported[QStringLiteral("state")].toString(), QStringLiteral("ready"));
    EXPECT_EQ(imported[QStringLiteral("title")].toString(), QStringLiteral("sample"));
    EXPECT_EQ(view_model.activeDocumentId(), QStringLiteral("import-1"));
    EXPECT_EQ(view_model.activeDocumentTitle(), QStringLiteral("sample"));
    EXPECT_EQ(activeChanged.count(), 2);
    EXPECT_NE(view_model.mesh_snapshot(), nullptr);

    EXPECT_EQ(documentsChanged.count(), 1);
    documentsChanged.clear();

    // 关闭最后一个活动页签后进入空白视口。
    view_model.closeDocument(QStringLiteral("import-1"));
    EXPECT_EQ(documentsChanged.count(), 1);
    EXPECT_EQ(view_model.openDocuments().size(), 0);
    EXPECT_EQ(view_model.activeDocumentId(), QString{});
    EXPECT_EQ(view_model.mesh_snapshot(), nullptr);
}

TEST(ProjectViewModelTest, ReorderingAndClosingDocumentsPreservesActiveMeshSelection) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());

    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("sample.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid sample\n"
                           "facet normal 0 0 1\n"
                           " outer loop\n"
                           "  vertex 0 0 0\n"
                           "  vertex 1 0 0\n"
                           "  vertex 0 1 0\n"
                           " endloop\n"
                           "endfacet\n"
                           "endsolid sample\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), fixture.path())));
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));

    // 第三个真实资产用于覆盖关闭非活动页签的场景。
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, QStringLiteral("import-3")));

    view_model.activateDocument(QStringLiteral("import-1"));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    const auto first_mesh = view_model.mesh_snapshot();
    ASSERT_NE(first_mesh, nullptr);
    view_model.activateDocument(QStringLiteral("import-2"));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    const auto second_mesh = view_model.mesh_snapshot();
    ASSERT_NE(second_mesh, nullptr);
    ASSERT_NE(first_mesh.get(), second_mesh.get());

    // 拖拽改变显示顺序时，活动身份及视口快照仍按稳定 ID 保持。
    view_model.moveDocument(1, 0);
    const auto reordered = view_model.openDocuments();
    ASSERT_EQ(reordered.size(), 3);
    EXPECT_EQ(reordered[0].toMap()[QStringLiteral("id")].toString(), QStringLiteral("import-2"));
    EXPECT_EQ(reordered[1].toMap()[QStringLiteral("id")].toString(), QStringLiteral("import-1"));
    EXPECT_EQ(reordered[2].toMap()[QStringLiteral("id")].toString(), QStringLiteral("import-3"));
    EXPECT_EQ(view_model.activeDocumentId(), QStringLiteral("import-2"));
    EXPECT_EQ(view_model.mesh_snapshot().get(), second_mesh.get());

    // 关闭非活动页签不改变场景；关闭活动页签优先激活右邻就绪文档。
    view_model.closeDocument(QStringLiteral("import-3"));
    EXPECT_EQ(view_model.activeDocumentId(), QStringLiteral("import-2"));
    EXPECT_EQ(view_model.mesh_snapshot().get(), second_mesh.get());
    view_model.closeDocument(QStringLiteral("import-2"));
    ASSERT_TRUE(wait_for_active_mesh(view_model, QStringLiteral("import-1")));
    EXPECT_EQ(view_model.activeDocumentId(), QStringLiteral("import-1"));
    EXPECT_NE(view_model.mesh_snapshot().get(), first_mesh.get());
    EXPECT_EQ(view_model.mesh_snapshot()->vertices, first_mesh->vertices);

    view_model.closeDocument(QStringLiteral("import-1"));
    EXPECT_TRUE(view_model.activeDocumentId().isEmpty());
    EXPECT_TRUE(view_model.openDocuments().isEmpty());
    EXPECT_EQ(view_model.mesh_snapshot(), nullptr);
}

TEST(ProjectViewModelTest, ReopenStartsEmptyAndActivatesSavedRecordOnDemand) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());

    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("sample.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid sample\n"
                           "facet normal 0 0 1\n"
                           " outer loop\n"
                           "  vertex 0 0 0\n"
                           "  vertex 2 0 0\n"
                           "  vertex 0 1 0\n"
                           " endloop\n"
                           "endfacet\n"
                           "endsolid sample\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), fixture.path())));
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    ASSERT_TRUE(finish_project_operation(view_model, view_model.saveProject()));

    ProjectViewModel reopened;
    ASSERT_TRUE(finish_project_operation(reopened, reopened.openProject(view_model.currentPath())));
    // 重开只读清单，不创建页签或继承旧会话的活动文档。
    EXPECT_TRUE(reopened.openDocuments().isEmpty());
    EXPECT_TRUE(reopened.activeDocumentId().isEmpty());
    EXPECT_EQ(reopened.mesh_snapshot(), nullptr);

    // 点击工程树未打开记录：Loading 页签先建立，激活仍由成功结果驱动。
    reopened.openImportRecord(QStringLiteral("import-1"));
    ASSERT_EQ(reopened.openDocuments().size(), 1);
    const auto loading = reopened.openDocuments()[0].toMap();
    EXPECT_EQ(loading[QStringLiteral("id")].toString(), QStringLiteral("import-1"));
    EXPECT_TRUE(reopened.activeDocumentId().isEmpty());
    QTRY_COMPARE(reopened.openDocuments()[0].toMap()[QStringLiteral("state")].toString(),
                 QStringLiteral("ready"));
    QTRY_COMPARE(reopened.activeDocumentId(), QStringLiteral("import-1"));
    QTRY_VERIFY(reopened.mesh_snapshot() != nullptr);
    EXPECT_EQ(reopened.mesh_snapshot()->vertices.size(), 3U);

    // 未知记录同步拒绝，不建页签。
    const auto before = reopened.openDocuments().size();
    reopened.openImportRecord(QStringLiteral("import-99"));
    EXPECT_EQ(reopened.openDocuments().size(), before);

    // 关闭最后一个页签后留白，再次打开仍可按需激活。
    reopened.closeDocument(QStringLiteral("import-1"));
    EXPECT_TRUE(reopened.openDocuments().isEmpty());
    EXPECT_TRUE(reopened.activeDocumentId().isEmpty());
    EXPECT_EQ(reopened.mesh_snapshot(), nullptr);
    reopened.openImportRecord(QStringLiteral("import-1"));
    QTRY_COMPARE(reopened.activeDocumentId(), QStringLiteral("import-1"));
    QTRY_VERIFY(reopened.mesh_snapshot() != nullptr);
}

TEST(ProjectViewModelTest, DuplicateImportOrdinalsFollowImportOrderAcrossActivationAndClose) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("part.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly));
    ASSERT_GT(source.write("vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n"), 0);
    source.close();

    ProjectViewModel project;
    ASSERT_TRUE(finish_project_operation(
        project, project.createProject(QStringLiteral("Demo"), fixture.path())));
    ASSERT_TRUE(project.importStl(sourcePath, QStringLiteral("solid-3d"),
                                  QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(project, project.activeDocumentId()));
    ASSERT_TRUE(project.importStl(sourcePath, QStringLiteral("solid-3d"),
                                  QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(project, project.activeDocumentId()));
    ASSERT_TRUE(finish_project_operation(project, project.saveProject()));

    ProjectViewModel reopened;
    ASSERT_TRUE(finish_project_operation(reopened, reopened.openProject(project.currentPath())));
    reopened.openImportRecord(QStringLiteral("import-2"));
    QTRY_COMPARE(reopened.activeDocumentId(), QStringLiteral("import-2"));
    reopened.openImportRecord(QStringLiteral("import-1"));
    QTRY_COMPARE(reopened.activeDocumentId(), QStringLiteral("import-1"));

    const auto ordinal_for = [&reopened](const QString& id) {
        for (const auto& entry : reopened.openDocuments()) {
            const auto document = entry.toMap();
            if (document.value(QStringLiteral("id")).toString() == id) {
                return qMakePair(document.value(QStringLiteral("displayOrdinal")).toInt(),
                                 document.value(QStringLiteral("displayCount")).toInt());
            }
        }
        return qMakePair(0, 0);
    };
    EXPECT_EQ(ordinal_for(QStringLiteral("import-1")), qMakePair(1, 2));
    EXPECT_EQ(ordinal_for(QStringLiteral("import-2")), qMakePair(2, 2));

    reopened.closeDocument(QStringLiteral("import-1"));
    EXPECT_EQ(ordinal_for(QStringLiteral("import-2")), qMakePair(2, 2));
}

TEST(ProjectViewModelTest, ClosedDocumentsReleaseTheirMeshSnapshotsAfterRepeatedSwitching) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());

    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("sample.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid sample\n"
                           "facet normal 0 0 1\n"
                           " outer loop\n"
                           "  vertex 0 0 0\n"
                           "  vertex 1 0 0\n"
                           "  vertex 0 1 0\n"
                           " endloop\n"
                           "endfacet\n"
                           "endsolid sample\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), fixture.path())));
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    const std::weak_ptr<const panta::visualization::SurfaceMeshSnapshot> first_snapshot =
        view_model.mesh_snapshot();
    ASSERT_FALSE(first_snapshot.expired());

    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    const std::weak_ptr<const panta::visualization::SurfaceMeshSnapshot> second_snapshot =
        view_model.mesh_snapshot();
    ASSERT_FALSE(second_snapshot.expired());
    EXPECT_TRUE(first_snapshot.expired());

    for (int round = 0; round < 32; ++round) {
        view_model.activateDocument(QStringLiteral("import-1"));
        ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
        ASSERT_NE(view_model.mesh_snapshot(), nullptr);
        EXPECT_TRUE(second_snapshot.expired());
        view_model.activateDocument(QStringLiteral("import-2"));
        ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
        ASSERT_NE(view_model.mesh_snapshot(), nullptr);
        EXPECT_TRUE(first_snapshot.expired());
    }
    EXPECT_TRUE(first_snapshot.expired());
    EXPECT_TRUE(second_snapshot.expired());

    view_model.closeDocument(QStringLiteral("import-1"));
    EXPECT_TRUE(first_snapshot.expired());
    EXPECT_EQ(view_model.activeDocumentId(), QStringLiteral("import-2"));
    EXPECT_NE(view_model.mesh_snapshot(), nullptr);

    const std::weak_ptr<const panta::visualization::SurfaceMeshSnapshot> active_snapshot =
        view_model.mesh_snapshot();
    view_model.closeDocument(QStringLiteral("import-2"));
    EXPECT_TRUE(active_snapshot.expired());
    EXPECT_TRUE(view_model.activeDocumentId().isEmpty());
    EXPECT_EQ(view_model.mesh_snapshot(), nullptr);
}

TEST(ProjectViewModelTest, InitialAndCreatedWorkspaceHaveNoDocuments) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    ProjectViewModel view_model;
    EXPECT_TRUE(view_model.openDocuments().isEmpty());
    EXPECT_TRUE(view_model.activeDocumentId().isEmpty());
    EXPECT_EQ(view_model.mesh_snapshot(), nullptr);

    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), fixture.path())));
    EXPECT_TRUE(view_model.openDocuments().isEmpty());
    EXPECT_TRUE(view_model.activeDocumentId().isEmpty());
    EXPECT_EQ(view_model.mesh_snapshot(), nullptr);
}

TEST(ProjectViewModelTest, FailedLoadRetainsTabAndCloseReleasesActivationState) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());

    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("sample.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid sample\n"
                           "facet normal 0 0 1\n"
                           " outer loop\n"
                           "  vertex 0 0 0\n"
                           "  vertex 1 0 0\n"
                           "  vertex 0 1 0\n"
                           " endloop\n"
                           "endfacet\n"
                           "endsolid sample\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Demo"), fixture.path())));
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    ASSERT_TRUE(finish_project_operation(view_model, view_model.saveProject()));
    const auto assetPath = view_model.importedAssetPath();

    ProjectViewModel reopened;
    ASSERT_TRUE(finish_project_operation(reopened, reopened.openProject(view_model.currentPath())));

    // 资产损坏 → 加载失败：页签保留 Failed 态并携带原因，视口保持空白。
    ASSERT_TRUE(QFile::rename(assetPath, assetPath + QStringLiteral(".bak")));
    reopened.openImportRecord(QStringLiteral("import-1"));
    QTRY_COMPARE(reopened.openDocuments().size(), 1);
    QTRY_COMPARE(reopened.openDocuments()[0].toMap()[QStringLiteral("state")].toString(),
                 QStringLiteral("failed"));
    EXPECT_EQ(reopened.openDocuments()[0].toMap()[QStringLiteral("message")].toString(),
              QStringLiteral("工程包中缺少已保存的网格资产。"));
    const auto diagnostic =
        reopened.openDocuments()[0].toMap().value(QStringLiteral("diagnostic")).toMap();
    EXPECT_EQ(diagnostic.value(QStringLiteral("code")).toString(),
              QStringLiteral("project.asset_missing"));
    EXPECT_EQ(diagnostic.value(QStringLiteral("category")).toString(), QStringLiteral("missing"));
    EXPECT_FALSE(diagnostic.value(QStringLiteral("detail")).toString().isEmpty());
    EXPECT_TRUE(reopened.activeDocumentId().isEmpty());
    EXPECT_EQ(reopened.mesh_snapshot(), nullptr);

    // 关闭 Failed 页签：释放激活簿记，页签栏恢复为空。
    reopened.closeDocument(QStringLiteral("import-1"));
    EXPECT_TRUE(reopened.openDocuments().isEmpty());
    EXPECT_TRUE(reopened.activeDocumentId().isEmpty());

    // 资产恢复后重新打开：新 attempt 加载成功并激活。
    ASSERT_TRUE(QFile::rename(assetPath + QStringLiteral(".bak"), assetPath));
    reopened.openImportRecord(QStringLiteral("import-1"));
    QTRY_COMPARE(reopened.activeDocumentId(), QStringLiteral("import-1"));
    QTRY_VERIFY(reopened.mesh_snapshot() != nullptr);
    EXPECT_EQ(reopened.mesh_snapshot()->vertices.size(), 3U);
    EXPECT_TRUE(
        reopened.openDocuments()[0].toMap().value(QStringLiteral("diagnostic")).toMap().isEmpty());
}

TEST(ProjectViewModelTest, ExportsTheActiveMeshInTheBackgroundAndReportsFailures) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("sample.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid sample\nfacet normal 0 0 1\n outer loop\n"
                           "  vertex 0 0 0\n  vertex 1 0 0\n  vertex 0 1 0\n"
                           " endloop\nendfacet\nendsolid sample\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Export"), fixture.path())));
    // 没有活动网格时拒绝导出，不启动后台任务。
    EXPECT_FALSE(view_model.beginExportActiveMeshUrl(
        QUrl::fromLocalFile(QDir(fixture.path()).filePath(QStringLiteral("none.stl")))));
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.mesh_not_resident"));
    EXPECT_FALSE(view_model.exportPending());

    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("dual-domain"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));

    QSignalSpy finished(&view_model, &ProjectViewModel::exportFinished);
    const QString target = QDir(fixture.path()).filePath(QStringLiteral("导出.vtp"));
    ASSERT_TRUE(view_model.beginExportActiveMeshUrl(QUrl::fromLocalFile(target)));
    EXPECT_TRUE(view_model.exportPending());
    ASSERT_TRUE(finished.wait(10000));
    EXPECT_TRUE(finished.takeFirst().at(1).toBool());
    EXPECT_FALSE(view_model.exportPending());
    EXPECT_TRUE(view_model.error().isEmpty());
    EXPECT_GT(QFileInfo(target).size(), 0);

    ASSERT_TRUE(view_model.beginExportActiveMeshUrl(
        QUrl::fromLocalFile(QDir(fixture.path()).filePath(QStringLiteral("out.3mf")))));
    ASSERT_TRUE(finished.wait(10000));
    EXPECT_FALSE(finished.takeFirst().at(1).toBool());
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.export_unsupported_format"));
}

TEST(ProjectViewModelTest, DiagnosesTheActiveMeshInTheBackground) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("open.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid open\nfacet normal 0 0 1\n outer loop\n"
                           "  vertex 0 0 0\n  vertex 1 0 0\n  vertex 0 1 0\n"
                           " endloop\nendfacet\nendsolid open\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Diagnose"), fixture.path())));
    EXPECT_FALSE(view_model.beginDiagnoseActiveMesh());
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.mesh_not_resident"));
    EXPECT_FALSE(view_model.diagnosticsPending());

    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("dual-domain"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    QSignalSpy finished(&view_model, &ProjectViewModel::diagnosticsFinished);
    ASSERT_TRUE(view_model.beginDiagnoseActiveMesh());
    EXPECT_TRUE(view_model.diagnosticsPending());
    ASSERT_TRUE(finished.wait(10000));
    EXPECT_TRUE(finished.takeFirst().at(0).toBool());
    EXPECT_FALSE(view_model.diagnosticsPending());
    const QVariantMap quality = view_model.meshQuality();
    EXPECT_EQ(quality.value(QStringLiteral("documentId")).toString(),
              view_model.activeDocumentId());
    EXPECT_EQ(quality.value(QStringLiteral("triangleCount")).toULongLong(), 1U);
    EXPECT_EQ(quality.value(QStringLiteral("boundaryEdgeCount")).toULongLong(), 3U);
    EXPECT_EQ(quality.value(QStringLiteral("holeCount")).toULongLong(), 1U);
    EXPECT_FALSE(quality.value(QStringLiteral("clean")).toBool());
}

TEST(ProjectViewModelTest, ReportsProgressAndCancelsDiagnosticsAndExport) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("part.stl"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("solid part\nfacet normal 0 0 1\n outer loop\n"
                           "  vertex 0 0 0\n  vertex 1 0 0\n  vertex 0 1 0\n"
                           " endloop\nendfacet\nendsolid part\n"),
              0);
    source.close();

    HeldBackgroundViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Progress"), fixture.path())));
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("dual-domain"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    EXPECT_EQ(view_model.importProgress(), -1.0);
    EXPECT_EQ(view_model.diagnosticsProgress(), -1.0);
    EXPECT_FALSE(view_model.cancelDiagnostics());
    EXPECT_FALSE(view_model.cancelExport());

    view_model.holdWork = true;
    QSignalSpy diagnosed(&view_model, &ProjectViewModel::diagnosticsFinished);
    ASSERT_TRUE(view_model.beginDiagnoseActiveMesh());
    EXPECT_EQ(view_model.diagnosticsProgress(), 0.0);
    EXPECT_TRUE(view_model.cancelDiagnostics());
    view_model.releaseFirst();
    ASSERT_TRUE(diagnosed.wait(10000));
    EXPECT_FALSE(diagnosed.takeFirst().at(0).toBool());
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.diagnostics_cancelled"));
    EXPECT_EQ(view_model.diagnosticsProgress(), -1.0);

    const QString target = QDir(fixture.path()).filePath(QStringLiteral("out.stl"));
    QSignalSpy exported(&view_model, &ProjectViewModel::exportFinished);
    ASSERT_TRUE(view_model.beginExportActiveMeshUrl(QUrl::fromLocalFile(target)));
    EXPECT_EQ(view_model.exportProgress(), 0.0);
    EXPECT_TRUE(view_model.cancelExport());
    view_model.releaseFirst();
    ASSERT_TRUE(exported.wait(10000));
    EXPECT_FALSE(exported.takeFirst().at(1).toBool());
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.export_cancelled"));
    EXPECT_FALSE(QFileInfo::exists(target));

    ASSERT_TRUE(view_model.beginExportActiveMeshUrl(QUrl::fromLocalFile(target)));
    view_model.releaseFirst();
    ASSERT_TRUE(exported.wait(10000));
    EXPECT_TRUE(exported.takeFirst().at(1).toBool());
    EXPECT_TRUE(QFileInfo::exists(target));
    EXPECT_EQ(view_model.exportProgress(), -1.0);
}

TEST(ProjectViewModelTest, RunsVolumeQualitySectionAndVtuExportInTheBackground) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("tet.vtu"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write(R"(<?xml version="1.0"?>
<VTKFile type="UnstructuredGrid" version="0.1" byte_order="LittleEndian" header_type="UInt32">
  <UnstructuredGrid>
    <Piece NumberOfPoints="4" NumberOfCells="1">
      <Points>
        <DataArray type="Float32" NumberOfComponents="3" format="ascii">
          0 0 0 1 0 0 0 1 0 0 0 1
        </DataArray>
      </Points>
      <Cells>
        <DataArray type="Int32" Name="connectivity" format="ascii">0 1 2 3</DataArray>
        <DataArray type="Int32" Name="offsets" format="ascii">4</DataArray>
        <DataArray type="UInt8" Name="types" format="ascii">10</DataArray>
      </Cells>
    </Piece>
  </UnstructuredGrid>
</VTKFile>
)"),
              0);
    source.close();

    HeldBackgroundViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Volume"), fixture.path())));
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    ASSERT_TRUE(view_model.volumeAvailable());

    // 体网格质量在后台诊断中计算并进入诊断面板数据。
    QSignalSpy diagnosed(&view_model, &ProjectViewModel::diagnosticsFinished);
    ASSERT_TRUE(view_model.beginDiagnoseActiveMesh());
    ASSERT_TRUE(diagnosed.wait(10000));
    EXPECT_TRUE(diagnosed.takeFirst().at(0).toBool());
    const QVariantMap quality = view_model.meshQuality();
    EXPECT_TRUE(quality.value(QStringLiteral("volumeAvailable")).toBool());
    EXPECT_EQ(quality.value(QStringLiteral("tetCount")).toULongLong(), 1U);
    EXPECT_EQ(quality.value(QStringLiteral("nodeCount")).toULongLong(), 4U);
    EXPECT_GT(quality.value(QStringLiteral("minDihedralDegrees")).toDouble(), 0.0);

    // 切面在后台计算；连续请求合并为最新意图。
    view_model.holdWork = true;
    QSignalSpy meshChanged(&view_model, &ProjectViewModel::meshChanged);
    ASSERT_TRUE(view_model.setSection(true, 1.0, 0.0, 0.0, 0.5));
    ASSERT_EQ(view_model.held.size(), 1U);
    ASSERT_TRUE(view_model.setSection(true, 1.0, 0.0, 0.0, 0.25));
    ASSERT_EQ(view_model.held.size(), 1U);
    view_model.releaseFirst();
    ASSERT_TRUE(QTest::qWaitFor([&] { return view_model.held.size() == 1U; }, 10000));
    EXPECT_EQ(meshChanged.count(), 0);
    view_model.releaseFirst();
    ASSERT_TRUE(meshChanged.wait(10000));
    ASSERT_NE(view_model.mesh_snapshot(), nullptr);
    const auto& vertices = view_model.mesh_snapshot()->vertices;
    ASSERT_FALSE(vertices.empty());
    EXPECT_EQ(vertices.size() % 3, 0U);
    bool onPlane = false;
    bool beyondPlane = false;
    bool behindPlane = false;
    for (const auto& vertex : vertices) {
        onPlane = onPlane || std::abs(vertex[0] - 0.25) < 1e-9;
        beyondPlane = beyondPlane || vertex[0] > 0.25 + 1e-9;
        behindPlane = behindPlane || vertex[0] < 0.25 - 1e-9;
    }
    // 最新意图（x=0.25）生效：切口落在该平面，且只保留一侧。
    EXPECT_TRUE(onPlane);
    EXPECT_FALSE(beyondPlane && behindPlane);
    view_model.holdWork = false;

    // 关闭切面后 .vtu 导出在后台写四面体单元。
    // 任意平面（task 024）：法向属性往返；零法向与非有限值在启用时被拒绝且不改变已有平面。
    EXPECT_FALSE(view_model.setSection(true, 0.0, 0.0, 0.0, 0.5));
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.command_invalid"));
    EXPECT_FALSE(view_model.setSection(true, std::nan(""), 0.0, 1.0, 0.5));
    EXPECT_EQ(view_model.sectionNormalX(), 1.0);
    EXPECT_EQ(view_model.sectionFraction(), 0.25);
    ASSERT_TRUE(view_model.setSection(true, 0.0, 1.0, 1.0, 0.75));
    EXPECT_EQ(view_model.sectionNormalX(), 0.0);
    EXPECT_EQ(view_model.sectionNormalY(), 1.0);
    EXPECT_EQ(view_model.sectionNormalZ(), 1.0);
    EXPECT_EQ(view_model.sectionFraction(), 0.75);
    ASSERT_TRUE(meshChanged.wait(10000));
    ASSERT_NE(view_model.mesh_snapshot(), nullptr);
    // 单位四面体在法向 (0,1,1)/√2 上的投影区间为 [0, 1/√2]，0.75 处即平面 y+z = 0.75；
    // 保留 y+z <= 0.75 一侧，且存在落在平面上的切口顶点。
    bool obliqueOnPlane = false;
    for (const auto& vertex : view_model.mesh_snapshot()->vertices) {
        EXPECT_LE(vertex[1] + vertex[2], 0.75 + 1e-9);
        obliqueOnPlane = obliqueOnPlane || std::abs(vertex[1] + vertex[2] - 0.75) < 1e-9;
    }
    EXPECT_TRUE(obliqueOnPlane);
    ASSERT_TRUE(view_model.setSection(false, 0.0, 0.0, 0.0, 0.25));
    EXPECT_EQ(view_model.sectionNormalY(), 1.0);
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    const QString target = QDir(fixture.path()).filePath(QStringLiteral("out.vtu"));
    QSignalSpy exported(&view_model, &ProjectViewModel::exportFinished);
    ASSERT_TRUE(view_model.beginExportActiveMeshUrl(QUrl::fromLocalFile(target)));
    ASSERT_TRUE(exported.wait(10000));
    EXPECT_TRUE(exported.takeFirst().at(1).toBool());
    QFile written(target);
    ASSERT_TRUE(written.open(QIODevice::ReadOnly));
    EXPECT_TRUE(written.readAll().contains("NumberOfCells=\"1\""));
}

TEST(ProjectViewModelTest, ColorsSectionsByResultFieldAndShowsLegend) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const QString sourcePath = QDir(fixture.path()).filePath(QStringLiteral("fields.vtk"));
    QFile source(sourcePath);
    ASSERT_TRUE(source.open(QIODevice::WriteOnly | QIODevice::Text));
    ASSERT_GT(source.write("# vtk DataFile Version 4.2\nfields\nASCII\nDATASET UNSTRUCTURED_GRID\n"
                           "POINTS 5 double\n0 0 0 1 0 0 0 1 0 0 0 1 1 1 1\n"
                           "CELLS 2 10\n4 0 1 2 3\n4 1 2 3 4\nCELL_TYPES 2\n10 10\n"
                           "POINT_DATA 5\nSCALARS temperature double\nLOOKUP_TABLE default\n"
                           "300 nan 310 320 330\n"
                           "VECTORS velocity double\n1 0 0 0 2 0 0 0 3 0 0 0 1 1 1\n"
                           "CELL_DATA 2\nSCALARS material int\nLOOKUP_TABLE default\n1 2\n"),
              0);
    source.close();

    ProjectViewModel view_model;
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Fields"), fixture.path())));
    ASSERT_TRUE(view_model.importStl(sourcePath, QStringLiteral("solid-3d"),
                                     QStringLiteral("millimeters"), false));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    ASSERT_TRUE(view_model.volumeAvailable());
    const auto fields = view_model.resultFields();
    ASSERT_EQ(fields.size(), 3);
    EXPECT_EQ(fields[1].toMap().value(QStringLiteral("name")).toString(),
              QStringLiteral("velocity"));
    EXPECT_EQ(fields[1].toMap().value(QStringLiteral("components")).toInt(), 3);
    EXPECT_EQ(fields[2].toMap().value(QStringLiteral("association")).toString(),
              QStringLiteral("cell"));
    EXPECT_TRUE(fields[0].toMap().value(QStringLiteral("sectionable")).toBool());
    EXPECT_EQ(ProjectViewModel::fieldLegendStops().size(), 3);
    EXPECT_EQ(ProjectViewModel::fieldMissingColor(), QStringLiteral("#ffff00"));

    // 非法选择被拒绝且不改变当前选择。
    EXPECT_FALSE(
        view_model.setSectionField(QStringLiteral("velocity"), QStringLiteral("cell"), -1));
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.command_invalid"));
    EXPECT_FALSE(view_model.setSectionField(QStringLiteral("velocity"), QStringLiteral("node"), 3));
    EXPECT_FALSE(
        view_model.setSectionField(QStringLiteral("temperature"), QStringLiteral("node"), 1));
    EXPECT_TRUE(view_model.sectionFieldName().isEmpty());

    // 先选字段再启用切面：切面带逐顶点标量，色标显示有限值范围与缺失数。
    ASSERT_TRUE(
        view_model.setSectionField(QStringLiteral("temperature"), QStringLiteral("node"), 0));
    EXPECT_EQ(view_model.sectionFieldComponent(), -1);
    QSignalSpy meshChanged(&view_model, &ProjectViewModel::meshChanged);
    ASSERT_TRUE(view_model.setSection(true, 1.0, 1.0, 1.0, 0.5));
    ASSERT_TRUE(meshChanged.wait(10000));
    auto mesh = view_model.mesh_snapshot();
    ASSERT_NE(mesh, nullptr);
    ASSERT_FALSE(mesh->vertices.empty());
    EXPECT_EQ(mesh->scalars.size(), mesh->vertices.size());
    EXPECT_TRUE(view_model.fieldLegendVisible());
    EXPECT_EQ(view_model.fieldLegendTitle(), QStringLiteral("temperature"));
    EXPECT_TRUE(view_model.fieldLegendHasRange());
    EXPECT_GE(view_model.fieldLegendMin(), 300.0);
    EXPECT_LE(view_model.fieldLegendMax(), 330.0);
    EXPECT_GT(view_model.fieldMissingVertices(), 0.0);

    // 换分量：重算切面，标题带分量名。
    ASSERT_TRUE(view_model.setSectionField(QStringLiteral("velocity"), QStringLiteral("node"), 1));
    ASSERT_TRUE(meshChanged.wait(10000));
    EXPECT_EQ(view_model.fieldLegendTitle(), QStringLiteral("velocity [Y]"));
    EXPECT_EQ(view_model.fieldMissingVertices(), 0.0);

    // 清除字段：切面回到单色，色标隐藏；关闭切面同样隐藏。
    ASSERT_TRUE(view_model.setSectionField(QString(), QString(), -1));
    ASSERT_TRUE(meshChanged.wait(10000));
    mesh = view_model.mesh_snapshot();
    ASSERT_NE(mesh, nullptr);
    EXPECT_TRUE(mesh->scalars.empty());
    EXPECT_FALSE(view_model.fieldLegendVisible());
    ASSERT_TRUE(view_model.setSectionField(QStringLiteral("material"), QStringLiteral("cell"), -1));
    ASSERT_TRUE(meshChanged.wait(10000));
    EXPECT_TRUE(view_model.fieldLegendVisible());
    ASSERT_TRUE(view_model.setSection(false, 0.0, 0.0, 0.0, 0.5));
    EXPECT_FALSE(view_model.fieldLegendVisible());
    EXPECT_EQ(view_model.sectionFieldName(), QStringLiteral("material"));
}

TEST(ProjectViewModelTest, GeneratesVolumeMeshFromStepInTheBackgroundAndHonoursCancel) {
    QTemporaryDir fixture;
    ASSERT_TRUE(fixture.isValid());
    const QString stepPath =
        QDir(QString::fromUtf8(PANTA_STRINGIFY(PANTA_BRIDGE_GEOMETRY_FIXTURE_DIR)))
            .filePath(QStringLiteral("box_mm.step"));
    ASSERT_TRUE(QFileInfo::exists(stepPath));

    ProjectViewModel view_model;
    // 未打开工程时拒绝。
    EXPECT_FALSE(view_model.beginGenerateVolumeFromStep(stepPath, 0.0));
    ASSERT_TRUE(finish_project_operation(
        view_model, view_model.createProject(QStringLiteral("Generated"), fixture.path())));

    // 参数校验：负尺寸、超限尺寸与缺失文件在提交后台作业前拒绝。
    EXPECT_FALSE(view_model.beginGenerateVolumeFromStep(stepPath, -1.0));
    EXPECT_FALSE(view_model.beginGenerateVolumeFromStep(stepPath, 2.0e6));
    EXPECT_FALSE(view_model.beginGenerateVolumeFromStep(
        QDir(fixture.path()).filePath(QStringLiteral("missing.step")), 0.0));
    EXPECT_FALSE(view_model.generationPending());

    // 启动后立即取消：不发布任何导入记录。
    QSignalSpy finished(&view_model, &ProjectViewModel::generationFinished);
    ASSERT_TRUE(view_model.beginGenerateVolumeFromStep(stepPath, 1.0));
    EXPECT_TRUE(view_model.generationPending());
    EXPECT_FALSE(view_model.beginGenerateVolumeFromStep(stepPath, 0.0));
    view_model.cancelGeneration();
    ASSERT_TRUE(finished.wait(60000));
    EXPECT_FALSE(finished.takeFirst().at(0).toBool());
    EXPECT_EQ(view_model.errorCode(), QStringLiteral("project.volume_generation_cancelled"));
    EXPECT_FALSE(view_model.generationPending());
    EXPECT_TRUE(view_model.importedPartNames().isEmpty());

    // 正常生成：后台完成后发布为 solid-3d 体网格资产并成为活动文档。
    ASSERT_TRUE(view_model.beginGenerateVolumeFromStep(stepPath, 0.0));
    ASSERT_TRUE(finished.wait(120000));
    EXPECT_TRUE(finished.takeFirst().at(0).toBool());
    EXPECT_FALSE(view_model.generationPending());
    EXPECT_EQ(view_model.generationProgress(), -1.0);
    ASSERT_EQ(view_model.importedPartNames().size(), 1);
    EXPECT_EQ(view_model.importedPartNames().front(), QStringLiteral("box_mm"));
    EXPECT_EQ(view_model.importedMeshType(), QStringLiteral("solid-3d"));
    ASSERT_TRUE(wait_for_active_mesh(view_model, view_model.activeDocumentId()));
    EXPECT_TRUE(view_model.volumeAvailable());
}
