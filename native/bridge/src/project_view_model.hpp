/// 工程服务的 Qt 适配层与运行期视口文档状态。
///
/// Rust application service 持有工程清单、导入记录和异步资产激活；
/// 本类型缓存工作区的 Qt 投影与当前活动显示 DTO，不拥有第二份领域选择。
#pragma once

#include "panta_ffi.h"
#include <QElapsedTimer>
#include <QStringList>
#include <QTimer>
#include <QUrl>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>
#include <array>
#include <functional>
#include <optional>
#include <panta/visualization/mesh_source.hpp>
#include <rust/cxx.h>

namespace panta::bridge {

class ProjectViewModel : public panta::visualization::MeshSource {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString defaultLocation READ defaultLocation CONSTANT)
    Q_PROPERTY(QUrl defaultLocationUrl READ defaultLocationUrl CONSTANT)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
    Q_PROPERTY(QString errorCode READ errorCode NOTIFY errorChanged)
    Q_PROPERTY(QString errorCategory READ errorCategory NOTIFY errorChanged)
    Q_PROPERTY(QString errorDetail READ errorDetail NOTIFY errorChanged)
    Q_PROPERTY(QString currentPath READ currentPath NOTIFY projectChanged)
    Q_PROPERTY(QString currentName READ currentName NOTIFY projectChanged)
    Q_PROPERTY(bool dirty READ dirty NOTIFY projectChanged)
    Q_PROPERTY(bool projectOperationPending READ projectOperationPending NOTIFY
                   projectOperationPendingChanged)
    Q_PROPERTY(bool importPreviewLargeMeshWarning READ importPreviewLargeMeshWarning NOTIFY
                   importPreviewChanged)
    Q_PROPERTY(quint64 importPreviewExpandedBytes READ importPreviewExpandedBytes NOTIFY
                   importPreviewChanged)
    Q_PROPERTY(QString lastCreatedPath READ lastCreatedPath NOTIFY projectCreated)
    Q_PROPERTY(QStringList importedPartNames READ importedPartNames NOTIFY importsChanged)
    Q_PROPERTY(QStringList importedPartIds READ importedPartIds NOTIFY importsChanged)
    Q_PROPERTY(QString importedPartName READ importedPartName NOTIFY importsChanged)
    Q_PROPERTY(QString importedAssetPath READ importedAssetPath NOTIFY importsChanged)
    Q_PROPERTY(QString importedMeshType READ importedMeshType NOTIFY importsChanged)
    Q_PROPERTY(QString importedUnits READ importedUnits NOTIFY importsChanged)
    Q_PROPERTY(QString importedDimensions READ importedDimensions NOTIFY importsChanged)
    Q_PROPERTY(quint64 importedTriangleCount READ importedTriangleCount NOTIFY importsChanged)
    Q_PROPERTY(QString activeMeshLodStatus READ activeMeshLodStatus NOTIFY meshChanged)
    Q_PROPERTY(
        double lodActivationTriangles READ lodActivationTriangles NOTIFY largeMeshLimitsChanged)
    Q_PROPERTY(double lodBudgetTriangles READ lodBudgetTriangles NOTIFY largeMeshLimitsChanged)
    Q_PROPERTY(double volumeStreamCells READ volumeStreamCells NOTIFY largeMeshLimitsChanged)
    Q_PROPERTY(QVariantList meshTypes READ meshTypes CONSTANT)
    Q_PROPERTY(QString defaultMeshType READ defaultMeshType CONSTANT)
    Q_PROPERTY(bool importPreviewPending READ importPreviewPending NOTIFY importPreviewChanged)
    Q_PROPERTY(bool importPreviewReady READ importPreviewReady NOTIFY importPreviewChanged)
    Q_PROPERTY(QString importPreviewName READ importPreviewName NOTIFY importPreviewChanged)
    Q_PROPERTY(
        QString importPreviewDimensions READ importPreviewDimensions NOTIFY importPreviewChanged)
    Q_PROPERTY(quint64 importPreviewTriangleCount READ importPreviewTriangleCount NOTIFY
                   importPreviewChanged)
    Q_PROPERTY(bool importPending READ importPending NOTIFY importPendingChanged)
    Q_PROPERTY(quint64 importRequestId READ importRequestId NOTIFY importPendingChanged)
    Q_PROPERTY(QVariantList openDocuments READ openDocuments NOTIFY documentsChanged)
    Q_PROPERTY(QString activeDocumentId READ activeDocumentId NOTIFY activeDocumentChanged)
    Q_PROPERTY(QString activeDocumentTitle READ activeDocumentTitle NOTIFY activeDocumentChanged)
    Q_PROPERTY(bool exportPending READ exportPending NOTIFY exportPendingChanged)
    Q_PROPERTY(bool diagnosticsPending READ diagnosticsPending NOTIFY meshQualityChanged)
    Q_PROPERTY(QVariantMap meshQuality READ meshQuality NOTIFY meshQualityChanged)
    /// 后台任务进度 0..=1；没有对应任务进行时为 -1（task 016）。
    Q_PROPERTY(double importProgress READ importProgress NOTIFY taskProgressChanged)
    Q_PROPERTY(double importPreviewProgress READ importPreviewProgress NOTIFY taskProgressChanged)
    Q_PROPERTY(double exportProgress READ exportProgress NOTIFY taskProgressChanged)
    Q_PROPERTY(double diagnosticsProgress READ diagnosticsProgress NOTIFY taskProgressChanged)
    Q_PROPERTY(bool generationPending READ generationPending NOTIFY generationPendingChanged)
    Q_PROPERTY(double generationProgress READ generationProgress NOTIFY taskProgressChanged)
    /// 最近打开/创建的工程路径（新到旧，最多 10 个，已不存在的文件在读取时剔除；task 018）。
    Q_PROPERTY(QStringList recentProjects READ recentProjects NOTIFY recentProjectsChanged)
    /// 时序动画（task 020）：只读序列播放，不入工程；活动时视口显示序列帧。
    Q_PROPERTY(bool animationActive READ animationActive NOTIFY animationChanged)
    Q_PROPERTY(int animationFrameCount READ animationFrameCount NOTIFY animationChanged)
    Q_PROPERTY(int animationFrame READ animationFrame NOTIFY animationChanged)
    Q_PROPERTY(bool animationPlaying READ animationPlaying NOTIFY animationChanged)
    Q_PROPERTY(int animationFps READ animationFps NOTIFY animationChanged)
    Q_PROPERTY(QString animationStatus READ animationStatus NOTIFY animationChanged)
    Q_PROPERTY(bool volumeAvailable READ volumeAvailable NOTIFY volumeChanged)
    Q_PROPERTY(QString volumeInfo READ volumeInfo NOTIFY volumeChanged)
    Q_PROPERTY(bool sectionActive READ sectionActive NOTIFY volumeChanged)
    Q_PROPERTY(double sectionNormalX READ sectionNormalX NOTIFY volumeChanged)
    Q_PROPERTY(double sectionNormalY READ sectionNormalY NOTIFY volumeChanged)
    Q_PROPERTY(double sectionNormalZ READ sectionNormalZ NOTIFY volumeChanged)
    Q_PROPERTY(double sectionFraction READ sectionFraction NOTIFY volumeChanged)

