#include "panta/qt_adapter/system_motion_preference.hpp"

#include <QCoreApplication>
#include <QString>
#include <QtCore/qbytearray.h>
#include <QtCore/qobjectdefs.h>
#include <qtmetamacros.h>
#if defined(Q_OS_WIN)
// Windows SDK 需先经此伞头建立目标架构和基础类型上下文，例外见 comments.md。
#include <windows.h>
#endif

#if defined(Q_OS_WIN)
// windef/winuser 再提供 MSG、SPI_GETCLIENTAREAANIMATION 等直接声明。
#include <windef.h>
#include <winuser.h>
#elif defined(Q_OS_LINUX)
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusVariant>
#include <QMetaType>
#include <QVariant>
#include <QtCore/qtenvironmentvariables.h>
#endif

#if defined(Q_OS_MACOS)
extern "C" bool panta_macos_read_reduced_motion();
extern "C" void*
panta_macos_observe_reduced_motion(panta::qt_adapter::SystemMotionPreference* preferences);
extern "C" void panta_macos_remove_reduced_motion_observer(void* observer);
#endif

namespace panta::qt_adapter {

namespace {

#if defined(Q_OS_LINUX)
constexpr auto kPortalService = "org.freedesktop.portal.Desktop";
constexpr auto kPortalPath = "/org/freedesktop/portal/desktop";
constexpr auto kPortalInterface = "org.freedesktop.portal.Settings";
constexpr auto kAppearanceNamespace = "org.freedesktop.appearance";
constexpr auto kReducedMotionKey = "reduced-motion";

bool reduced_motion_from_variant(const QVariant& value) {
    bool valid = false;
    const uint preference = value.toUInt(&valid);
    return valid && preference == 1U;
}
#endif

} // namespace

SystemMotionPreference::SystemMotionPreference(QObject* parent) : QObject(parent) {
#if defined(Q_OS_LINUX)
    const QString sessionAddress = qEnvironmentVariable("DBUS_SESSION_BUS_ADDRESS");
    if (!sessionAddress.isEmpty()) {
        auto bus = QDBusConnection::sessionBus();
        bus.connect(QString::fromLatin1(kPortalService), QString::fromLatin1(kPortalPath),
                    QString::fromLatin1(kPortalInterface), QStringLiteral("SettingChanged"), this,
                    SLOT(portalSettingChanged(QString, QString, QDBusVariant)));
    }
#elif defined(Q_OS_WIN)
    if (auto* application = QCoreApplication::instance()) {
        application->installNativeEventFilter(this);
    }
#endif
    refreshReducedMotion();
#if defined(Q_OS_MACOS)
    m_platformObserver = panta_macos_observe_reduced_motion(this);
#endif
}

SystemMotionPreference::~SystemMotionPreference() {
#if defined(Q_OS_LINUX)
    if (!qEnvironmentVariableIsEmpty("DBUS_SESSION_BUS_ADDRESS")) {
        QDBusConnection::sessionBus().disconnect(
            QString::fromLatin1(kPortalService), QString::fromLatin1(kPortalPath),
            QString::fromLatin1(kPortalInterface), QStringLiteral("SettingChanged"), this,
            SLOT(portalSettingChanged(QString, QString, QDBusVariant)));
    }
#elif defined(Q_OS_WIN)
    if (auto* application = QCoreApplication::instance()) {
        application->removeNativeEventFilter(this);
    }
#elif defined(Q_OS_MACOS)
    if (m_platformObserver != nullptr) {
        panta_macos_remove_reduced_motion_observer(m_platformObserver);
    }
#endif
}

bool SystemMotionPreference::reducedMotion() const { return m_reducedMotion; }

void SystemMotionPreference::refreshReducedMotion() {
#if defined(Q_OS_MACOS)
    publishReducedMotion(panta_macos_read_reduced_motion());
#elif defined(Q_OS_WIN)
    BOOL animationsEnabled = FALSE;
    const bool available =
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &animationsEnabled, 0) != FALSE;
    publishReducedMotion(available && animationsEnabled == FALSE);
#elif defined(Q_OS_LINUX)
    startPortalRead();
#else
    publishReducedMotion(false);
#endif
}

void SystemMotionPreference::publishReducedMotion(bool reducedMotion) {
    if (m_reducedMotion == reducedMotion) {
        return;
    }
    m_reducedMotion = reducedMotion;
    emit reducedMotionChanged();
}

#if defined(Q_OS_WIN)
bool SystemMotionPreference::nativeEventFilter(const QByteArray& eventType, void* message,
                                               qintptr*) {
    if (eventType == QByteArrayLiteral("windows_generic_MSG") &&
        static_cast<MSG*>(message)->message == WM_SETTINGCHANGE) {
        refreshReducedMotion();
    }
    return false;
}
#elif defined(Q_OS_LINUX)
void SystemMotionPreference::startPortalRead() {
    if (qEnvironmentVariableIsEmpty("DBUS_SESSION_BUS_ADDRESS")) {
        return;
    }
    const auto bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        return;
    }

    QDBusMessage request = QDBusMessage::createMethodCall(
        QString::fromLatin1(kPortalService), QString::fromLatin1(kPortalPath),
        QString::fromLatin1(kPortalInterface), QStringLiteral("ReadOne"));
    request << QString::fromLatin1(kAppearanceNamespace) << QString::fromLatin1(kReducedMotionKey);
    const quint64 revision = m_portalRevision;
    auto* watcher = new QDBusPendingCallWatcher(bus.asyncCall(request, 1000), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this,
            [this, revision](QDBusPendingCallWatcher* finished) {
                const QDBusPendingReply<QDBusVariant> reply = *finished;
                finished->deleteLater();
                if (!reply.isValid() || m_portalRevision != revision) {
                    return;
                }
                publishReducedMotion(reduced_motion_from_variant(reply.value().variant()));
            });
}

void SystemMotionPreference::portalSettingChanged(const QString& nameSpace, const QString& key,
                                                  const QDBusVariant& value) {
    if (nameSpace != QString::fromLatin1(kAppearanceNamespace) ||
        key != QString::fromLatin1(kReducedMotionKey)) {
        return;
    }
    ++m_portalRevision;
    publishReducedMotion(reduced_motion_from_variant(value.variant()));
}
#endif

} // namespace panta::qt_adapter
