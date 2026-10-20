#include "utf8_boundary.hpp"
#include <QChar>
#include <QString>
#include <QtTypes>
#include <gtest/gtest.h>
#include <initializer_list>
#include <string>

TEST(Utf8BoundaryTest, PreservesUnicodeAndEmbeddedNullBytes) {
    QString text = QStringLiteral("资产/零件 😀");
    text.append(QChar(0));
    text.append(QStringLiteral(".stl"));
    std::string encoded;
    QString error;
    ASSERT_TRUE(panta::bridge::to_boundary_utf8(text, &encoded, &error));
    EXPECT_TRUE(error.isEmpty());
    EXPECT_EQ(QString::fromUtf8(encoded.data(), static_cast<qsizetype>(encoded.size())), text);
}

TEST(Utf8BoundaryTest, RejectsUnpairedSurrogatesWithoutReplacingOutput) {
    for (const char16_t surrogate : {char16_t{0xD800}, char16_t{0xDC00}}) {
        const QString text{QChar(surrogate)};
        std::string encoded = "unchanged";
        QString error;
        EXPECT_FALSE(panta::bridge::to_boundary_utf8(text, &encoded, &error));
        EXPECT_EQ(error, QStringLiteral("path.non_unicode"));
        EXPECT_EQ(encoded, "unchanged");
        EXPECT_FALSE(panta::bridge::to_boundary_utf8(text, nullptr, nullptr));
    }
}