  public:
    explicit ProjectViewModel(QObject* parent = nullptr);
    [[nodiscard]] bool projectOperationPending() const { return m_projectOperationPending; }
    [[nodiscard]] bool exportPending() const { return m_exportPending; }
    [[nodiscard]] bool diagnosticsPending() const { return m_diagnosticsPending; }
    /// 最近一次诊断报告；documentId 标明所属页签，空映射表示尚未诊断。
    [[nodiscard]] QVariantMap meshQuality() const { return m_meshQuality; }
    [[nodiscard]] double importProgress() const;
    [[nodiscard]] double exportProgress() const;
    [[nodiscard]] double diagnosticsProgress() const;
    [[nodiscard]] bool importPreviewLargeMeshWarning() const {
        return m_importPreviewLargeMeshWarning;
    }
    [[nodiscard]] quint64 importPreviewExpandedBytes() const {
        return m_importPreviewExpandedBytes;
    }
    [[nodiscard]] std::shared_ptr<const panta::visualization::SurfaceMeshSnapshot>
    mesh_snapshot() const override;

    /// 默认工程目录为平台 Documents 目录下的 panta 子目录；不会在构造时写盘。
    [[nodiscard]] const QString& defaultLocation() const;
    [[nodiscard]] QUrl defaultLocationUrl() const;

