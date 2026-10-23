# 012 — 移除 Mesh sidecar v2 只读兼容

- 状态：planned
- 阶段：清理
- 依赖：[011](011-mesh-asset-chunked-storage.md)；需用户确认旧工程支持策略
- 优先级：P2
- 负责人：待分配
- 创建 / 更新：2026-10-24 / 2026-10-24

## 范围与保留原因

task 011 起新导入写 sidecar v3；为保证此前保存的工程可激活，保留 v2 只读解码。待决策：是否提供显式迁移命令（如 `project migrate-assets`，重写为 v3 并更新清单摘要），或直接宣布不再支持 v2 工程。在用户确认前不实施。

## 退出与验收

- [ ] 确认旧工程策略（迁移命令或放弃支持）。
- [ ] 删除所有 `COMPAT(task 011; remove-task 012)` 标记代码：`crates/panta-mesh/src/asset/v2.rs`、`is_readable_mesh_asset_version` 的版本 2 分支、`mesh_asset_encoded_len` 的 v2 分派、v2 夹具与相关测试。
- [ ] v2 sidecar 激活返回 `project.asset_unsupported_format`，有测试覆盖。
- [ ] fmt、clippy、`cargo test` 通过。

## 验证记录

2026-10-24：仅登记，未实施。
