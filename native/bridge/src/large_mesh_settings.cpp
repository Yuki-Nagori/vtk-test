/// 大网格激活设置（task 026）：QSettings 持久化，校验与生效在 Rust。
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

namespace panta::bridge {

namespace {

constexpr auto kSurfaceTrianglesKey = "large_mesh/surface_triangles";
constexpr auto kSurfaceBudgetKey = "large_mesh/surface_budget";
constexpr auto kVolumeCellsKey = "large_mesh/volume_cells";

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
    const panta::ffi::LargeMeshLimits limits{
        settings.value(QLatin1String(kSurfaceTrianglesKey)).toULongLong(&ok_triangles),
        settings.value(QLatin1String(kSurfaceBudgetKey)).toULongLong(&ok_budget),
        settings.value(QLatin1String(kVolumeCellsKey)).toULongLong(&ok_cells)};
    // 读到非法值时保持 Rust 默认值，不报错（持久化文件可能被手工改坏）。
    if (ok_triangles && ok_budget && ok_cells)
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

bool ProjectViewModel::setLargeMeshLimits(double surfaceTriangles, double surfaceBudget,
                                          double volumeCells) {
    const auto triangles = to_count(surfaceTriangles);
    const auto budget = to_count(surfaceBudget);
    const auto cells = to_count(volumeCells);
    if (!triangles || !budget || !cells)
        return fail(QStringLiteral("project.lod_settings_invalid"), QStringLiteral("validation"),
                    QStringLiteral("not a non-negative integer"));
    const panta::ffi::LargeMeshLimits limits{*triangles, *budget, *cells};
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

} // namespace panta::bridge
