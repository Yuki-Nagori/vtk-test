#include "settings.hpp"

#include <QDir>
#include <QObject>
#include <QSettings>
#include <QString>
#include <QStringLiteral>
#include <QVariant>
#include <algorithm>
#include <memory>
#include <panta/qt_adapter/standard_paths.hpp>
#include <panta/qt_adapter/system_motion_preference.hpp>

namespace panta::bridge {

namespace {

// 034 Q4：左栏默认 300 px，可在 260–480 px 间拖动；底部面板默认折叠。
constexpr int kSidebarDefaultWidth = 300;
constexpr int kSidebarMinimumWidth = 260;
constexpr int kSidebarMaximumWidth = 480;
constexpr int kBottomPanelDefaultHeight = 200;
constexpr int kBottomPanelMinimumHeight = 120;
constexpr int kBottomPanelMaximumHeight = 480;

constexpr auto kSidebarWidthKey = "layout/sidebar_width";
constexpr auto kSidebarCollapsedKey = "layout/sidebar_collapsed";
constexpr auto kBottomPanelHeightKey = "layout/bottom_panel_height";
constexpr auto kBottomPanelCollapsedKey = "layout/bottom_panel_collapsed";

int read_int(const QSettings* settings, const char* key, int fallback, int low, int high) {
    if (settings == nullptr)
        return fallback;
    bool ok = false;
    const int value = settings->value(QLatin1String(key), fallback).toInt(&ok);
    return ok ? std::clamp(value, low, high) : fallback;
}

bool read_bool(const QSettings* settings, const char* key, bool fallback) {
    return settings == nullptr ? fallback : settings->value(QLatin1String(key), fallback).toBool();
}

} // namespace

Settings::Settings(QObject* parent)
    : QObject(parent), m_preference(std::make_unique<panta::qt_adapter::SystemMotionPreference>()) {
    connect(m_preference.get(), &panta::qt_adapter::SystemMotionPreference::reducedMotionChanged,
            this, &Settings::reducedMotionChanged);
    if (const QString file = layoutSettingsFile(); !file.isEmpty())
        m_layout = std::make_unique<QSettings>(file, QSettings::IniFormat);
    m_sidebarWidth = read_int(m_layout.get(), kSidebarWidthKey, kSidebarDefaultWidth,
                              kSidebarMinimumWidth, kSidebarMaximumWidth);
    m_sidebarCollapsed = read_bool(m_layout.get(), kSidebarCollapsedKey, false);
    m_bottomPanelHeight = read_int(m_layout.get(), kBottomPanelHeightKey, kBottomPanelDefaultHeight,
                                   kBottomPanelMinimumHeight, kBottomPanelMaximumHeight);
    m_bottomPanelCollapsed = read_bool(m_layout.get(), kBottomPanelCollapsedKey, true);
}

Settings::~Settings() = default;

bool Settings::reducedMotion() const { return m_preference->reducedMotion(); }

QString Settings::layoutSettingsFile() {
    // 与最近工程相同：独立 ini，不依赖 QCoreApplication 组织名；测试用 QStandardPaths
    // 测试模式隔离。
    const QString directory =
        panta::qt_adapter::standard_location(panta::qt_adapter::StandardLocation::UserConfig);
    return directory.isEmpty() ? QString()
                               : QDir(directory).filePath(QStringLiteral("workbench-layout.ini"));
}

int Settings::sidebarMinimumWidth() { return kSidebarMinimumWidth; }
int Settings::sidebarMaximumWidth() { return kSidebarMaximumWidth; }
int Settings::bottomPanelMinimumHeight() { return kBottomPanelMinimumHeight; }
int Settings::bottomPanelMaximumHeight() { return kBottomPanelMaximumHeight; }

void Settings::store(const char* key, int value) {
    // QSettings 自带写缓存并在析构 / 事件循环空闲时落盘；拖动期间不逐帧 sync。
    if (m_layout)
        m_layout->setValue(QLatin1String(key), value);
}

void Settings::setSidebarWidth(int width) {
    const int clamped = std::clamp(width, kSidebarMinimumWidth, kSidebarMaximumWidth);
    if (clamped == m_sidebarWidth)
        return;
    m_sidebarWidth = clamped;
    store(kSidebarWidthKey, clamped);
    emit sidebarWidthChanged();
}

void Settings::setSidebarCollapsed(bool collapsed) {
    if (collapsed == m_sidebarCollapsed)
        return;
    m_sidebarCollapsed = collapsed;
    store(kSidebarCollapsedKey, collapsed ? 1 : 0);
    emit sidebarCollapsedChanged();
}

void Settings::setBottomPanelHeight(int height) {
    const int clamped = std::clamp(height, kBottomPanelMinimumHeight, kBottomPanelMaximumHeight);
    if (clamped == m_bottomPanelHeight)
        return;
    m_bottomPanelHeight = clamped;
    store(kBottomPanelHeightKey, clamped);
    emit bottomPanelHeightChanged();
}

void Settings::setBottomPanelCollapsed(bool collapsed) {
    if (collapsed == m_bottomPanelCollapsed)
        return;
    m_bottomPanelCollapsed = collapsed;
    store(kBottomPanelCollapsedKey, collapsed ? 1 : 0);
    emit bottomPanelCollapsedChanged();
}

} // namespace panta::bridge
