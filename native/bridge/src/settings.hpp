/// QML 设置入口：投影系统减少动态效果偏好，并持久化工作台布局（task 036）。
#pragma once

#include <QObject>
#include <QString>
#include <QtQml/qqmlregistration.h>
#include <memory>

class QSettings;

namespace panta::qt_adapter {
class SystemMotionPreference;
}

namespace panta::bridge {

/// 应用 QML 引擎内共享的平台偏好与用户级界面布局入口。
///
/// 布局键写入用户配置目录的独立 ini（`workbench-layout.ini`），不进入工程文件，
/// 因此拖动或折叠面板不会让工程变脏。写入值先夹到允许范围，越界不报错。
class Settings : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool reducedMotion READ reducedMotion NOTIFY reducedMotionChanged)
    Q_PROPERTY(int sidebarWidth READ sidebarWidth WRITE setSidebarWidth NOTIFY sidebarWidthChanged)
    Q_PROPERTY(int sidebarMinimumWidth READ sidebarMinimumWidth CONSTANT)
    Q_PROPERTY(int sidebarMaximumWidth READ sidebarMaximumWidth CONSTANT)
    Q_PROPERTY(bool sidebarCollapsed READ sidebarCollapsed WRITE setSidebarCollapsed NOTIFY
                   sidebarCollapsedChanged)
    Q_PROPERTY(int bottomPanelHeight READ bottomPanelHeight WRITE setBottomPanelHeight NOTIFY
                   bottomPanelHeightChanged)
    Q_PROPERTY(int bottomPanelMinimumHeight READ bottomPanelMinimumHeight CONSTANT)
    Q_PROPERTY(int bottomPanelMaximumHeight READ bottomPanelMaximumHeight CONSTANT)
    Q_PROPERTY(bool bottomPanelCollapsed READ bottomPanelCollapsed WRITE setBottomPanelCollapsed
                   NOTIFY bottomPanelCollapsedChanged)

  public:
    explicit Settings(QObject* parent = nullptr);
    ~Settings() override;
    [[nodiscard]] bool reducedMotion() const;

    [[nodiscard]] int sidebarWidth() const { return m_sidebarWidth; }
    void setSidebarWidth(int width);
    [[nodiscard]] static int sidebarMinimumWidth();
    [[nodiscard]] static int sidebarMaximumWidth();
    [[nodiscard]] bool sidebarCollapsed() const { return m_sidebarCollapsed; }
    void setSidebarCollapsed(bool collapsed);
    [[nodiscard]] int bottomPanelHeight() const { return m_bottomPanelHeight; }
    void setBottomPanelHeight(int height);
    [[nodiscard]] static int bottomPanelMinimumHeight();
    [[nodiscard]] static int bottomPanelMaximumHeight();
    [[nodiscard]] bool bottomPanelCollapsed() const { return m_bottomPanelCollapsed; }
    void setBottomPanelCollapsed(bool collapsed);

    /// 布局 ini 的绝对路径；用户配置目录不可用时为空（此时只在内存中生效）。
    [[nodiscard]] static QString layoutSettingsFile();

  signals:
    void reducedMotionChanged();
    void sidebarWidthChanged();
    void sidebarCollapsedChanged();
    void bottomPanelHeightChanged();
    void bottomPanelCollapsedChanged();

  private:
    void store(const char* key, int value);

    std::unique_ptr<panta::qt_adapter::SystemMotionPreference> m_preference;
    std::unique_ptr<QSettings> m_layout;
    int m_sidebarWidth = 0;
    bool m_sidebarCollapsed = false;
    int m_bottomPanelHeight = 0;
    bool m_bottomPanelCollapsed = true;
};

} // namespace panta::bridge
