//! 工作台对象树投影（task 037，034「对象树」）：把工程导入记录与活动文档的显示状态
//! 投影成先序扁平节点表，供 Bridge 的 `QAbstractItemModel` 直接消费。
//!
//! 纯函数、无副作用：节点 ID 只由稳定资产 ID 派生，输入相同则输出相同；
//! 可见性来自宿主的会话显示状态，不读写工程数据。
use crate::bridge::{ObjectTreeInfo, ObjectTreeInput, ObjectTreeNode, ProjectImport};

/// 节点类型的稳定标识；QML 按此选择属性页与图标。
pub mod kind {
    pub const PROJECT: &str = "project";
    pub const ASSET: &str = "asset";
    pub const SECTION: &str = "section";
    pub const FIELD: &str = "field";
    pub const TIME_STEPS: &str = "timeSteps";
    pub const QUALITY: &str = "quality";
    pub const ANIMATION: &str = "animation";
}

/// 先序扁平节点表：`parent` 为父节点下标（根为 -1），子节点紧随父节点之后。
/// 未打开工程时返回空表，由界面显示空状态。
pub(crate) fn object_tree_nodes(input: &ObjectTreeInput) -> Vec<ObjectTreeNode> {
    if !input.project_open {
        return Vec::new();
    }
    let mut nodes = vec![ObjectTreeNode {
        id: kind::PROJECT.to_owned(),
        kind: kind::PROJECT.to_owned(),
        label: input.project_name.clone(),
        info: vec![
            info("路径", &input.project_path),
            info("资产数", &input.assets.len().to_string()),
        ],
        ..node_defaults(-1, 0)
    }];
    for asset in &input.assets {
        push_asset(&mut nodes, input, asset);
    }
    if !input.animation_name.is_empty() {
        nodes.push(ObjectTreeNode {
            id: kind::ANIMATION.to_owned(),
            kind: kind::ANIMATION.to_owned(),
            label: format!("表面序列：{}", input.animation_name),
            info: vec![
                info("序列", &input.animation_name),
                info("帧数", &input.animation_frame_count.to_string()),
            ],
            ..node_defaults(0, 1)
        });
    }
    nodes
}

fn push_asset(nodes: &mut Vec<ObjectTreeNode>, input: &ObjectTreeInput, asset: &ProjectImport) {
    let parent = to_parent(nodes.len());
    let active = asset.id == input.active_asset_id;
    let mut asset_info = vec![
        info("网格类型", mesh_type_label(&asset.mesh_type)),
        info("单位", unit_label(&asset.units)),
        info("三角形数", &asset.triangle_count.to_string()),
        info(
            "尺寸",
            &panta_core::project::format_project_dimensions(
                asset.size_x,
                asset.size_y,
                asset.size_z,
                unit_label(&asset.units),
            ),
        ),
        info("来源文件", &asset.source_name),
        info("格式", &asset.format.to_uppercase()),
    ];
    if active && !input.lod_status.is_empty() {
        asset_info.push(info("显示状态", &input.lod_status));
    }
    if active && input.volume_available && !input.volume_info.is_empty() {
        asset_info.push(info("体网格", &input.volume_info));
    }
    nodes.push(ObjectTreeNode {
        id: format!("{}:{}", kind::ASSET, asset.id),
        kind: kind::ASSET.to_owned(),
        label: asset.display_name.clone(),
        asset_id: asset.id.clone(),
        visible: !input.hidden_asset_ids.iter().any(|id| id == &asset.id),
        can_toggle_visibility: true,
        info: asset_info,
        ..node_defaults(0, 1)
    });
    // 切面 / 着色 / 时间步只描述活动文档（ViewModel 只持有活动文档的这些状态）。
    if active && input.volume_available {
        nodes.push(child(
            asset,
            kind::SECTION,
            if input.section_active {
                "切面".to_owned()
            } else {
                "切面（关闭）".to_owned()
            },
            parent,
            ObjectTreeNode {
                visible: input.section_active,
                can_toggle_visibility: true,
                ..node_defaults(parent, 2)
            },
        ));
        if input.field_count > 0 {
            let label = if input.field_label.is_empty() {
                "着色：无".to_owned()
            } else {
                format!("着色：{}", input.field_label)
            };
            nodes.push(child(
                asset,
                kind::FIELD,
                label,
                parent,
                ObjectTreeNode {
                    info: vec![info("可用字段", &input.field_count.to_string())],
                    ..node_defaults(parent, 2)
                },
            ));
        }
    }
    // 时间序列只在至少两个时间步时出现（034：单步结果没有可播放的序列）。
    if active && input.time_step_count >= 2 {
        nodes.push(child(
            asset,
            kind::TIME_STEPS,
            format!("时间序列（{} 步）", input.time_step_count),
            parent,
            ObjectTreeNode {
                info: vec![info("步数", &input.time_step_count.to_string())],
                ..node_defaults(parent, 2)
            },
        ));
    }
    if input.quality_asset_id == asset.id {
        nodes.push(child(
            asset,
            kind::QUALITY,
            "质量诊断".to_owned(),
            parent,
            node_defaults(parent, 2),
        ));
    }
}

