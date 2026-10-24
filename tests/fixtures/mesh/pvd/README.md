# `.pvd` 时间序列夹具（任务 033）

由 `gen.py` 生成（纯 Python，无外部依赖）。网格为 1 个六面体 + 顶部 1 个四面体（9 节点）。

| 文件 | 内容 |
|---|---|
| `flow.pvd` | 3 步（timestep 1 / 0 / 0.5 故意乱序列出），引用 `steps/flow_{0,1,2}.vtu`；节点场 `T = 100·t + 节点号`，单元场 `p = t + 单元号`；只有 t = 0 带单元场 `init` |
| `moving.pvd` | 2 步，`steps/moved_1.vtu` 的顶点 z 从 2 改为 2.5：拓扑不一致，导入整体拒绝 |

断言见 `crates/panta-io/src/import/formats/pvd/tests.rs` 与 `crates/panta-core/src/project/results.rs`。
