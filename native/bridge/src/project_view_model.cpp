#include "project_view_model.hpp"

#include "panta/visualization/mesh_source.hpp"
#include "panta_ffi.h"
#include "utf8_boundary.hpp"
#include <QDir>
#include <QFileInfo>
#include <QFuture>
#include <QFutureWatcher>
#include <QLatin1Char>
#include <QMetaType>
#include <QObject>
#include <QPromise>
#include <QString>
#include <QUrl>
#include <QVariantMap>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qnamespace.h>
#include <QtCore/qtmetamacros.h>
#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <exception>
#include <functional>
#include <future>
#include <memory>
#include <optional>
#include <panta/qt_adapter/standard_paths.hpp>
#include <string>
#include <system_error>
#include <thread>
#include <utility>

namespace panta::bridge {

namespace {

QString default_project_location() {
    const QString documents =
        panta::qt_adapter::standard_location(panta::qt_adapter::StandardLocation::Documents);
    return documents.isEmpty() ? QString{} : QDir(documents).filePath(QStringLiteral("panta"));
}

QString project_error_message(const QString& code) {
    return QString::fromUtf8(panta::ffi::project_error_message(code.toStdString()));
}

QVariantList choice_catalog(const rust::Vec<panta::ffi::ChoiceDefinition>& catalog) {
    QVariantList result;
    for (const auto& entry : catalog) {
        result.append(
            QVariantMap{{QStringLiteral("id"), QString::fromUtf8(entry.id)},
                        {QStringLiteral("sourceText"), QString::fromUtf8(entry.source_text)}});
    }
    return result;
}

} // namespace

ProjectViewModel::ProjectViewModel(QObject* parent)
    : MeshSource(parent), m_defaultLocation(default_project_location()),
      m_meshTypes(choice_catalog(panta::ffi::mesh_type_catalog())),
      m_defaultMeshType(QString::fromUtf8(panta::ffi::default_mesh_type())),
      m_service(panta::ffi::project_service_new()) {
    // 激活结果为拉取式队列（073）；仅在存在 Loading 文档时每 10ms 轮询。
    m_activationPoll.setInterval(10);
    connect(&m_activationPoll, &QTimer::timeout, this, &ProjectViewModel::drain_activations);
    m_importPreviewPoll.setInterval(10);
    connect(&m_importPreviewPoll, &QTimer::timeout, this, &ProjectViewModel::finish_import_preview);
    m_stlImportPoll.setInterval(10);
    connect(&m_stlImportPoll, &QTimer::timeout, this, &ProjectViewModel::finish_stl_import);
    reset_documents();
}

std::shared_ptr<const panta::visualization::SurfaceMeshSnapshot>
ProjectViewModel::mesh_snapshot() const {
    // 绘制刷新路径只读取这个本地共享快照，不能在这里跨 CXX 调 Rust。
    // 无活动资产或加载中返回空，视口保持空白。
    return m_activeMesh;
}

const QString& ProjectViewModel::defaultLocation() const { return m_defaultLocation; }

QUrl ProjectViewModel::defaultLocationUrl() const {
    return m_defaultLocation.isEmpty() ? QUrl{} : QUrl::fromLocalFile(m_defaultLocation);
}

const QString& ProjectViewModel::lastCreatedPath() const { return m_lastCreatedPath; }

const QString& ProjectViewModel::error() const { return m_error; }

const QString& ProjectViewModel::errorCode() const { return m_errorCode; }

const QString& ProjectViewModel::errorCategory() const { return m_errorCategory; }

const QString& ProjectViewModel::errorDetail() const { return m_errorDetail; }

const QString& ProjectViewModel::currentPath() const { return m_currentPath; }

const QString& ProjectViewModel::currentName() const { return m_currentName; }

bool ProjectViewModel::dirty() const { return m_dirty; }

const QStringList& ProjectViewModel::importedPartNames() const { return m_importedPartNames; }

const QString& ProjectViewModel::importedPartName() const { return m_importedPartName; }

const QString& ProjectViewModel::importedAssetPath() const { return m_importedAssetPath; }

QVariantList ProjectViewModel::meshTypes() const { return m_meshTypes; }

QString ProjectViewModel::defaultMeshType() const { return m_defaultMeshType; }

const QString& ProjectViewModel::importedMeshType() const { return m_importedMeshType; }

const QString& ProjectViewModel::importedUnits() const { return m_importedUnits; }

const QString& ProjectViewModel::importedDimensions() const { return m_importedDimensions; }

quint64 ProjectViewModel::importedTriangleCount() const { return m_importedTriangleCount; }

bool ProjectViewModel::importPreviewPending() const { return m_importPreviewPending; }

bool ProjectViewModel::importPreviewReady() const { return m_importPreviewReady; }

const QString& ProjectViewModel::importPreviewName() const { return m_importPreviewName; }

const QString& ProjectViewModel::importPreviewDimensions() const {
    return m_importPreviewDimensions;
}

quint64 ProjectViewModel::importPreviewTriangleCount() const {
    return m_importPreviewTriangleCount;
}

bool ProjectViewModel::importPending() const { return m_importPending; }

quint64 ProjectViewModel::importRequestId() const { return m_importRequest; }

bool ProjectViewModel::createProject(const QString& rawName, const QString& rawLocation) {
    clearError();
    std::string name;
    std::string location;
    QString conversionError;
    if (!toBoundaryText(rawName.trimmed(), &name, &conversionError) ||
        !toBoundaryText(QDir::cleanPath(QDir::fromNativeSeparators(rawLocation.trimmed())),
                        &location, &conversionError)) {
        return fail(conversionError);
    }
    return beginProjectOperation({panta::ffi::ProjectOperationKind::Create, location, name},
                                 QStringLiteral("create"));
}

void ProjectViewModel::clearError() {
    if (m_error.isEmpty() && m_errorCode.isEmpty() && m_errorCategory.isEmpty() &&
        m_errorDetail.isEmpty()) {
        return;
    }
    m_error.clear();
    m_errorCode.clear();
    m_errorCategory.clear();
    m_errorDetail.clear();
    emit errorChanged();
}

bool ProjectViewModel::openProject(const QString& path) {
    clearError();
    std::string boundaryPath;
    QString conversionError;
    if (!toBoundaryText(QDir::cleanPath(QDir::fromNativeSeparators(path.trimmed())), &boundaryPath,
                        &conversionError)) {
        return fail(conversionError);
    }
    return beginProjectOperation({panta::ffi::ProjectOperationKind::Open, boundaryPath, {}},
                                 QStringLiteral("open"));
}

bool ProjectViewModel::openProjectUrl(const QUrl& url) {
    const QString path = localPath(url);
    return path.isEmpty() ? fail(QStringLiteral("project.file_not_local")) : openProject(path);
}

QString ProjectViewModel::localPath(const QUrl& url) const {
    return url.isLocalFile() ? url.toLocalFile() : QString{};
}

bool ProjectViewModel::saveProject() {
    clearError();
    return beginProjectOperation({panta::ffi::ProjectOperationKind::Save, {}, {}},
                                 QStringLiteral("save"));
}

void ProjectViewModel::startBackgroundWaiter(std::function<void()> work) {
    std::thread(std::move(work)).detach();
}

bool ProjectViewModel::beginProjectOperation(panta::ffi::ProjectOperationInput input,
                                             const QString& operation) {
    if (m_projectOperationPending) {
        return fail(QStringLiteral("project.command_invalid"), QStringLiteral("state"),
                    QStringLiteral("project operation pending"));
    }
    using Request = rust::Box<panta::ffi::ProjectOperationRequest>;
    using Completion = rust::Box<panta::ffi::ProjectOperationCompletion>;
    using Result = std::shared_ptr<Completion>;
    using Watcher = QFutureWatcher<Result>;
    const auto gate = std::make_shared<std::promise<std::optional<Request>>>();
    const auto ready = std::make_shared<std::future<std::optional<Request>>>(gate->get_future());
    const auto result = std::make_shared<QPromise<Result>>();
    auto watcher = std::make_unique<Watcher>();
    result->start();
    watcher->setFuture(result->future());
    connect(watcher.get(), &Watcher::finished, watcher.get(), &QObject::deleteLater);
    connect(
        watcher.get(), &Watcher::finished, this,
        [this, future = result->future(), operation] {
            if (future.resultCount() == 0)
                return;
            const auto completion = future.result();
            const auto snapshot =
                panta::ffi::project_service_finish_operation(*m_service, std::move(*completion));
            bool succeeded = applySnapshot(snapshot);
            if (succeeded &&
                (operation == QStringLiteral("create") || operation == QStringLiteral("open"))) {
                cancelImportPreview();
                succeeded = refreshImports();
                reset_documents();
            }
            // 先完成本次错误投影，再允许 pendingChanged 接收者准入下一命令。
            if (succeeded)
                clearError();
            m_projectOperationPending = false;
            emit projectOperationPendingChanged();
            if (succeeded) {
                if (operation == QStringLiteral("create")) {
                    m_lastCreatedPath = m_currentPath;
                    emit projectCreated(m_lastCreatedPath);
                } else if (operation == QStringLiteral("open")) {
                    emit projectOpened(m_currentPath);
                } else if (operation == QStringLiteral("save")) {
                    emit projectSaved(m_currentPath);
                }
            }
            emit projectOperationFinished(operation, succeeded);
        },
        Qt::QueuedConnection);
    try {
        // 等待线程先启动，避免 Rust 已接受持久化命令后无人消费终态。
        startBackgroundWaiter([ready, result] {
            auto request = ready->get();
            if (request)
                result->addResult(std::make_shared<Completion>(
                    panta::ffi::project_operation_wait(std::move(*request))));
            result->finish();
        });
    } catch (const std::system_error& error) {
        return fail(QStringLiteral("project.io"), QStringLiteral("io"),
                    QString::fromUtf8(error.what()));
    }
    watcher.release()->setParent(this);
    auto request = panta::ffi::project_service_begin_operation(*m_service, std::move(input));
    if (!panta::ffi::project_operation_accepted(*request)) {
        gate->set_value(std::nullopt);
        return applySnapshot(panta::ffi::project_service_finish_operation(
            *m_service, panta::ffi::project_operation_wait(std::move(request))));
    }
    m_projectOperationPending = true;
    gate->set_value(std::move(request));
    emit projectOperationPendingChanged();
    return true;
}

void ProjectViewModel::prepareActiveMesh() {
    // 最多一个后台显示缓冲在准备；连续选择合并为最新意图，不无限创建线程/快照。
    if (m_meshPreparationPending || m_activeDocumentId.isEmpty())
        return;
    const auto found = std::find_if(
        m_openDocuments.cbegin(), m_openDocuments.cend(), [this](const QVariant& item) {
            const auto document = item.toMap();
            return document.value(QStringLiteral("id")).toString() == m_activeDocumentId &&
                   document.value(QStringLiteral("kind")).toString() == QStringLiteral("import") &&
                   document.value(QStringLiteral("state")).toString() == QStringLiteral("ready");
        });
    if (found == m_openDocuments.cend())
        return;
    using Input = rust::Box<panta::ffi::MeshDisplayInput>;
    struct Prepared {
        std::shared_ptr<const panta::visualization::SurfaceMeshSnapshot> mesh;
        panta::ffi::ProjectDiagnostic error;
    };
    using Result = std::shared_ptr<Prepared>;
    using Watcher = QFutureWatcher<Result>;
    const auto gate = std::make_shared<std::promise<Input>>();
    const auto ready = std::make_shared<std::future<Input>>(gate->get_future());
    const auto result = std::make_shared<QPromise<Result>>();
    auto watcher = std::make_unique<Watcher>();
    result->start();
    watcher->setFuture(result->future());
    connect(watcher.get(), &Watcher::finished, watcher.get(), &QObject::deleteLater);
    const auto selection = m_meshSelection;
    connect(
        watcher.get(), &Watcher::finished, this,
        [this, future = result->future(), selection] {
            m_meshPreparationPending = false;
            const auto prepared = future.result();
            if (selection != m_meshSelection) {
                prepared->mesh.reset();
                prepareActiveMesh();
                return;
            }
            if (!prepared->error.code.empty()) {
                fail(prepared->error);
                return;
            }
            const auto current = panta::ffi::project_service_current(*m_service);
            if (!current.error.code.empty())
                return;
            if (prepared->mesh && prepared->mesh->project_revision != current.value.revision) {
                prepared->mesh.reset();
                prepareActiveMesh();
                return;
            }
            m_activeMesh = std::move(prepared->mesh);
            emit meshChanged();
        },
        Qt::QueuedConnection);
    try {
        startBackgroundWaiter([ready, result] {
            auto prepared = std::make_shared<Prepared>();
            auto source = panta::ffi::prepare_mesh_display(ready->get());
            prepared->error = std::move(source.error);
            if (prepared->error.code.empty()) {
                try {
                    auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
                    mesh->project_revision = source.value.revision;
                    mesh->vertices.reserve(source.value.coordinates.size() / 3);
                    for (std::size_t offset = 0; offset < source.value.coordinates.size();
                         offset += 3) {
                        mesh->vertices.push_back({source.value.coordinates[offset],
                                                  source.value.coordinates[offset + 1],
                                                  source.value.coordinates[offset + 2]});
                    }
                    prepared->mesh = std::move(mesh);
                } catch (const std::exception& error) {
                    prepared->error = {rust::String("project.mesh_allocation_failed"),
                                       rust::String("resource"), rust::String(error.what())};
                }
            }
            result->addResult(prepared);
            result->finish();
        });
    } catch (const std::system_error& error) {
        fail(QStringLiteral("project.io"), QStringLiteral("io"), QString::fromUtf8(error.what()));
        return;
    }
    watcher.release()->setParent(this);
    m_meshPreparationPending = true;
    gate->set_value(panta::ffi::project_service_mesh_display_input(
        *m_service, m_activeDocumentId.toStdString()));
}

bool ProjectViewModel::renameProject(const QString& name) {
    clearError();
    std::string boundaryName;
    QString conversionError;
    if (!toBoundaryText(name.trimmed(), &boundaryName, &conversionError)) {
        return fail(conversionError);
    }
    const auto snapshot = panta::ffi::project_service_execute(
        *m_service,
        panta::ffi::ProjectCommand{panta::ffi::ProjectCommandKind::Rename, boundaryName});
    return applySnapshot(snapshot);
}

bool ProjectViewModel::importStl(const QString& rawPath, const QString& rawMeshType,
                                 const QString& rawUnits, bool showImportLog) {
    clearError();
    std::string path;
    std::string meshType;
    std::string units;
    QString conversionError;
    if (!toBoundaryText(QDir::cleanPath(QDir::fromNativeSeparators(rawPath.trimmed())), &path,
                        &conversionError) ||
        !toBoundaryText(rawMeshType.trimmed(), &meshType, &conversionError) ||
        !toBoundaryText(rawUnits.trimmed(), &units, &conversionError)) {
        return fail(conversionError);
    }
    const auto result =
        panta::ffi::project_service_import_stl(*m_service, path, meshType, units, showImportLog);
    if (!result.error.code.empty()) {
        return fail(result.error);
    }
    return publish_import(result.value);
}

bool ProjectViewModel::beginImportStl(const QString& rawPath, const QString& rawMeshType,
                                      const QString& rawUnits, bool showImportLog) {
    clearError();
    std::string path;
    std::string meshType;
    std::string units;
    QString conversionError;
    if (!toBoundaryText(QDir::cleanPath(QDir::fromNativeSeparators(rawPath.trimmed())), &path,
                        &conversionError) ||
        !toBoundaryText(rawMeshType.trimmed(), &meshType, &conversionError) ||
        !toBoundaryText(rawUnits.trimmed(), &units, &conversionError)) {
        return fail(conversionError);
    }
    const auto result = panta::ffi::project_service_begin_stl_import(*m_service, path, meshType,
                                                                     units, showImportLog);
    if (!result.error.code.empty()) {
        return fail(result.error);
    }
    m_importRequest = result.value;
    m_importPending = true;
    m_stlImportPoll.start();
    emit importPendingChanged();
    return true;
}

bool ProjectViewModel::cancelImport() {
    if (!m_importPending || m_importRequest == 0) {
        return false;
    }
    if (!panta::ffi::project_service_cancel_stl_import(*m_service, m_importRequest)) {
        return false;
    }
    m_stlImportPoll.stop();
    m_importRequest = 0;
    m_importPending = false;
    emit importPendingChanged();
    return true;
}

void ProjectViewModel::finish_stl_import() {
    const auto request = m_importRequest;
    if (!m_importPending || request == 0) {
        m_stlImportPoll.stop();
        return;
    }
    const auto result = panta::ffi::project_service_finish_stl_import(*m_service, request);
    if (!result.ready) {
        return;
    }
    m_stlImportPoll.stop();
    m_importRequest = 0;
    m_importPending = false;
    bool succeeded = false;
    if (!result.error.code.empty()) {
        fail(result.error);
        cancelImportPreview();
    } else {
        succeeded = publish_import(result.value);
    }
    emit projectImportFinished(request, succeeded);
    emit importPendingChanged();
}

bool ProjectViewModel::publish_import(const panta::ffi::ProjectImport& imported) {
    const auto snapshot = panta::ffi::project_service_current(*m_service);
    if (!applySnapshot(snapshot) || !refreshImports()) {
        return false;
    }
    panta::ffi::project_service_workspace_publish_import(*m_service, imported.id,
                                                         imported.display_name);
    refresh_workspace();
    emit projectImported(m_importedAssetPath);
    return true;
}

void ProjectViewModel::cancelImportPreview() {
    m_importPreviewPoll.stop();
    if (m_previewRequest != 0) {
        panta::ffi::project_service_cancel_stl_preview(*m_service, m_previewRequest);
        m_previewRequest = 0;
    }
    const bool changed = m_importPreviewPending || m_importPreviewReady;
    m_importPreviewPending = false;
    m_importPreviewReady = false;
    m_importPreviewName.clear();
    m_importPreviewDimensions.clear();
    m_importPreviewTriangleCount = 0;
    m_importPreviewLargeMeshWarning = false;
    m_importPreviewExpandedBytes = 0;
    if (changed)
        emit importPreviewChanged();
}

bool ProjectViewModel::inspectStl(const QString& rawPath) {
    cancelImportPreview();
    clearError();
    std::string path;
    QString conversionError;
    if (!toBoundaryText(QDir::cleanPath(QDir::fromNativeSeparators(rawPath.trimmed())), &path,
                        &conversionError)) {
        return fail(conversionError);
    }
    const auto result = panta::ffi::project_service_begin_stl_preview(*m_service, path);
    if (!result.error.code.empty())
        return fail(result.error);
    m_previewRequest = result.value;
    m_importPreviewPending = true;
    m_importPreviewPoll.start();
    emit importPreviewChanged();
    return true;
}

void ProjectViewModel::finish_import_preview() {
    const auto request = m_previewRequest;
    const auto result = panta::ffi::project_service_finish_stl_preview(*m_service, request);
    if (!result.ready && result.error.code.empty())
        return;
    m_importPreviewPoll.stop();
    m_importPreviewPending = false;
    const bool succeeded = result.error.code.empty();
    if (succeeded) {
        m_importPreviewReady = true;
        m_importPreviewName = QString::fromUtf8(result.value.source_name);
        m_importPreviewDimensions = QString::fromUtf8(panta::ffi::format_project_dimensions(
            result.value.size_x, result.value.size_y, result.value.size_z, ""));
        m_importPreviewTriangleCount = result.value.triangle_count;
        m_importPreviewLargeMeshWarning = result.value.large_mesh_warning;
        m_importPreviewExpandedBytes = result.value.expanded_mesh_bytes;
    } else {
        fail(result.error);
    }
    // 错误或属性通知可重入并替换请求；旧完成事件不能再驱动新表单。
    if (m_previewRequest != request)
        return;
    emit importPreviewChanged();
    if (m_previewRequest == request)
        emit importPreviewFinished(request, succeeded);
}

bool ProjectViewModel::fail(const panta::ffi::ProjectDiagnostic& diagnostic) {
    return fail(QString::fromUtf8(diagnostic.code), QString::fromUtf8(diagnostic.category),
                QString::fromUtf8(diagnostic.detail));
}

bool ProjectViewModel::applySnapshot(const panta::ffi::ProjectSnapshotResult& result) {
    return result.error.code.empty() ? applySnapshot(result.value) : fail(result.error);
}

bool ProjectViewModel::fail(const QString& code, const QString& category, const QString& detail) {
    const QString message = project_error_message(code);
    if (m_errorCode != code || m_error != message || m_errorCategory != category ||
        m_errorDetail != detail) {
        m_errorCode = code;
        m_error = message;
        m_errorCategory = category;
        m_errorDetail = detail;
        emit errorChanged();
    }
    return false;
}

bool ProjectViewModel::applySnapshot(const panta::ffi::ProjectSnapshot& snapshot) {
    const QString path = QString::fromUtf8(snapshot.path);
    const QString name = QString::fromUtf8(snapshot.name);
    const bool changed =
        m_currentPath != path || m_currentName != name || m_dirty != snapshot.dirty;
    m_currentPath = path;
    m_currentName = name;
    m_dirty = snapshot.dirty;
    if (changed) {
        emit projectChanged();
    }
    return true;
}

void ProjectViewModel::applyImports(const rust::Vec<panta::ffi::ProjectImport>& imports) {
    QStringList nextImportedPartNames;
    QStringList nextPartIds;
    const QDir projectDirectory(QFileInfo(m_currentPath).absolutePath());
    for (std::size_t index = 0; index < imports.size(); ++index) {
        const auto& imported = imports[index];
        nextImportedPartNames.append(QString::fromUtf8(imported.display_name));
        nextPartIds.append(QString::fromUtf8(imported.id));
    }

    QString nextPartName;
    QString nextAssetPath;
    QString nextMeshType;
    QString nextUnits;
    QString nextDimensions;
    quint64 nextTriangleCount = 0;

    if (imports.size() > 0) {
        const auto& imported = imports[imports.size() - 1];
        nextPartName = QString::fromUtf8(imported.display_name);
        nextAssetPath = projectDirectory.filePath(QString::fromUtf8(imported.mesh_asset_path));
        nextMeshType = QString::fromUtf8(imported.mesh_type);
        nextUnits = QString::fromUtf8(imported.units);
        nextTriangleCount = imported.triangle_count;
        const QString unitLabel = nextUnits == QStringLiteral("millimeters") ? QStringLiteral("mm")
                                  : nextUnits == QStringLiteral("centimeters")
                                      ? QStringLiteral("cm")
                                      : QStringLiteral("in");
        nextDimensions = QString::fromUtf8(panta::ffi::format_project_dimensions(
            imported.size_x, imported.size_y, imported.size_z, unitLabel.toStdString()));
    }

    const bool changed =
        m_importedPartNames != nextImportedPartNames || m_importedPartIds != nextPartIds ||
        m_importedPartName != nextPartName || m_importedAssetPath != nextAssetPath ||
        m_importedMeshType != nextMeshType || m_importedUnits != nextUnits ||
        m_importedDimensions != nextDimensions || m_importedTriangleCount != nextTriangleCount;
    m_importedPartNames = std::move(nextImportedPartNames);
    m_importedPartIds = std::move(nextPartIds);
    m_importedPartName = nextPartName;
    m_importedAssetPath = nextAssetPath;
    m_importedMeshType = nextMeshType;
    m_importedUnits = nextUnits;
    m_importedDimensions = nextDimensions;
    m_importedTriangleCount = nextTriangleCount;
    if (changed) {
        emit importsChanged();
    }
}

bool ProjectViewModel::refreshImports() {
    const auto result = panta::ffi::project_service_imports(*m_service);
    if (!result.error.code.empty()) {
        return fail(result.error);
    }
    applyImports(result.value);
    // 网格内容只随文档（激活 / 快照变化）改变：由文档路径负责发射
    // meshChanged，避免与导入激活的发射重复。
    return true;
}

bool ProjectViewModel::toBoundaryText(const QString& text, std::string* out, QString* error) {
    return to_boundary_utf8(text, out, error);
}

QVariantList ProjectViewModel::openDocuments() const { return m_openDocuments; }

QString ProjectViewModel::activeDocumentId() const { return m_activeDocumentId; }

QString ProjectViewModel::activeDocumentTitle() const {
    for (const auto& item : m_openDocuments) {
        const auto document = item.toMap();
        if (document.value(QStringLiteral("id")).toString() == m_activeDocumentId) {
            return document.value(QStringLiteral("title")).toString();
        }
    }
    return QString{};
}

QStringList ProjectViewModel::importedPartIds() const { return m_importedPartIds; }

void ProjectViewModel::reset_documents() {
    panta::ffi::project_service_workspace_reset(*m_service);
    refresh_workspace(true);
}

void ProjectViewModel::refresh_workspace(bool forceActiveNotification) {
    const auto snapshot = panta::ffi::project_service_workspace_snapshot(*m_service);
    QVariantList nextDocuments;
    nextDocuments.reserve(static_cast<qsizetype>(snapshot.documents.size()));
    for (const auto& document : snapshot.documents) {
        QVariantMap diagnostic;
        if (!document.diagnostic_code.empty()) {
            diagnostic = {
                {QStringLiteral("code"), QString::fromUtf8(document.diagnostic_code)},
                {QStringLiteral("category"), QString::fromUtf8(document.diagnostic_category)},
                {QStringLiteral("detail"), QString::fromUtf8(document.diagnostic_detail)}};
        }
        nextDocuments.append(
            QVariantMap{{QStringLiteral("id"), QString::fromUtf8(document.id)},
                        {QStringLiteral("kind"), QString::fromUtf8(document.kind)},
                        {QStringLiteral("state"), QString::fromUtf8(document.state)},
                        {QStringLiteral("title"), QString::fromUtf8(document.title)},
                        {QStringLiteral("message"), QString::fromUtf8(document.message)},
                        {QStringLiteral("diagnostic"), diagnostic},
                        {QStringLiteral("displayOrdinal"),
                         QVariant::fromValue<qulonglong>(document.display_ordinal)},
                        {QStringLiteral("displayCount"),
                         QVariant::fromValue<qulonglong>(document.display_count)}});
    }

    const bool documentsChangedValue = m_openDocuments != nextDocuments;
    const QString nextActiveId = QString::fromUtf8(snapshot.active_document_id);
    const bool activeChanged = forceActiveNotification || m_activeDocumentId != nextActiveId;
    m_openDocuments = std::move(nextDocuments);
    if (activeChanged) {
        m_activeDocumentId = nextActiveId;
        ++m_meshSelection;
        m_activeMesh.reset();
        prepareActiveMesh();
        emit activeDocumentChanged();
        emit meshChanged();
    }
    if (documentsChangedValue) {
        emit documentsChanged();
    }
    sync_activation_poll();
}

void ProjectViewModel::activateDocument(const QString& documentId) {
    panta::ffi::project_service_workspace_activate(*m_service, documentId.toStdString());
    refresh_workspace(!m_activeMesh);
}

void ProjectViewModel::closeDocument(const QString& documentId) {
    panta::ffi::project_service_workspace_close(*m_service, documentId.toStdString());
    refresh_workspace();
}

void ProjectViewModel::openImportRecord(const QString& recordId) {
    const qsizetype index = m_importedPartIds.indexOf(recordId);
    if (index < 0) {
        return;
    }
    panta::ffi::project_service_workspace_open_import(
        *m_service, recordId.toStdString(), m_importedPartNames.value(index).toStdString());
    refresh_workspace();
}

void ProjectViewModel::moveDocument(int fromIndex, int toIndex) {
    panta::ffi::project_service_workspace_move(*m_service, static_cast<std::uint64_t>(fromIndex),
                                               static_cast<std::uint64_t>(toIndex));
    refresh_workspace();
}

void ProjectViewModel::drain_activations() {
    panta::ffi::project_service_drain_workspace_activations(*m_service);
    refresh_workspace();
}

void ProjectViewModel::sync_activation_poll() {
    const bool loading =
        std::any_of(m_openDocuments.cbegin(), m_openDocuments.cend(), [](const QVariant& item) {
            return item.toMap().value(QStringLiteral("state")) == QStringLiteral("loading");
        });
    if (loading && !m_activationPoll.isActive()) {
        m_activationPoll.start();
    } else if (!loading && m_activationPoll.isActive()) {
        m_activationPoll.stop();
    }
}

} // namespace panta::bridge
