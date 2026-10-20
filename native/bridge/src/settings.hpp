/// QML 设置入口：投影系统减少动态效果偏好。
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>
#include <memory>

namespace panta::qt_adapter {
class SystemMotionPreference;
}

namespace panta::bridge {

/// 应用 QML 引擎内共享的平台偏好入口。
class Settings : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool reducedMotion READ reducedMotion NOTIFY reducedMotionChanged)

  public:
    explicit Settings(QObject* parent = nullptr);
    ~Settings() override;
    [[nodiscard]] bool reducedMotion() const;

  signals:
    void reducedMotionChanged();

  private:
    std::unique_ptr<panta::qt_adapter::SystemMotionPreference> m_preference;
};

} // namespace panta::bridge
