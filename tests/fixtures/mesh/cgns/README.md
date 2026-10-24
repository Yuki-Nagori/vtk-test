# CGNS 导入夹具（任务 032）

由 Debian `libcgns3.4`（CGNS 3.4.0，HDF5 后端）经同目录 `gen.c` 写出，`strip_offsets.c` 直接用
libhdf5 删除 `ElementStartOffset` 链接以模拟 CGNS 3.3 及更早的 MIXED 布局；生成命令见 `make.sh`
（会对每个文件跑 `cgnscheck`，只有命名 / DataClass 警告）。断言见
`crates/panta-io/src/import/formats/cgns/tests.rs`。

| 文件 | 内容 | 预期 |
|---|---|---|
| `mixed.cgns` | `gen mixed 2`：Zone `blockHex`（2×2×2 HEXA_8）与 `blockMixed`（MIXED + `ElementStartOffset`，16 个 PENTA_6，x 平移 2）；各带 QUAD_4 面 section `Bottom` 与 FaceCenter PointRange BC `bottomWall`；`NodeSol`（Vertex：`Pressure` = x + 10y、`VelocityX/Y/Z` = 1 / z / 1）与 `CellSol`（CellCenter：`CellIndex`） | 54 点、8 hex + 16 wedge、2 区域、体积 16；组 `blockHex/bottomWall`、`blockMixed/bottomWall`、`unassigned` 三角形 8 / 8 / 80；`Velocity` 合成矢量 |
| `mixed_v3.cgns` | 同上，删去 `/Base/blockMixed/Mixed/ElementStartOffset` | 与 `mixed.cgns` 结果完全一致 |
| `tetpyr.cgns` | `gen tetpyr`：单 Zone `cube`，单位立方体拆成 3 个共顶点 PYRA_5，另有 x ∈ [2, 3] 的 TETRA_4；面 section `cubeBottom` / `cubeTop`，Vertex PointList BC `top`（节点 5–8）；CellCenter `Rank` = 10 / 20 / 30 / 40、`MomentumX` | 3 pyramid + 1 tetra，体积 7/6；组 `top` / `cubeBottom` / `unassigned` 三角形 2 / 2 / 12 |
| `ngon.cgns` | `gen ngon`：两个单位立方体，NGON_n 11 个面（第 5、9 面反向存储）+ NFACE_n（反向面以负号引用）；FaceCenter PointList BC `inlet`（面 1） | 2 个多面体，体积 2；组 `inlet` / `unassigned` 三角形 2 / 18 |
| `adf.cgns` | `cgnsconvert -a tetpyr.cgns`（ADF 后端） | 拒绝并提示先转换为 HDF5 |
| `transient.cgns` | `gen transient`（任务 033）：Zone `a`、`b` 各一个 TETRA_4；`BaseIterativeData` 3 步，TimeValues 0 / 0.5 / 1；`ZoneIterativeData` 的 `FlowSolutionPointers`（CellCenter `T` = 100·步 + Zone 序号）与 `FlowSolutionVertexPointers`（Vertex `P` = 步 + x）；Zone `a` 另有未被引用的解 `Initial`，Zone `b` 第 2 步为 `Null` | 3 个时间步；0.5 步 Zone `b` 的值为 NaN；`Initial` 记一条忽略说明 |
