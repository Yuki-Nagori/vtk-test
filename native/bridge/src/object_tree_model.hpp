/// 工作台对象树的 Qt 模型投影（task 037）。
///
/// 节点由 Rust `object_tree_nodes` 计算（先序扁平表）；本类型只负责把它适配为
/// `QAbstractItemModel` 供 QML TreeView 使用，不派生领域状态。结构不变时只发
/// dataChanged（播放等高频刷新不重置展开状态），结构变化时整体重置。
#pragma once

#include <QAbstractItemModel>
#include <QString>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>
#include <vector>

namespace panta::bridge {

class ObjectTreeModel : public QAbstractItemModel {
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("由 ProjectViewModel.objectTree 提供")
    Q_PROPERTY(int nodeCount READ nodeCount NOTIFY nodesChanged)

  public:
    enum Role {
        NodeIdRole = Qt::UserRole + 1,
        KindRole,
        AssetIdRole,
        NodeVisibleRole,
        CanToggleVisibilityRole,
        InfoRole,
    };
    Q_ENUM(Role)

    struct Node {
        QString id;
        QString kind;
        QString label;
        QString assetId;
        /// 父节点在扁平表中的下标；根为 -1。
        int parent = -1;
        bool visible = true;
        bool canToggleVisibility = false;
        /// `[{label, value}]`，属性面板「信息」页按序显示。
        QVariantList info;
    };

    using QAbstractItemModel::QAbstractItemModel;

    /// 替换节点表；父节点必须位于子节点之前（Rust 先序输出保证）。
    void setNodes(std::vector<Node> nodes);

    [[nodiscard]] int nodeCount() const { return static_cast<int>(m_nodes.size()); }
    /// 按节点 ID 取 TreeView 可用的索引；不存在时返回无效索引。
    Q_INVOKABLE QModelIndex indexOfNode(const QString& id) const;
    /// 索引对应的节点 ID；无效索引返回空串（TreeView 当前项 → 属性面板选中）。
    Q_INVOKABLE QString nodeIdAt(const QModelIndex& index) const;
    /// 属性面板使用的节点快照：id / kind / label / assetId / visible / canToggleVisibility / info；
    /// 不存在时返回空映射。
    Q_INVOKABLE QVariantMap node(const QString& id) const;

    [[nodiscard]] QModelIndex index(int row, int column,
                                    const QModelIndex& parent = {}) const override;
    [[nodiscard]] QModelIndex parent(const QModelIndex& child) const override;
    [[nodiscard]] int rowCount(const QModelIndex& parent = {}) const override;
    [[nodiscard]] int columnCount(const QModelIndex& parent = {}) const override;
    [[nodiscard]] QVariant data(const QModelIndex& index, int role) const override;
    [[nodiscard]] QHash<int, QByteArray> roleNames() const override;

  signals:
    void nodesChanged();

  private:
    [[nodiscard]] int find(const QString& id) const;
    [[nodiscard]] const std::vector<int>& children_of(int parent) const;
    void rebuild_children();

    std::vector<Node> m_nodes;
    /// 每个节点在父节点下的行号，以及各节点（含虚拟根 -1）的子节点下标表。
    std::vector<int> m_rows;
    std::vector<int> m_roots;
    std::vector<std::vector<int>> m_children;
};

} // namespace panta::bridge
