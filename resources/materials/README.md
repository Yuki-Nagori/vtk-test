# 内置材料资源

`pp-mineral-25.json` 是内置默认材料：聚丙烯（PP），矿物填充比例 25%，版本 ID 为 `pp-mineral-25-v1`。

温度为 K、应力为 Pa、剪切速率为 s⁻¹，比热为 J/(kg·K)、导热率为 W/(m·K)，PVT 比容为 m³/kg。资源包含 Cross-WLF、Modified Tait 及比热 / 导热率表。当前 Rust 只读取该资源的版本化 ID，校验既有工程清单的材料引用；旧材料选择界面和工艺摘要投影已经移除。资源中的本构参数保留为来源数据，不代表已支持求解器适用性或完整材料模型校验。

Rust 使用 `include_str!` 内置资源，运行时不访问 `target/`。参数变化应使用新版本 ID，避免已有材料引用的含义静默变化；材料区域和已求解结果的关联设计见 [模型与结果组合](../../ai-docs/architecture/study-and-automation.md)。