    /// 最近一次创建成功的工程目录；失败时保留上一成功路径。
    [[nodiscard]] const QString& lastCreatedPath() const;

    /// 最近一次可恢复失败的用户可读摘要；空串表示无错误。
    [[nodiscard]] const QString& error() const;
    [[nodiscard]] const QString& errorCode() const;
    /// 稳定诊断类别与原始上下文；detail 供诊断，不参与业务判断。
    [[nodiscard]] const QString& errorCategory() const;
    [[nodiscard]] const QString& errorDetail() const;

    [[nodiscard]] const QString& currentPath() const;
    [[nodiscard]] const QString& currentName() const;
    [[nodiscard]] bool dirty() const;
    /// 名称按 Rust 工程导入记录顺序排列；QML 只负责呈现，不派生领域状态。
    [[nodiscard]] const QStringList& importedPartNames() const;
    [[nodiscard]] QStringList importedPartIds() const;
    [[nodiscard]] const QString& importedPartName() const;
    [[nodiscard]] const QString& importedAssetPath() const;
    [[nodiscard]] const QString& importedMeshType() const;
    [[nodiscard]] const QString& importedUnits() const;
    [[nodiscard]] const QString& importedDimensions() const;
    [[nodiscard]] quint64 importedTriangleCount() const;
    [[nodiscard]] QString activeMeshLodStatus() const;
    [[nodiscard]] double importPreviewProgress() const;
    [[nodiscard]] bool importPreviewPending() const;
    [[nodiscard]] bool importPreviewReady() const;
    [[nodiscard]] const QString& importPreviewName() const;
    [[nodiscard]] const QString& importPreviewDimensions() const;
    [[nodiscard]] quint64 importPreviewTriangleCount() const;
    [[nodiscard]] bool importPending() const;
    [[nodiscard]] quint64 importRequestId() const;

    [[nodiscard]] QVariantList meshTypes() const;
    [[nodiscard]] QString defaultMeshType() const;
    [[nodiscard]] QVariantList openDocuments() const; // Rust 工作区快照的 Qt 投影。
    [[nodiscard]] QString activeDocumentId() const;
    [[nodiscard]] QString activeDocumentTitle() const;
    /// 通过 Rust 工程服务创建目录和初始清单；工程资产由后续命令负责。
    ///
    /// 返回请求准入结果；projectCreated 仅在后台成功提交后发出。
    /// 文件系统和清单处理不占用 GUI，失败保留当前工程和对话框输入。
    Q_INVOKABLE bool createProject(const QString& name, const QString& location);

    /// 清除当前错误，供对话框每次打开时重置显示状态。
    Q_INVOKABLE void clearError();

    /// 后台打开工程；返回准入结果，成功由 projectOpened 通知。
    Q_INVOKABLE bool openProject(const QString& path);

    /// 将 Qt 文件 URL 转成本地路径并打开；QML 不自行解析 URL 字符串。
    Q_INVOKABLE bool openProjectUrl(const QUrl& url);

    /// FolderDialog/FileDialog 共用的 URL 适配；非本地 URL 返回空串。
    Q_INVOKABLE QString localPath(const QUrl& url) const;

    /// 后台保存当前工程；返回准入结果，完成由 projectOperationFinished 通知。
    Q_INVOKABLE bool saveProject();

    /// 示例模型命令：修改显示名称并将工程标记为 dirty。
    Q_INVOKABLE bool renameProject(const QString& name);

