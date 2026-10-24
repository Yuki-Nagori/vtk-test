/// 大网格激活设置（task 026 / 028）：QSettings 持久化，校验与生效在 Rust；
/// 另负责派生分块缓存目录与可用物理内存的宿主侧查询。
#include "panta_ffi.h"
#include "project_view_model.hpp"
#include <QDir>
#include <QSettings>
#include <QString>
#include <QStringLiteral>
#include <QVariant>
#include <cmath>
#include <cstdint>
#include <optional>
#include <panta/qt_adapter/standard_paths.hpp>
#include <panta/qt_adapter/system_memory.hpp>
#include <string>

namespace panta::bridge {

namespace {

constexpr auto kSurfaceTrianglesKey = "large_mesh/surface_triangles";
constexpr auto kSurfaceBudgetKey = "large_mesh/surface_budget";
constexpr auto kVolumeCellsKey = "large_mesh/volume_cells";
constexpr auto kVolumeModeKey = "large_mesh/volume_mode";
constexpr auto kChunkCacheKey = "large_mesh/chunk_cache_mib";

/// QML 数值为 double：只接受有限、非负的整数（< 2^53，精确可表示）。
std::optional<std::uint64_t> to_count(double value) {
    constexpr double kMaxExact = 9007199254740992.0;
    if (!std::isfinite(value) || value < 0.0 || value >= kMaxExact || std::floor(value) != value)
        return std::nullopt;
    return static_cast<std::uint64_t>(value);
}

} // namespace

QString ProjectViewModel::largeMeshSettingsFile() {
    // 与最近工程相同：独立 ini，测试用 QStandardPaths 测试模式隔离。
    const QString directory =
        panta::qt_adapter::standard_location(panta::qt_adapter::StandardLocation::UserConfig);
    return directory.isEmpty() ? QString()
                               : QDir(directory).filePath(QStringLiteral("large-mesh.ini"));
}

void ProjectViewModel::loadLargeMeshLimits() {
    const QString file = largeMeshSettingsFile();
    if (file.isEmpty())
        return;
    const QSettings settings(file, QSettings::IniFormat);
    if (!settings.contains(QLatin1String(kSurfaceTrianglesKey)))
        return;
    bool ok_triangles = false;
    bool ok_budget = false;
    bool ok_cells = false;
    bool ok_mode = true;
    bool ok_cache = true;
    // task 026 写出的文件没有模式与块缓存键：沿用默认值。
    const auto defaults = panta::ffi::default_large_mesh_limits();
    const panta::ffi::LargeMeshLimits limits{
        settings.value(QLatin1String(kSurfaceTrianglesKey)).toULongLong(&ok_triangles),
        settings.value(QLatin1String(kSurfaceBudgetKey)).toULongLong(&ok_budget),
        settings.value(QLatin1String(kVolumeCellsKey)).toULongLong(&ok_cells),
        settings.contains(QLatin1String(kVolumeModeKey))
            ? settings.value(QLatin1String(kVolumeModeKey)).toUInt(&ok_mode)
            : defaults.volume_mode,
        settings.contains(QLatin1String(kChunkCacheKey))
            ? settings.value(QLatin1String(kChunkCacheKey)).toULongLong(&ok_cache)
            : defaults.chunk_cache_mib};
    // 读到非法值时保持 Rust 默认值，不报错（持久化文件可能被手工改坏）。
    if (ok_triangles && ok_budget && ok_cells && ok_mode && ok_cache)
        static_cast<void>(panta::ffi::project_service_set_large_mesh_limits(*m_service, limits));
}

double ProjectViewModel::lodActivationTriangles() const {
    return static_cast<double>(
        panta::ffi::project_service_large_mesh_limits(*m_service).surface_triangles);
}

double ProjectViewModel::lodBudgetTriangles() const {
    return static_cast<double>(
        panta::ffi::project_service_large_mesh_limits(*m_service).surface_budget);
}

double ProjectViewModel::volumeStreamCells() const {
    return static_cast<double>(
        panta::ffi::project_service_large_mesh_limits(*m_service).volume_cells);
}

int ProjectViewModel::volumeMode() const {
    return static_cast<int>(panta::ffi::project_service_large_mesh_limits(*m_service).volume_mode);
}

double ProjectViewModel::chunkCacheMib() const {
    return static_cast<double>(
        panta::ffi::project_service_large_mesh_limits(*m_service).chunk_cache_mib);
}

bool ProjectViewModel::setLargeMeshLimits(double surfaceTriangles, double surfaceBudget,
                                          double volumeCells, int volumeMode,
                                          double chunkCacheMib) {
    const auto triangles = to_count(surfaceTriangles);
    const auto budget = to_count(surfaceBudget);
    const auto cells = to_count(volumeCells);
    const auto cache = to_count(chunkCacheMib);
    if (!triangles || !budget || !cells || !cache || volumeMode < 0)
        return fail(QStringLiteral("project.lod_settings_invalid"), QStringLiteral("validation"),
                    QStringLiteral("not a non-negative integer"));
    const panta::ffi::LargeMeshLimits limits{*triangles, *budget, *cells,
                                             static_cast<std::uint32_t>(volumeMode), *cache};
    const auto error = panta::ffi::project_service_set_large_mesh_limits(*m_service, limits);
    if (!error.code.empty())
        return fail(error);
    const QString file = largeMeshSettingsFile();
    if (!file.isEmpty()) {
        QSettings settings(file, QSettings::IniFormat);
        settings.setValue(QLatin1String(kSurfaceTrianglesKey),
                          QVariant::fromValue(static_cast<qulonglong>(*triangles)));
        settings.setValue(QLatin1String(kSurfaceBudgetKey),
                          QVariant::fromValue(static_cast<qulonglong>(*budget)));
        settings.setValue(QLatin1String(kVolumeCellsKey),
                          QVariant::fromValue(static_cast<qulonglong>(*cells)));
        settings.setValue(QLatin1String(kVolumeModeKey), volumeMode);
        settings.setValue(QLatin1String(kChunkCacheKey),
                          QVariant::fromValue(static_cast<qulonglong>(*cache)));
        settings.sync();
    }
    emit largeMeshLimitsChanged();
    return true;
}

void ProjectViewModel::resetLargeMeshLimits() {
    const auto defaults = panta::ffi::default_large_mesh_limits();
    static_cast<void>(panta::ffi::project_service_set_large_mesh_limits(*m_service, defaults));
    const QString file = largeMeshSettingsFile();
    if (!file.isEmpty()) {
        QSettings settings(file, QSettings::IniFormat);
        settings.remove(QStringLiteral("large_mesh"));
        settings.sync();
    }
    emit largeMeshLimitsChanged();
}

void ProjectViewModel::configureDerivedChunkCache() {
    // 派生文件可随时重建，放在缓存目录；目录由 Rust 在首次构建时创建。
    const QString directory =
        panta::qt_adapter::standard_location(panta::qt_adapter::StandardLocation::Cache);
    const std::string path =
        directory.isEmpty()
            ? std::string()
            : QDir(directory).filePath(QStringLiteral("volume-chunks")).toStdString();
    panta::ffi::project_service_set_derived_chunk_dir(*m_service, path);
}

void ProjectViewModel::refreshAvailableMemory() {
    panta::ffi::project_service_set_available_memory(
        *m_service, panta::qt_adapter::available_physical_memory());
}

} // namespace panta::bridge
