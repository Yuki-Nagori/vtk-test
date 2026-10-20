/// 只读的系统减少动态效果偏好及其变化通知。
#pragma once

#include <QObject>
#include <QtGlobal>
#if defined(Q_OS_WIN)
#include <QAbstractNativeEventFilter>
#elif defined(Q_OS_LINUX)
#include <QDBusVariant>
#endif

namespace panta::qt_adapter {

/// 将操作系统减少动态效果偏好映射为稳定布尔值，并跟踪运行期变化。
class SystemMotionPreference final : public QObject
#if defined(Q_OS_WIN)
    ,
                                     public QAbstractNativeEventFilter
#endif
{
    Q_OBJECT
    Q_PROPERTY(bool reducedMotion READ reducedMotion NOTIFY reducedMotionChanged)

  public:
    explicit SystemMotionPreference(QObject* parent = nullptr);
    ~SystemMotionPreference() override;

    [[nodiscard]] bool reducedMotion() const;

  signals:
    void reducedMotionChanged();

  private slots:
    void refreshReducedMotion();
#if defined(Q_OS_LINUX)
    void portalSettingChanged(const QString& nameSpace, const QString& key,
                              const QDBusVariant& value);
#endif

  private:
#if defined(Q_OS_WIN)
    bool nativeEventFilter(const QByteArray& eventType, void* message, qintptr* result) override;
#elif defined(Q_OS_LINUX)
    void startPortalRead();
    quint64 m_portalRevision = 0;
#elif defined(Q_OS_MACOS)
    void* m_platformObserver = nullptr;
#endif
    void publishReducedMotion(bool reducedMotion);
    bool m_reducedMotion = false;
};

} // namespace panta::qt_adapter
