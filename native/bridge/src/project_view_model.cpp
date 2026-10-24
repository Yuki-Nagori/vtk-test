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
#include <QSettings>
#include <QString>
#include <QUrl>
#include <QVariantMap>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qnamespace.h>
#include <QtCore/qtmetamacros.h>
#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <exception>
#include <filesystem>
#include <functional>
#include <future>
#include <memory>
#include <optional>
#include <panta/geometry/step_import.hpp>
#include <panta/mesh/netgen_mesher.hpp>
#include <panta/mesh/rust_dto.hpp>
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
    m_taskProgressPoll.setInterval(100);
    connect(&m_taskProgressPoll, &QTimer::timeout, this, [this] {
        emit taskProgressChanged();
        if (!m_importPending && !m_importPreviewPending && !m_exportProgress &&
            !m_diagnosticsProgress && !m_generationProgress)
            m_taskProgressPoll.stop();
    });
    m_animationPoll.setInterval(10);
    connect(&m_animationPoll, &QTimer::timeout, this, &ProjectViewModel::tick_animation);
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
        quint64 lodSourceTriangles = 0;
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
            // 序列播放期间视口由动画帧占用；关闭序列时重新准备活动网格。
            if (m_animation)
                return;
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
            m_activeMeshTriangles = prepared->mesh ? prepared->mesh->vertices.size() / 3 : 0;
            m_activeLodSourceTriangles = prepared->lodSourceTriangles;
            m_activeMesh = std::move(prepared->mesh);
            refresh_volume();
            emit meshChanged();
            if (m_sectionActive && m_volumeAvailable)
                request_section();
        },
        Qt::QueuedConnection);
    try {
        startBackgroundWaiter([ready, result] {
            auto prepared = std::make_shared<Prepared>();
            auto source = panta::ffi::prepare_mesh_display(ready->get());
            prepared->error = std::move(source.error);
            prepared->lodSourceTriangles = source.value.lod_source_triangles;
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

bool ProjectViewModel::beginExportActiveMeshUrl(const QUrl& url) {
    clearError();
    if (m_exportPending)
        return fail(QStringLiteral("project.command_invalid"), QStringLiteral("state"),
                    QStringLiteral("export already pending"));
    const QString path = localPath(url);
    if (path.isEmpty())
        return fail(QStringLiteral("project.export_write_failed"), QStringLiteral("io"),
                    url.toString());
    std::string boundaryPath;
    QString conversionError;
    if (!toBoundaryText(path, &boundaryPath, &conversionError))
        return fail(conversionError);
    if (m_activeDocumentId.isEmpty())
        return fail(QStringLiteral("project.mesh_not_resident"), QStringLiteral("state"));
    // 体网格文档导出 .vtu 时由 Rust 写四面体单元（task 022 后台化）；其他格式导出外表面。
    using Input = rust::Box<panta::ffi::MeshDisplayInput>;
    using Result = std::shared_ptr<panta::ffi::ProjectDiagnostic>;
    using Watcher = QFutureWatcher<Result>;
    // 共享快照在 GUI 线程捕获（只增加引用计数），写盘与编码在后台线程完成。
    const auto input =
        std::make_shared<std::optional<Input>>(panta::ffi::project_service_mesh_display_input(
            *m_service, m_activeDocumentId.toStdString()));
    auto hostProgress = panta::ffi::task_progress_new();
    const auto progress = std::make_shared<rust::Box<panta::ffi::TaskProgress>>(
        panta::ffi::task_progress_share(*hostProgress));
    const auto result = std::make_shared<QPromise<Result>>();
    auto watcher = std::make_unique<Watcher>();
    result->start();
    watcher->setFuture(result->future());
    connect(watcher.get(), &Watcher::finished, watcher.get(), &QObject::deleteLater);
    connect(
        watcher.get(), &Watcher::finished, this,
        [this, future = result->future(), path] {
            m_exportPending = false;
            m_exportProgress.reset();
            emit taskProgressChanged();
            emit exportPendingChanged();
            const auto diagnostic = future.result();
            const bool succeeded = diagnostic && diagnostic->code.empty();
            if (!succeeded && diagnostic)
                fail(*diagnostic);
            emit exportFinished(path, succeeded);
        },
        Qt::QueuedConnection);
    try {
        startBackgroundWaiter([input, result, boundaryPath, progress] {
            auto diagnostic =
                std::make_shared<panta::ffi::ProjectDiagnostic>(panta::ffi::export_mesh_display(
                    std::move(input->value()), boundaryPath, **progress));
            input->reset();
            result->addResult(diagnostic);
            result->finish();
        });
    } catch (const std::system_error& error) {
        return fail(QStringLiteral("project.io"), QStringLiteral("io"),
                    QString::fromUtf8(error.what()));
    }
    watcher.release()->setParent(this);
    m_exportPending = true;
    m_exportProgress.emplace(std::move(hostProgress));
    m_taskProgressPoll.start();
    emit exportPendingChanged();
    emit taskProgressChanged();
    return true;
}

bool ProjectViewModel::beginDiagnoseActiveMesh() {
    clearError();
    if (m_diagnosticsPending)
        return fail(QStringLiteral("project.command_invalid"), QStringLiteral("state"),
                    QStringLiteral("diagnostics already pending"));
    if (m_activeDocumentId.isEmpty())
        return fail(QStringLiteral("project.mesh_not_resident"), QStringLiteral("state"));
    using Input = rust::Box<panta::ffi::MeshDisplayInput>;
    using Result = std::shared_ptr<panta::ffi::MeshQualityReportResult>;
    using Watcher = QFutureWatcher<Result>;
    const QString documentId = m_activeDocumentId;
    const auto input = std::make_shared<std::optional<Input>>(
        panta::ffi::project_service_mesh_display_input(*m_service, documentId.toStdString()));
    auto hostProgress = panta::ffi::task_progress_new();
    const auto progress = std::make_shared<rust::Box<panta::ffi::TaskProgress>>(
        panta::ffi::task_progress_share(*hostProgress));
    const auto result = std::make_shared<QPromise<Result>>();
    auto watcher = std::make_unique<Watcher>();
    result->start();
    watcher->setFuture(result->future());
    connect(watcher.get(), &Watcher::finished, watcher.get(), &QObject::deleteLater);
    connect(
        watcher.get(), &Watcher::finished, this,
        [this, future = result->future(), documentId] {
            m_diagnosticsPending = false;
            m_diagnosticsProgress.reset();
            emit taskProgressChanged();
            const auto report = future.result();
            const bool succeeded = report && report->error.code.empty();
            if (succeeded) {
                const auto& value = report->value;
                const auto count = [](std::uint64_t number) { return QVariant::fromValue(number); };
                m_meshQuality = QVariantMap{
                    {QStringLiteral("documentId"), documentId},
                    {QStringLiteral("clean"), value.clean},
                    {QStringLiteral("weldTolerance"), value.weld_tolerance},
                    {QStringLiteral("triangleCount"), count(value.triangle_count)},
                    {QStringLiteral("rawVertexCount"), count(value.raw_vertex_count)},
                    {QStringLiteral("weldedVertexCount"), count(value.welded_vertex_count)},
                    {QStringLiteral("nearDuplicateVertexCount"),
                     count(value.near_duplicate_vertex_count)},
                    {QStringLiteral("degenerateTriangleCount"),
                     count(value.degenerate_triangle_count)},
                    {QStringLiteral("boundaryEdgeCount"), count(value.boundary_edge_count)},
                    {QStringLiteral("holeCount"), count(value.hole_count)},
                    {QStringLiteral("nonManifoldEdgeCount"), count(value.non_manifold_edge_count)},
                    {QStringLiteral("inconsistentOrientationEdgeCount"),
                     count(value.inconsistent_orientation_edge_count)},
                    {QStringLiteral("volumeAvailable"), value.volume_available},
                };
                if (value.volume_available) {
                    m_meshQuality.insert(QStringLiteral("nodeCount"), count(value.node_count));
                    m_meshQuality.insert(QStringLiteral("tetCount"), count(value.tet_count));
                    m_meshQuality.insert(QStringLiteral("minDihedralDegrees"),
                                         value.min_dihedral_degrees);
                    m_meshQuality.insert(QStringLiteral("maxAspectRatio"), value.max_aspect_ratio);
                    m_meshQuality.insert(QStringLiteral("meanAspectRatio"),
                                         value.mean_aspect_ratio);
                    m_meshQuality.insert(QStringLiteral("poorTetCount"), count(value.poor_count));
                }
            } else if (report) {
                m_meshQuality.clear();
                fail(report->error);
            }
            emit meshQualityChanged();
            emit diagnosticsFinished(succeeded);
        },
        Qt::QueuedConnection);
    try {
        startBackgroundWaiter([input, result, progress] {
            auto report = std::make_shared<panta::ffi::MeshQualityReportResult>(
                panta::ffi::diagnose_mesh_display(std::move(input->value()), **progress));
            input->reset();
            result->addResult(report);
            result->finish();
        });
    } catch (const std::system_error& error) {
        return fail(QStringLiteral("project.io"), QStringLiteral("io"),
                    QString::fromUtf8(error.what()));
    }
    watcher.release()->setParent(this);
    m_diagnosticsPending = true;
    m_diagnosticsProgress.emplace(std::move(hostProgress));
    m_taskProgressPoll.start();
    emit meshQualityChanged();
    emit taskProgressChanged();
    return true;
}

double ProjectViewModel::importProgress() const {
    if (!m_importPending)
        return -1.0;
    return panta::ffi::project_service_stl_import_progress(*m_service, m_importRequest);
}

double ProjectViewModel::exportProgress() const {
    return m_exportProgress ? panta::ffi::task_progress_fraction(**m_exportProgress) : -1.0;
}

double ProjectViewModel::diagnosticsProgress() const {
    return m_diagnosticsProgress ? panta::ffi::task_progress_fraction(**m_diagnosticsProgress)
                                 : -1.0;
}

bool ProjectViewModel::cancelExport() {
    if (!m_exportProgress)
        return false;
    panta::ffi::task_progress_cancel(**m_exportProgress);
    return true;
}

bool ProjectViewModel::cancelDiagnostics() {
    if (!m_diagnosticsProgress)
        return false;
    panta::ffi::task_progress_cancel(**m_diagnosticsProgress);
    return true;
}

namespace {

/// 与 Rust begin_generated_volume 的参数上限一致；在启动昂贵生成前先拒绝。
constexpr double kMaxGenerationLengthMm = 1.0e6;

/// 后台生成的结果：失败携带稳定诊断码，成功携带可直接交给 Rust 的 DTO。
struct GenerationOutcome {
    QString code;
    QString category;
    QString detail;
    std::optional<panta::ffi::TetMeshData> mesh;
};

GenerationOutcome generation_failure(const QString& category, const std::string& detail) {
    return {QStringLiteral("project.volume_generation_failed"), category,
            QString::fromStdString(detail), std::nullopt};
}

GenerationOutcome generate_volume_from_step(const std::filesystem::path& file, double maxLengthMm,
                                            const panta::ffi::TaskProgress& progress) {
    const auto imported = panta::geometry::import_step(file);
    if (imported.status != panta::geometry::StepImportStatus::kNone) {
        if (imported.shape.id != 0)
            panta::geometry::release_shape(imported.shape);
        return generation_failure(QStringLiteral("format"), "STEP import: " + imported.detail);
    }
    panta::mesh::TetMeshGenerationParameters parameters;
    parameters.max_length_mm = maxLengthMm;
    parameters.on_progress = [&progress](double value) {
        panta::ffi::task_progress_set_fraction(progress, value);
    };
    parameters.is_cancelled = [&progress] {
        return panta::ffi::task_progress_is_cancelled(progress);
    };
    const auto generated = panta::mesh::generate_tet_mesh(imported.shape, parameters);
    panta::geometry::release_shape(imported.shape);
    if (generated.status == panta::mesh::TetMeshGenerationStatus::kCancelled)
        return {QStringLiteral("project.volume_generation_cancelled"),
                QStringLiteral("state"),
                {},
                std::nullopt};
    if (generated.status != panta::mesh::TetMeshGenerationStatus::kNone)
        return generation_failure(QStringLiteral("format"), "Netgen: " + generated.detail);
    return {{}, {}, {}, panta::mesh::to_rust_tet_mesh_data(generated.mesh)};
}

} // namespace

bool ProjectViewModel::beginGenerateVolumeFromStep(const QString& rawPath, double maxLengthMm) {
    clearError();
    if (m_generationPending || m_importPending)
        return fail(QStringLiteral("project.command_invalid"), QStringLiteral("state"),
                    QStringLiteral("another import or generation is pending"));
    if (m_currentPath.isEmpty())
        return fail(QStringLiteral("project.no_project"), QStringLiteral("state"));
    if (!std::isfinite(maxLengthMm) || maxLengthMm < 0.0 || maxLengthMm > kMaxGenerationLengthMm)
        return fail(QStringLiteral("project.volume_generation_failed"),
                    QStringLiteral("validation"),
                    QStringLiteral("max length %1 mm outside 0..=%2")
                        .arg(maxLengthMm)
                        .arg(kMaxGenerationLengthMm));
    std::string path;
    QString conversionError;
    const QString cleanPath = QDir::cleanPath(QDir::fromNativeSeparators(rawPath.trimmed()));
    if (cleanPath.isEmpty() || !toBoundaryText(cleanPath, &path, &conversionError))
        return fail(conversionError.isEmpty() ? QStringLiteral("project.import_invalid_file")
                                              : conversionError);
    const QFileInfo sourceInfo(cleanPath);
    if (!sourceInfo.isFile())
        return fail(QStringLiteral("project.import_file_missing"), QStringLiteral("missing"),
                    cleanPath);
    // 平台原生编码的路径给 OCCT；UTF-8 文本给 Rust 工程记录。
    const std::filesystem::path sourceFile = sourceInfo.filesystemFilePath();

    using Result = std::shared_ptr<GenerationOutcome>;
    using Watcher = QFutureWatcher<Result>;
    auto hostProgress = panta::ffi::task_progress_new();
    const auto progress = std::make_shared<rust::Box<panta::ffi::TaskProgress>>(
        panta::ffi::task_progress_share(*hostProgress));
    const auto result = std::make_shared<QPromise<Result>>();
    auto watcher = std::make_unique<Watcher>();
    result->start();
    watcher->setFuture(result->future());
    connect(watcher.get(), &Watcher::finished, watcher.get(), &QObject::deleteLater);
    const QString projectPath = m_currentPath;
    connect(
        watcher.get(), &Watcher::finished, this,
        [this, future = result->future(), path, maxLengthMm, projectPath] {
            m_generationProgress.reset();
            emit taskProgressChanged();
            const auto outcome = future.result();
            if (!outcome || !outcome->mesh) {
                if (outcome)
                    fail(outcome->code, outcome->category, outcome->detail);
                finish_generation(false);
                return;
            }
            // 生成期间切换或关闭了工程：结果不写入另一个工程（保守丢弃）。
            if (m_currentPath != projectPath) {
                fail(QStringLiteral("project.command_invalid"), QStringLiteral("state"),
                     QStringLiteral("project changed during volume generation"));
                finish_generation(false);
                return;
            }
            const auto begun = panta::ffi::project_service_begin_generated_volume(
                *m_service, path, maxLengthMm, std::move(*outcome->mesh));
            outcome->mesh.reset();
            if (!begun.error.code.empty()) {
                fail(begun.error);
                finish_generation(false);
                return;
            }
            m_importRequest = begun.value;
            m_importPending = true;
            m_stlImportPoll.start();
            m_taskProgressPoll.start();
            emit importPendingChanged();
        },
        Qt::QueuedConnection);
    try {
        startBackgroundWaiter([sourceFile, maxLengthMm, result, progress] {
            Result outcome;
            try {
                outcome = std::make_shared<GenerationOutcome>(
                    generate_volume_from_step(sourceFile, maxLengthMm, **progress));
            } catch (const std::exception& error) {
                outcome = std::make_shared<GenerationOutcome>(
                    generation_failure(QStringLiteral("io"), error.what()));
            }
            result->addResult(outcome);
            result->finish();
        });
    } catch (const std::system_error& error) {
        return fail(QStringLiteral("project.io"), QStringLiteral("io"),
                    QString::fromUtf8(error.what()));
    }
    watcher.release()->setParent(this);
    m_generationPending = true;
    m_generationProgress.emplace(std::move(hostProgress));
    m_taskProgressPoll.start();
    emit generationPendingChanged();
    emit taskProgressChanged();
    return true;
}

bool ProjectViewModel::cancelGeneration() {
    if (!m_generationPending)
        return false;
    if (m_generationProgress) {
        panta::ffi::task_progress_cancel(**m_generationProgress);
        return true;
    }
    // 提交阶段：取消导入请求后同步收尾（cancelImport 不发 projectImportFinished）。
    if (!cancelImport())
        return false;
    fail(QStringLiteral("project.volume_generation_cancelled"), QStringLiteral("state"));
    finish_generation(false);
    return true;
}

double ProjectViewModel::generationProgress() const {
    if (!m_generationPending)
        return -1.0;
    if (m_generationProgress)
        return 0.9 * panta::ffi::task_progress_fraction(**m_generationProgress);
    const double commit = importProgress();
    return commit < 0.0 ? 0.9 : 0.9 + 0.1 * commit;
}

void ProjectViewModel::finish_generation(bool succeeded) {
    if (!m_generationPending)
        return;
    m_generationPending = false;
    emit generationPendingChanged();
    emit generationFinished(succeeded);
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
    m_taskProgressPoll.start();
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
    finish_generation(succeeded);
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
    m_taskProgressPoll.start();
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

QString ProjectViewModel::recentProjectsFile() {
    // 独立 ini 文件，避免依赖 QCoreApplication 组织名；测试启用 QStandardPaths 测试模式隔离。
    const QString directory =
        panta::qt_adapter::standard_location(panta::qt_adapter::StandardLocation::UserConfig);
    return directory.isEmpty() ? QString()
                               : QDir(directory).filePath(QStringLiteral("recent-projects.ini"));
}

QStringList ProjectViewModel::recentProjects() const {
    const QString file = recentProjectsFile();
    if (file.isEmpty())
        return {};
    const QSettings settings(file, QSettings::IniFormat);
    QStringList result;
    for (const QString& path : settings.value(QStringLiteral("recent/paths")).toStringList()) {
        if (!path.isEmpty() && QFileInfo::exists(path) && !result.contains(path))
            result.append(path);
    }
    return result;
}

void ProjectViewModel::rememberRecentProject(const QString& path) {
    const QString file = recentProjectsFile();
    if (file.isEmpty())
        return;
    const QString absolute = QFileInfo(path).absoluteFilePath();
    QStringList paths = recentProjects();
    paths.removeAll(absolute);
    paths.prepend(absolute);
    while (paths.size() > kMaxRecentProjects)
        paths.removeLast();
    QSettings settings(file, QSettings::IniFormat);
    settings.setValue(QStringLiteral("recent/paths"), paths);
    settings.sync();
    emit recentProjectsChanged();
}

void ProjectViewModel::clearRecentProjects() {
    const QString file = recentProjectsFile();
    if (file.isEmpty())
        return;
    QSettings settings(file, QSettings::IniFormat);
    settings.remove(QStringLiteral("recent"));
    settings.sync();
    emit recentProjectsChanged();
}

QString ProjectViewModel::classifyDroppedUrl(const QUrl& url) const {
    if (!url.isLocalFile())
        return {};
    const QString suffix = QFileInfo(url.toLocalFile()).suffix().toLower();
    if (suffix == QLatin1String("panta"))
        return QStringLiteral("project");
    static const QStringList mesh_suffixes = {QStringLiteral("stl"), QStringLiteral("vtu"),
                                              QStringLiteral("vtp"), QStringLiteral("vtk"),
                                              QStringLiteral("obj"), QStringLiteral("ply")};
    return mesh_suffixes.contains(suffix) ? QStringLiteral("mesh") : QString();
}

bool ProjectViewModel::applySnapshot(const panta::ffi::ProjectSnapshot& snapshot) {
    const QString path = QString::fromUtf8(snapshot.path);
    const QString name = QString::fromUtf8(snapshot.name);
    const bool changed =
        m_currentPath != path || m_currentName != name || m_dirty != snapshot.dirty;
    const bool path_changed = m_currentPath != path;
    m_currentPath = path;
    m_currentName = name;
    m_dirty = snapshot.dirty;
    if (path_changed && !path.isEmpty())
        rememberRecentProject(path);
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
        m_activeLodSourceTriangles = 0;
        m_volumeAvailable = false;
        m_volumeInfo.clear();
        m_sectionActive = false;
        m_sectionDirty = false;
        if (m_sectionProgress)
            panta::ffi::task_progress_cancel(**m_sectionProgress);
        emit volumeChanged();
        if (m_animation) {
            // 切换文档即退出序列查看（保守：序列不与文档并存）。
            m_animation.reset();
            m_animationPoll.stop();
            m_animationPlaying = false;
            emit animationChanged();
        }
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

QString panta::bridge::ProjectViewModel::activeMeshLodStatus() const {
    if (!m_activeMesh || m_activeLodSourceTriangles == 0)
        return {};
    return QStringLiteral(
               "大网格：显示简化 LOD %1 面 / 源 %2 面；拾取坐标为近似值，诊断与导出不可用")
        .arg(m_activeMeshTriangles)
        .arg(m_activeLodSourceTriangles);
}

double panta::bridge::ProjectViewModel::importPreviewProgress() const {
    if (!m_importPreviewPending || m_previewRequest == 0)
        return -1.0;
    return panta::ffi::project_service_stl_preview_progress(*m_service, m_previewRequest);
}

bool panta::bridge::ProjectViewModel::openAnimationUrl(const QUrl& url) {
    clearError();
    std::string path;
    QString conversionError;
    const QString local = url.isLocalFile() ? url.toLocalFile() : url.toString();
    if (!toBoundaryText(QDir::cleanPath(QDir::fromNativeSeparators(local)), &path,
                        &conversionError))
        return fail(conversionError);
    panta::ffi::ProjectDiagnostic error;
    auto sequence = panta::ffi::animation_open(path, error);
    if (!error.code.empty())
        return fail(error);
    if (m_sectionActive) {
        m_sectionActive = false;
        emit volumeChanged();
    }
    m_animation.emplace(std::move(sequence));
    m_animationFrameCount = static_cast<int>(panta::ffi::animation_frame_count(**m_animation));
    m_animationTarget = 0;
    m_animationDisplayed = -1;
    m_animationPlaying = false;
    m_animationFrameError.clear();
    panta::ffi::animation_request(**m_animation, 0);
    m_animationClock.start();
    m_animationPoll.start();
    emit animationChanged();
    return true;
}

void panta::bridge::ProjectViewModel::closeAnimation() {
    if (!m_animation)
        return;
    m_animation.reset();
    m_animationPoll.stop();
    m_animationPlaying = false;
    m_animationFrameCount = 0;
    m_animationDisplayed = -1;
    m_activeMesh.reset();
    emit animationChanged();
    emit meshChanged();
    prepareActiveMesh();
}

void panta::bridge::ProjectViewModel::setAnimationFrame(int frame) {
    if (!m_animation || m_animationFrameCount <= 0)
        return;
    m_animationTarget = std::clamp(frame, 0, m_animationFrameCount - 1);
    panta::ffi::animation_request(**m_animation, static_cast<std::uint64_t>(m_animationTarget));
}

void panta::bridge::ProjectViewModel::stepAnimation(int delta) {
    if (!m_animation || m_animationFrameCount <= 0)
        return;
    const int base = m_animationDisplayed < 0 ? 0 : m_animationDisplayed;
    setAnimationFrame(((base + delta) % m_animationFrameCount + m_animationFrameCount) %
                      m_animationFrameCount);
}

void panta::bridge::ProjectViewModel::setAnimationPlaying(bool playing) {
    if (!m_animation || m_animationPlaying == playing)
        return;
    m_animationPlaying = playing;
    m_animationClock.restart();
    emit animationChanged();
}

void panta::bridge::ProjectViewModel::setAnimationFps(int fps) {
    const int clamped = std::clamp(fps, 1, 60);
    if (clamped == m_animationFps)
        return;
    m_animationFps = clamped;
    emit animationChanged();
}

QString panta::bridge::ProjectViewModel::animationStatus() const {
    if (!m_animation)
        return {};
    const auto frame = m_animationDisplayed < 0 ? 0 : m_animationDisplayed;
    QString text = QStringLiteral("帧 %1 / %2：%3（已缓存 %4 帧）")
                       .arg(frame + 1)
                       .arg(m_animationFrameCount)
                       .arg(QString::fromUtf8(panta::ffi::animation_frame_name(
                           **m_animation, static_cast<std::uint64_t>(frame))))
                       .arg(panta::ffi::animation_cached_frames(**m_animation));
    if (!m_animationFrameError.isEmpty())
        text += QStringLiteral("；") + m_animationFrameError;
    return text;
}

// 10ms 轮询：播放时按帧率推进目标帧；目标帧未就绪则保持上一帧等待，不阻塞 GUI。
void panta::bridge::ProjectViewModel::tick_animation() {
    if (!m_animation || m_animationFrameCount <= 0)
        return;
    if (m_animationPlaying && m_animationDisplayed == m_animationTarget &&
        m_animationClock.elapsed() >= 1000 / m_animationFps) {
        setAnimationFrame((m_animationTarget + 1) % m_animationFrameCount);
        m_animationClock.restart();
    }
    if (m_animationDisplayed == m_animationTarget)
        return;
    auto frame =
        panta::ffi::animation_frame(**m_animation, static_cast<std::uint64_t>(m_animationTarget));
    if (!frame.error.code.empty()) {
        // 坏帧不替换视口，只记录诊断并视为已到达，播放继续。
        m_animationFrameError =
            QString::fromUtf8(panta::ffi::project_error_message(frame.error.code));
        m_animationDisplayed = m_animationTarget;
        emit animationChanged();
        return;
    }
    if (!frame.ready)
        return;
    try {
        auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
        const auto current = panta::ffi::project_service_current(*m_service);
        mesh->project_revision = current.value.revision;
        mesh->vertices.reserve(frame.coordinates.size() / 3);
        for (std::size_t offset = 0; offset + 2 < frame.coordinates.size(); offset += 3)
            mesh->vertices.push_back({frame.coordinates[offset], frame.coordinates[offset + 1],
                                      frame.coordinates[offset + 2]});
        m_activeMeshTriangles = mesh->vertices.size() / 3;
        m_activeLodSourceTriangles = 0;
        m_activeMesh = std::move(mesh);
    } catch (const std::exception& error) {
        fail(QStringLiteral("project.mesh_allocation_failed"), QStringLiteral("resource"),
             QString::fromUtf8(error.what()));
        setAnimationPlaying(false);
        return;
    }
    m_animationFrameError.clear();
    m_animationDisplayed = m_animationTarget;
    emit meshChanged();
    emit animationChanged();
}

void panta::bridge::ProjectViewModel::refresh_volume() {
    // 只读计数摘要；质量统计在后台诊断中计算并显示在诊断面板（task 022）。
    const auto info =
        panta::ffi::project_service_volume_summary(*m_service, m_activeDocumentId.toStdString());
    m_volumeAvailable = info.available;
    m_volumeInfo = info.available ? QStringLiteral("体网格：%1 节点 / %2 四面体\n"
                                                   "质量统计见“诊断网格”结果")
                                        .arg(info.node_count)
                                        .arg(info.tet_count)
                                  : QString();
    if (!m_volumeAvailable)
        m_sectionActive = false;
    emit volumeChanged();
}

void panta::bridge::ProjectViewModel::request_section() {
    if (m_sectionPending) {
        // 合并为最新意图：取消在途任务，完成回调里按当前参数重跑。
        m_sectionDirty = true;
        if (m_sectionProgress)
            panta::ffi::task_progress_cancel(**m_sectionProgress);
        return;
    }
    if (!m_sectionActive || !m_volumeAvailable || m_activeDocumentId.isEmpty())
        return;
    using Input = rust::Box<panta::ffi::MeshDisplayInput>;
    struct Section {
        panta::ffi::AnimationFrameResult frame;
    };
    using Result = std::shared_ptr<Section>;
    using Watcher = QFutureWatcher<Result>;
    const auto input =
        std::make_shared<std::optional<Input>>(panta::ffi::project_service_mesh_display_input(
            *m_service, m_activeDocumentId.toStdString()));
    auto hostProgress = panta::ffi::task_progress_new();
    const auto progress = std::make_shared<rust::Box<panta::ffi::TaskProgress>>(
        panta::ffi::task_progress_share(*hostProgress));
    const auto normal = m_sectionNormal;
    const double fraction = m_sectionFraction;
    const auto selection = m_meshSelection;
    const auto current = panta::ffi::project_service_current(*m_service);
    if (!current.error.code.empty())
        return;
    const auto revision = current.value.revision;
    const auto result = std::make_shared<QPromise<Result>>();
    auto watcher = std::make_unique<Watcher>();
    result->start();
    watcher->setFuture(result->future());
    connect(watcher.get(), &Watcher::finished, watcher.get(), &QObject::deleteLater);
    connect(
        watcher.get(), &Watcher::finished, this,
        [this, future = result->future(), selection, revision] {
            m_sectionPending = false;
            m_sectionProgress.reset();
            const auto section = future.result();
            if (m_sectionDirty) {
                m_sectionDirty = false;
                request_section();
                return;
            }
            // 结果已过期（切换文档、关闭切面或进入动画）时丢弃。
            if (!section || selection != m_meshSelection || !m_sectionActive || m_animation)
                return;
            if (!section->frame.error.code.empty()) {
                fail(section->frame.error);
                return;
            }
            if (!section->frame.ready)
                return;
            try {
                auto mesh = std::make_shared<panta::visualization::SurfaceMeshSnapshot>();
                mesh->project_revision = revision;
                const auto& coordinates = section->frame.coordinates;
                mesh->vertices.reserve(coordinates.size() / 3);
                for (std::size_t offset = 0; offset + 2 < coordinates.size(); offset += 3)
                    mesh->vertices.push_back(
                        {coordinates[offset], coordinates[offset + 1], coordinates[offset + 2]});
                m_activeMeshTriangles = mesh->vertices.size() / 3;
                m_activeLodSourceTriangles = 0;
                m_activeMesh = std::move(mesh);
            } catch (const std::exception& error) {
                fail(QStringLiteral("project.mesh_allocation_failed"), QStringLiteral("resource"),
                     QString::fromUtf8(error.what()));
                return;
            }
            emit meshChanged();
        },
        Qt::QueuedConnection);
    try {
        startBackgroundWaiter([input, result, progress, normal, fraction] {
            auto section = std::make_shared<Section>();
            section->frame = panta::ffi::section_mesh_display(
                std::move(input->value()), normal[0], normal[1], normal[2], fraction, **progress);
            input->reset();
            result->addResult(section);
            result->finish();
        });
    } catch (const std::system_error& error) {
        fail(QStringLiteral("project.io"), QStringLiteral("io"), QString::fromUtf8(error.what()));
        return;
    }
    watcher.release()->setParent(this);
    m_sectionPending = true;
    m_sectionProgress.emplace(std::move(hostProgress));
}

bool panta::bridge::ProjectViewModel::setSection(bool enabled, double normalX, double normalY,
                                                 double normalZ, double fraction) {
    clearError();
    if (enabled && (!m_volumeAvailable || m_animation))
        return fail(QStringLiteral("project.volume_unavailable"), QStringLiteral("state"),
                    m_activeDocumentId);
    const double length = std::sqrt(normalX * normalX + normalY * normalY + normalZ * normalZ);
    const bool planeValid = std::isfinite(length) && length > 0.0 && std::isfinite(fraction);
    if (enabled && !planeValid)
        return fail(QStringLiteral("project.command_invalid"), QStringLiteral("state"),
                    QStringLiteral("section normal or position is invalid"));
    const bool wasActive = m_sectionActive;
    // 关闭切面时允许任意参数，只在平面有效时保留，供下次启用沿用。
    if (planeValid) {
        m_sectionNormal = {normalX, normalY, normalZ};
        m_sectionFraction = std::clamp(fraction, 0.0, 1.0);
    }
    m_sectionActive = enabled;
    emit volumeChanged();
    if (enabled) {
        request_section();
        return true;
    }
    if (m_sectionPending && m_sectionProgress)
        panta::ffi::task_progress_cancel(**m_sectionProgress);
    if (wasActive) {
        m_activeMesh.reset();
        emit meshChanged();
        prepareActiveMesh();
    }
    return true;
}
