/// 验证系统动态效果偏好经 Settings 单例向 QML 提供只读属性。
#include "settings.hpp"
#include <QJSEngine>
#include <QMetaProperty>
#include <QObject>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QSignalSpy>
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
};

QTEST_GUILESS_MAIN(SettingsTest)
#include "settings_test.moc"