    /// 同步导入供非 GUI 调用与行为测试使用；QML 必须使用 beginImportStl。
    bool importStl(const QString& path, const QString& meshType, const QString& units,
                   bool showImportLog);
    /// 启动 Rust 后台确认导入；完成由 projectImportFinished 通知。
    Q_INVOKABLE bool beginImportStl(const QString& path, const QString& meshType,
                                    const QString& units, bool showImportLog);
    /// 请求取消导入；manifest 替换阶段开始后返回 false，完成回执仍由 ViewModel 消费。
    Q_INVOKABLE bool cancelImport();

    /// 发起 Rust 后台预检；返回请求是否被接受，完成由 importPreviewFinished 通知。
    /// 新选择替换旧请求，关闭弹窗需调用 cancelImportPreview，不等待解析线程。
    Q_INVOKABLE bool inspectStl(const QString& path);
    /// 废弃进行中的请求及已完成的预检快照，清空表单投影。
    Q_INVOKABLE void cancelImportPreview();

    /// 提交工作区选择意图；驻留 Mesh 的显示缓冲也在后台准备，完成后通知视口。
    /// 已提交资产由 Rust 按需异步激活。
    Q_INVOKABLE void activateDocument(const QString& documentId);

    /// 关闭文档：仅结束本次运行期视图，不删 ImportRecord / 资产、不设
    /// dirty。关闭 Loading 文档会先请求取消对应 attempt。
    Q_INVOKABLE void closeDocument(const QString& documentId);

    /// 工程树点击入口：已就绪记录直接激活；未打开记录创建 Loading 页签并
    /// 经 073 只读激活异步加载，成功后自动激活；失败保留 Failed 页签。
    Q_INVOKABLE void openImportRecord(const QString& recordId);

    /// 拖拽重排的提交入口；from/to 为 openDocuments 下标。
    Q_INVOKABLE void moveDocument(int fromIndex, int toIndex);

    /// 把活动页签的已驻留网格导出到本地文件（格式由扩展名决定，坐标为毫米）。
    /// 写盘在后台线程执行；返回准入结果，完成后发出 exportFinished。
    Q_INVOKABLE bool beginExportActiveMeshUrl(const QUrl& url);

    /// 在后台诊断活动页签的已驻留网格（task 006 diagnose_surface，默认容差）；
    /// 返回准入结果，完成后更新 meshQuality 并发出 diagnosticsFinished。
    Q_INVOKABLE bool beginDiagnoseActiveMesh();

    /// 请求协作式取消进行中的导出 / 诊断；返回是否有任务接收了请求。
    /// 完成信号仍会到达，结果为 project.export_cancelled / project.diagnostics_cancelled。
    Q_INVOKABLE bool cancelExport();
    Q_INVOKABLE bool cancelDiagnostics();

    /// STEP → Netgen 四面体体网格（task 023）。STEP 读取与 Netgen 生成在后台线程，
    /// 成功后经 Rust 写入工程（复用导入提交与发布）；maxLengthMm <= 0 用 Netgen 默认。
    /// 返回准入结果，完成后发出 generationFinished。
    Q_INVOKABLE bool beginGenerateVolumeFromStep(const QString& path, double maxLengthMm);
    /// 协作式取消；生成阶段经 Netgen terminate 在检查点返回，提交阶段同 cancelImport。
    Q_INVOKABLE bool cancelGeneration();
    [[nodiscard]] bool generationPending() const { return m_generationPending; }
    /// 0..1；生成占 0–0.9，工程提交占 0.9–1；无任务时为 -1。
    [[nodiscard]] double generationProgress() const;
    /// 清空最近工程列表。
    Q_INVOKABLE void clearRecentProjects();
    /// 拖放分类：返回 "project"（.panta）、"mesh"（可导入网格扩展名）或空串（不支持）。
    Q_INVOKABLE QString classifyDroppedUrl(const QUrl& url) const;
    [[nodiscard]] QStringList recentProjects() const;
    static constexpr int kMaxRecentProjects = 10;

