#include "object_tree_model.hpp"

#include <utility>

namespace panta::bridge {

namespace {

bool same_structure(const std::vector<ObjectTreeModel::Node>& left,
                    const std::vector<ObjectTreeModel::Node>& right) {
    if (left.size() != right.size())
        return false;
    for (std::size_t index = 0; index < left.size(); ++index) {
        if (left[index].id != right[index].id || left[index].parent != right[index].parent)
            return false;
    }
    return true;
}

bool same_content(const ObjectTreeModel::Node& left, const ObjectTreeModel::Node& right) {
    return left.kind == right.kind && left.label == right.label && left.assetId == right.assetId &&
           left.visible == right.visible && left.canToggleVisibility == right.canToggleVisibility &&
           left.info == right.info;
}

} // namespace

void ObjectTreeModel::setNodes(std::vector<Node> nodes) {
    if (same_structure(m_nodes, nodes)) {
        bool changed = false;
        for (std::size_t index = 0; index < nodes.size(); ++index) {
            if (same_content(m_nodes[index], nodes[index]))
                continue;
            m_nodes[index] = std::move(nodes[index]);
            const QModelIndex item = createIndex(m_rows[index], 0, static_cast<quintptr>(index));
            emit dataChanged(item, item);
            changed = true;
        }
        if (changed)
            emit nodesChanged();
        return;
    }
    beginResetModel();
    m_nodes = std::move(nodes);
    rebuild_children();
    endResetModel();
    emit nodesChanged();
}

void ObjectTreeModel::rebuild_children() {
    m_roots.clear();
    m_children.assign(m_nodes.size(), {});
    m_rows.assign(m_nodes.size(), 0);
    for (std::size_t index = 0; index < m_nodes.size(); ++index) {
        const int parent = m_nodes[index].parent;
        // 父节点越界或不在子节点之前时按根节点处理，避免错误输入破坏索引。
        auto& siblings = parent >= 0 && static_cast<std::size_t>(parent) < index
                             ? m_children[static_cast<std::size_t>(parent)]
                             : m_roots;
        if (&siblings == &m_roots)
            m_nodes[index].parent = -1;
        m_rows[index] = static_cast<int>(siblings.size());
        siblings.push_back(static_cast<int>(index));
    }
}

const std::vector<int>& ObjectTreeModel::children_of(int parent) const {
    return parent < 0 ? m_roots : m_children[static_cast<std::size_t>(parent)];
}

int ObjectTreeModel::find(const QString& id) const {
    for (std::size_t index = 0; index < m_nodes.size(); ++index) {
        if (m_nodes[index].id == id)
            return static_cast<int>(index);
    }
    return -1;
}

QModelIndex ObjectTreeModel::indexOfNode(const QString& id) const {
    const int node = find(id);
    if (node < 0)
        return {};
    return createIndex(m_rows[static_cast<std::size_t>(node)], 0, static_cast<quintptr>(node));
}

QString ObjectTreeModel::nodeIdAt(const QModelIndex& index) const {
    if (!index.isValid() || index.model() != this || index.internalId() >= m_nodes.size())
        return {};
    return m_nodes[index.internalId()].id;
}

QVariantMap ObjectTreeModel::node(const QString& id) const {
    const int index = find(id);
    if (index < 0)
        return {};
    const auto& item = m_nodes[static_cast<std::size_t>(index)];
    return {
        {QStringLiteral("id"), item.id},
        {QStringLiteral("kind"), item.kind},
        {QStringLiteral("label"), item.label},
        {QStringLiteral("assetId"), item.assetId},
        {QStringLiteral("visible"), item.visible},
        {QStringLiteral("canToggleVisibility"), item.canToggleVisibility},
        {QStringLiteral("info"), item.info},
    };
}

QModelIndex ObjectTreeModel::index(int row, int column, const QModelIndex& parent) const {
    if (column != 0 || row < 0)
        return {};
    const int parentNode = parent.isValid() ? static_cast<int>(parent.internalId()) : -1;
    const auto& siblings = children_of(parentNode);
    if (static_cast<std::size_t>(row) >= siblings.size())
        return {};
    return createIndex(row, 0, static_cast<quintptr>(siblings[static_cast<std::size_t>(row)]));
}

QModelIndex ObjectTreeModel::parent(const QModelIndex& child) const {
    if (!child.isValid())
        return {};
    const int parentNode = m_nodes[child.internalId()].parent;
    if (parentNode < 0)
        return {};
    return createIndex(m_rows[static_cast<std::size_t>(parentNode)], 0,
                       static_cast<quintptr>(parentNode));
}

int ObjectTreeModel::rowCount(const QModelIndex& parent) const {
    if (parent.column() > 0)
        return 0;
    const int parentNode = parent.isValid() ? static_cast<int>(parent.internalId()) : -1;
    return static_cast<int>(children_of(parentNode).size());
}

int ObjectTreeModel::columnCount(const QModelIndex& /*parent*/) const { return 1; }

QVariant ObjectTreeModel::data(const QModelIndex& index, int role) const {
    if (!index.isValid() || index.internalId() >= m_nodes.size())
        return {};
    const auto& item = m_nodes[index.internalId()];
    switch (role) {
    case Qt::DisplayRole:
        return item.label;
    case NodeIdRole:
        return item.id;
    case KindRole:
        return item.kind;
    case AssetIdRole:
        return item.assetId;
    case NodeVisibleRole:
        return item.visible;
    case CanToggleVisibilityRole:
        return item.canToggleVisibility;
    case InfoRole:
        return item.info;
    default:
        return {};
    }
}

QHash<int, QByteArray> ObjectTreeModel::roleNames() const {
    return {
        {Qt::DisplayRole, "display"},
        {NodeIdRole, "nodeId"},
        {KindRole, "kind"},
        {AssetIdRole, "assetId"},
        {NodeVisibleRole, "nodeVisible"},
        {CanToggleVisibilityRole, "canToggleVisibility"},
        {InfoRole, "info"},
    };
}

} // namespace panta::bridge
