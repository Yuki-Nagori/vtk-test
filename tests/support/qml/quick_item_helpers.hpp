// 布局断言遍历可视树：Repeater delegate 与被替换的 contentItem 不保证 QObject 父链同构。
#pragma once

#include <QList>
#include <QQuickItem>
#include <QString>

inline QList<QQuickItem*> visual_items(QQuickItem* root, const QString& name) {
    QList<QQuickItem*> result;
    if (root->objectName() == name)
        result.append(root);
    for (auto* child : root->childItems())
        result.append(visual_items(child, name));
    return result;
}

inline QQuickItem* visual_item(QQuickItem* root, const QString& name) {
    if (root->objectName() == name)
        return root;
    for (auto* child : root->childItems()) {
        if (auto* item = visual_item(child, name))
            return item;
    }
    return nullptr;
}