fn child(
    asset: &ProjectImport,
    node_kind: &str,
    label: String,
    parent: i32,
    base: ObjectTreeNode,
) -> ObjectTreeNode {
    ObjectTreeNode {
        id: format!("{node_kind}:{}", asset.id),
        kind: node_kind.to_owned(),
        label,
        asset_id: asset.id.clone(),
        parent,
        ..base
    }
}

fn node_defaults(parent: i32, depth: u32) -> ObjectTreeNode {
    ObjectTreeNode {
        parent,
        depth,
        visible: true,
        ..ObjectTreeNode::default()
    }
}

fn to_parent(index: usize) -> i32 {
    // 节点数受导入记录数约束，远小于 i32::MAX；饱和只为满足类型转换。
    i32::try_from(index).unwrap_or(i32::MAX)
}

fn info(label: &str, value: &str) -> ObjectTreeInfo {
    ObjectTreeInfo {
        label: label.to_owned(),
        value: value.to_owned(),
    }
}

fn unit_label(units: &str) -> &'static str {
    match units {
        "millimeters" => "mm",
        "centimeters" => "cm",
        "meters" => "m",
        _ => "in",
    }
}

fn mesh_type_label(mesh_type: &str) -> &str {
    panta_core::project::MESH_TYPES
        .iter()
        .find(|entry| entry.id == mesh_type)
        .map_or(mesh_type, |entry| entry.source_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, name: &str) -> ProjectImport {
        ProjectImport {
            id: id.to_owned(),
            display_name: name.to_owned(),
            source_name: format!("{name}.stl"),
            format: "stl".to_owned(),
            mesh_type: "solid-3d".to_owned(),
            units: "millimeters".to_owned(),
            triangle_count: 12,
            size_x: 1.0,
            size_y: 2.0,
            size_z: 3.0,
            ..ProjectImport::default()
        }
    }

    fn open_project(assets: Vec<ProjectImport>) -> ObjectTreeInput {
        ObjectTreeInput {
            project_open: true,
            project_name: "uidemo".to_owned(),
            project_path: "/tmp/uidemo.panta".to_owned(),
            assets,
            ..ObjectTreeInput::default()
        }
    }

    /// (id, parent, depth) 三元组，便于整体比较树结构。
    fn shape(nodes: &[ObjectTreeNode]) -> Vec<(String, i32, u32)> {
        nodes
            .iter()
            .map(|node| (node.id.clone(), node.parent, node.depth))
            .collect()
    }

    fn ids(nodes: &[ObjectTreeNode]) -> Vec<&str> {
        nodes.iter().map(|node| node.id.as_str()).collect()
    }

    fn find<'a>(nodes: &'a [ObjectTreeNode], id: &str) -> &'a ObjectTreeNode {
        nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap_or_else(|| panic!("missing node {id}"))
    }

    #[test]
    fn closed_project_has_no_nodes() {
        assert!(object_tree_nodes(&ObjectTreeInput::default()).is_empty());
    }

    #[test]
    fn assets_follow_import_records_in_order() {
        let mut input = open_project(vec![asset("a", "flow"), asset("b", "wing")]);
        let nodes = object_tree_nodes(&input);
        assert_eq!(
            shape(&nodes),
            vec![
                ("project".to_owned(), -1, 0),
                ("asset:a".to_owned(), 0, 1),
                ("asset:b".to_owned(), 0, 1),
            ]
        );
        assert_eq!(find(&nodes, "project").label, "uidemo");
        let flow = find(&nodes, "asset:a");
        assert_eq!(flow.label, "flow");
        assert_eq!(flow.asset_id, "a");
        assert!(flow.visible && flow.can_toggle_visibility);
        assert!(
            flow.info
                .iter()
                .any(|row| row.label == "网格类型" && row.value == "实体 3D")
        );
        assert!(
            flow.info
                .iter()
                .any(|row| row.label == "尺寸" && row.value.contains("mm"))
        );

        // 删除资产：节点与其子节点一并消失，其余顺序不变。
        input.assets.remove(0);
        assert_eq!(ids(&object_tree_nodes(&input)), vec!["project", "asset:b"]);
        // 新增资产追加在末尾。
        input.assets.push(asset("c", "duct"));
        assert_eq!(
            ids(&object_tree_nodes(&input)),
            vec!["project", "asset:b", "asset:c"]
        );
    }

    #[test]
    fn section_node_follows_active_volume_and_toggle() {
        let mut input = open_project(vec![asset("a", "flow"), asset("b", "wing")]);
        input.active_asset_id = "a".to_owned();
        // 表面网格没有切面节点。
        assert!(
            !ids(&object_tree_nodes(&input)).contains(&"section:a"),
            "surface mesh must not expose a section"
        );
        input.volume_available = true;
        let nodes = object_tree_nodes(&input);
        assert_eq!(
            shape(&nodes),
            vec![
                ("project".to_owned(), -1, 0),
                ("asset:a".to_owned(), 0, 1),
                ("section:a".to_owned(), 1, 2),
                ("asset:b".to_owned(), 0, 1),
            ]
        );
        let section = find(&nodes, "section:a");
        assert_eq!(section.label, "切面（关闭）");
        assert!(!section.visible && section.can_toggle_visibility);

        input.section_active = true;
        let section_on = object_tree_nodes(&input);
        let section = find(&section_on, "section:a");
        assert_eq!(section.label, "切面");
        assert!(section.visible);
        // 开关只改节点状态，不改树结构。
        assert_eq!(shape(&section_on), shape(&nodes));

        // 激活另一个文档：子节点跟随活动资产移动。
        input.active_asset_id = "b".to_owned();
        assert_eq!(
            ids(&object_tree_nodes(&input)),
            vec!["project", "asset:a", "asset:b", "section:b"]
        );
    }

    #[test]
    fn field_node_reflects_selected_field() {
        let mut input = open_project(vec![asset("a", "flow")]);
        input.active_asset_id = "a".to_owned();
        input.volume_available = true;
        assert!(!ids(&object_tree_nodes(&input)).contains(&"field:a"));
        input.field_count = 2;
        let nodes = object_tree_nodes(&input);
        assert_eq!(
            ids(&nodes),
            vec!["project", "asset:a", "section:a", "field:a"]
        );
        assert_eq!(find(&nodes, "field:a").label, "着色：无");
        input.field_label = "T（节点）".to_owned();
        let colored = object_tree_nodes(&input);
        assert_eq!(find(&colored, "field:a").label, "着色：T（节点）");
        assert_eq!(shape(&colored), shape(&nodes));
    }

    #[test]
    fn time_series_appears_with_two_or_more_steps() {
        let mut input = open_project(vec![asset("a", "flow")]);
        input.active_asset_id = "a".to_owned();
        input.time_step_count = 1;
        assert_eq!(ids(&object_tree_nodes(&input)), vec!["project", "asset:a"]);
        input.time_step_count = 3;
        let nodes = object_tree_nodes(&input);
        assert_eq!(
            shape(&nodes),
            vec![
                ("project".to_owned(), -1, 0),
                ("asset:a".to_owned(), 0, 1),
                ("timeSteps:a".to_owned(), 1, 2),
            ]
        );
        assert_eq!(find(&nodes, "timeSteps:a").label, "时间序列（3 步）");
        input.time_step_count = 0;
        assert_eq!(ids(&object_tree_nodes(&input)), vec!["project", "asset:a"]);
    }

    #[test]
    fn quality_and_animation_nodes() {
        let mut input = open_project(vec![asset("a", "flow"), asset("b", "wing")]);
        input.quality_asset_id = "b".to_owned();
        input.animation_name = "wave".to_owned();
        input.animation_frame_count = 4;
        let nodes = object_tree_nodes(&input);
        assert_eq!(
            shape(&nodes),
            vec![
                ("project".to_owned(), -1, 0),
                ("asset:a".to_owned(), 0, 1),
                ("asset:b".to_owned(), 0, 1),
                ("quality:b".to_owned(), 2, 2),
                ("animation".to_owned(), 0, 1),
            ]
        );
        assert_eq!(find(&nodes, "animation").label, "表面序列：wave");
        input.animation_name.clear();
        assert!(!ids(&object_tree_nodes(&input)).contains(&"animation"));
    }

    #[test]
    fn hidden_assets_only_change_visibility() {
        let mut input = open_project(vec![asset("a", "flow"), asset("b", "wing")]);
        let shown = object_tree_nodes(&input);
        input.hidden_asset_ids = vec!["b".to_owned()];
        let hidden = object_tree_nodes(&input);
        assert_eq!(shape(&hidden), shape(&shown));
        assert!(find(&hidden, "asset:a").visible);
        assert!(!find(&hidden, "asset:b").visible);
    }
}
