/// 验证系统动态效果偏好经 Settings 单例向 QML 提供只读属性，布局键持久化到用户配置。
#include "settings.hpp"
#include <QFile>
#include <QJSEngine>
#include <QMetaProperty>
#include <QObject>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QSettings>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QString>
#include <QUrl>
#include <QVariant>
#include <QtCore/qtmetamacros.h>
#include <QtQml/qqml.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <memory>
#include <panta/qt_adapter/system_motion_preference.hpp>

using panta::bridge::Settings;

class SettingsTest final : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase() {
        // 布局写入用户配置目录：测试模式隔离到 ~/.qttest，避免污染真实设置。
        QStandardPaths::setTestModeEnabled(true);
    }
    void init() { QFile::remove(Settings::layoutSettingsFile()); }
    void reduced_motion_is_projected_to_qml() {
        const int settingsTypeId = qmlRegisterSingletonType<Settings>(
            "Panta.Test", 1, 0, "Settings",
            [](QQmlEngine*, QJSEngine*) -> QObject* { return new Settings; });
        QQmlEngine engine;
        auto* settings = engine.singletonInstance<Settings*>(settingsTypeId);
        QVERIFY(settings != nullptr);
        panta::qt_adapter::SystemMotionPreference systemPreference;
        QTRY_COMPARE(settings->reducedMotion(), systemPreference.reducedMotion());

        const auto property = settings->metaObject()->property(
            settings->metaObject()->indexOfProperty("reducedMotion"));
        QVERIFY(property.isValid());
        QVERIFY(!property.isWritable());
        QSignalSpy changes(settings, &Settings::reducedMotionChanged);
        QVERIFY(changes.isValid());

        QQmlComponent component(&engine);
        component.setData("import QtQml\nimport Panta.Test 1.0\n"
                          "QtObject { property bool reducedMotion: Settings.reducedMotion }",
                          QUrl());
        std::unique_ptr<QObject> object(component.create());
        QVERIFY2(object != nullptr, qPrintable(component.errorString()));
        QCOMPARE(object->property("reducedMotion").toBool(), settings->reducedMotion());
    }

    void layout_defaults_follow_design() {
        const Settings settings;
        QCOMPARE(settings.sidebarWidth(), 300);
        QCOMPARE(Settings::sidebarMinimumWidth(), 260);
        QCOMPARE(Settings::sidebarMaximumWidth(), 480);
        QVERIFY(!settings.sidebarCollapsed());
        QCOMPARE(settings.bottomPanelHeight(), 200);
        QVERIFY(settings.bottomPanelCollapsed());
    }

    void layout_values_are_clamped_and_persisted() {
        QVERIFY(!Settings::layoutSettingsFile().isEmpty());
        {
            Settings settings;
            QSignalSpy widthChanges(&settings, &Settings::sidebarWidthChanged);
            settings.setSidebarWidth(1000);
            QCOMPARE(settings.sidebarWidth(), 480);
            settings.setSidebarWidth(480);
            QCOMPARE(widthChanges.count(), 1);
            settings.setSidebarWidth(10);
            QCOMPARE(settings.sidebarWidth(), 260);
            settings.setSidebarWidth(320);
            settings.setSidebarCollapsed(true);
            settings.setBottomPanelHeight(5);
            QCOMPARE(settings.bottomPanelHeight(), 120);
            settings.setBottomPanelHeight(240);
            settings.setBottomPanelCollapsed(false);
        }
        const Settings reloaded;
        QCOMPARE(reloaded.sidebarWidth(), 320);
        QVERIFY(reloaded.sidebarCollapsed());
        QCOMPARE(reloaded.bottomPanelHeight(), 240);
        QVERIFY(!reloaded.bottomPanelCollapsed());
    }

    void corrupt_layout_values_fall_back_or_clamp() {
        {
            QSettings file(Settings::layoutSettingsFile(), QSettings::IniFormat);
            file.setValue(QStringLiteral("layout/sidebar_width"), QStringLiteral("wide"));
            file.setValue(QStringLiteral("layout/bottom_panel_height"), 9999);
        }
        const Settings settings;
        QCOMPARE(settings.sidebarWidth(), 300);
        QCOMPARE(settings.bottomPanelHeight(), 480);
    }
};

QTEST_GUILESS_MAIN(SettingsTest)
#include "settings_test.moc"