    Q_INVOKABLE bool openAnimationUrl(const QUrl& url);
    Q_INVOKABLE void closeAnimation();
    Q_INVOKABLE void setAnimationFrame(int frame);
    Q_INVOKABLE void stepAnimation(int delta);
    Q_INVOKABLE void setAnimationPlaying(bool playing);
    Q_INVOKABLE void setAnimationFps(int fps);
    [[nodiscard]] bool animationActive() const { return m_animation.has_value(); }
    [[nodiscard]] int animationFrameCount() const { return m_animationFrameCount; }
    [[nodiscard]] int animationFrame() const { return m_animationDisplayed; }
    [[nodiscard]] bool animationPlaying() const { return m_animationPlaying; }
    [[nodiscard]] int animationFps() const { return m_animationFps; }
    [[nodiscard]] QString animationStatus() const;
    /// 体网格单切面（task 021/024）：平面由法向 (nx, ny, nz) 和沿法向投影区间的 0..1 位置确定；
    /// 只替换视口显示，不写工程、不生成资产。启用时零法向或非有限值被拒绝。
    Q_INVOKABLE bool setSection(bool enabled, double normalX, double normalY, double normalZ,
                                double fraction);
    /// 大网格设置（task 026）：表面 LOD 阈值 / 预算（面）、体网格流式阈值（四面体）。
    /// 校验在 Rust，非法时返回 false 并设置 `project.lod_settings_invalid`；成功后持久化。
    /// 只影响之后的激活，已打开的网格不重算。
    Q_INVOKABLE bool setLargeMeshLimits(double surfaceTriangles, double surfaceBudget,
                                        double volumeCells);
    Q_INVOKABLE void resetLargeMeshLimits();
    [[nodiscard]] double lodActivationTriangles() const;
    [[nodiscard]] double lodBudgetTriangles() const;
    [[nodiscard]] double volumeStreamCells() const;
    [[nodiscard]] bool volumeAvailable() const { return m_volumeAvailable; }
    [[nodiscard]] QString volumeInfo() const { return m_volumeInfo; }
    [[nodiscard]] bool sectionActive() const { return m_sectionActive; }
    [[nodiscard]] double sectionNormalX() const { return m_sectionNormal[0]; }
    [[nodiscard]] double sectionNormalY() const { return m_sectionNormal[1]; }
    [[nodiscard]] double sectionNormalZ() const { return m_sectionNormal[2]; }
    [[nodiscard]] double sectionFraction() const { return m_sectionFraction; }

  signals:
    void animationChanged();
    void volumeChanged();
    void largeMeshLimitsChanged();
    void errorChanged();
    void projectOperationPendingChanged();
    /// operation 为 create/open/save；成功表示已提交并发布快照。
    void projectOperationFinished(const QString& operation, bool succeeded);
    void projectChanged();
    void recentProjectsChanged();
    void projectCreated(const QString& path);
    void projectOpened(const QString& path);
    void projectSaved(const QString& path);
    void importsChanged();
    void projectImported(const QString& path);
    void importPendingChanged();
    void projectImportFinished(quint64 requestId, bool succeeded);
    void importPreviewChanged();
    /// 当前请求完成且发布快照后触发；取消或在通知中被替换的请求不触发。
    void importPreviewFinished(quint64 requestId, bool succeeded);
    void documentsChanged();
    void activeDocumentChanged();
    void exportPendingChanged();
    void exportFinished(const QString& path, bool succeeded);
    void meshQualityChanged();
    void diagnosticsFinished(bool succeeded);
    /// 任一后台任务进行中时约每 100ms 触发一次。
    void taskProgressChanged();
    void generationPendingChanged();
    void generationFinished(bool succeeded);

  protected:
    /// 启动只等待独占 Rust 请求的线程；启动失败抛 system_error，调用方尚未准入领域任务。
    virtual void startBackgroundWaiter(std::function<void()> work);

  private:
    void rememberRecentProject(const QString& path);
    [[nodiscard]] static QString recentProjectsFile();
    [[nodiscard]] static QString largeMeshSettingsFile();
    void loadLargeMeshLimits();
    bool beginProjectOperation(panta::ffi::ProjectOperationInput input, const QString& operation);
    void prepareActiveMesh();
    bool fail(const panta::ffi::ProjectDiagnostic& diagnostic);
    bool fail(const QString& code, const QString& category = QStringLiteral("validation"),
              const QString& detail = {});
    bool applySnapshot(const panta::ffi::ProjectSnapshotResult& result);
    bool applySnapshot(const panta::ffi::ProjectSnapshot& snapshot);
    void applyImports(const rust::Vec<panta::ffi::ProjectImport>& imports);
    bool refreshImports();
    bool publish_import(const panta::ffi::ProjectImport& imported);
    static bool toBoundaryText(const QString& text, std::string* out, QString* error);

    void refresh_workspace(bool forceActiveNotification = false);
    void reset_documents();
    void drain_activations();
    void sync_activation_poll();
    void finish_import_preview();
    void finish_stl_import();
    void finish_generation(bool succeeded);

    QString m_defaultLocation;
    const QVariantList m_meshTypes;
    const QString m_defaultMeshType;
    QString m_error;
    QString m_errorCode;
    QString m_errorCategory;
    QString m_errorDetail;
    QString m_currentPath;
    QString m_currentName;
    QString m_lastCreatedPath;
    bool m_dirty = false;
    bool m_projectOperationPending = false;
    bool m_meshPreparationPending = false;
    bool m_exportPending = false;
    bool m_diagnosticsPending = false;
    QVariantMap m_meshQuality;
    quint64 m_meshSelection = 0;
    bool m_importPreviewLargeMeshWarning = false;
    quint64 m_importPreviewExpandedBytes = 0;
    QStringList m_importedPartNames;
    QStringList m_importedPartIds;
    QString m_importedPartName;
    QString m_importedAssetPath;
    QString m_importedMeshType;
    QString m_importedUnits;
    QString m_importedDimensions;
    quint64 m_importedTriangleCount = 0;
    bool m_importPreviewReady = false;
    QString m_importPreviewName;
    QString m_importPreviewDimensions;
    quint64 m_importPreviewTriangleCount = 0;
    quint64 m_importRequest = 0;
    bool m_importPending = false;
    QVariantList m_openDocuments;
    QString m_activeDocumentId;
    /// 当前活动文档的 C++ 显示 DTO；Rust ProjectService 拥有所有驻留 Mesh 与缓存策略。
    std::shared_ptr<const panta::visualization::SurfaceMeshSnapshot> m_activeMesh;
    quint64 m_activeMeshTriangles = 0;
    quint64 m_activeLodSourceTriangles = 0;
    QTimer m_importPreviewPoll;
    QTimer m_stlImportPoll;
    QTimer m_taskProgressPoll;
    void tick_animation();
    void refresh_volume();
    /// 后台计算切面（task 022）：同时最多一个任务，参数变化时取消在途任务并在完成后重跑最新意图。
    void request_section();
    bool m_sectionPending = false;
    bool m_sectionDirty = false;
    std::optional<rust::Box<panta::ffi::TaskProgress>> m_sectionProgress;
    bool m_volumeAvailable = false;
    QString m_volumeInfo;
    bool m_sectionActive = false;
    std::array<double, 3> m_sectionNormal{0.0, 0.0, 1.0};
    double m_sectionFraction = 0.5;
    std::optional<rust::Box<panta::ffi::AnimationSequence>> m_animation;
    QTimer m_animationPoll;
    QElapsedTimer m_animationClock;
    int m_animationFrameCount = 0;
    int m_animationTarget = 0;
    int m_animationDisplayed = -1;
    bool m_animationPlaying = false;
    int m_animationFps = 10;
    QString m_animationFrameError;
    std::optional<rust::Box<panta::ffi::TaskProgress>> m_exportProgress;
    std::optional<rust::Box<panta::ffi::TaskProgress>> m_diagnosticsProgress;
    /// 生成阶段的进度/取消句柄；进入工程提交阶段后清空，进度改读导入请求。
    std::optional<rust::Box<panta::ffi::TaskProgress>> m_generationProgress;
    bool m_generationPending = false;
    quint64 m_previewRequest = 0;
    bool m_importPreviewPending = false;
    QTimer m_activationPoll;
    rust::Box<panta::ffi::ProjectService> m_service;
};

} // namespace panta::bridge
