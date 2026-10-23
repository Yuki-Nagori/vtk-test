# mesh 表面诊断夹具（任务 006）

手写 ASCII STL，基准四面体顶点为 A(0,0,0)、B(1,0,0)、C(0,1,0)、D(0,0,1)，四个面均外法向。
断言见 `crates/panta-mesh/src/diagnostics.rs` 与 `tests/rust/cli.rs`；修改夹具须同步测试与下表。

| 文件 | 缺陷 | 默认容差（对角线 × 1e-6）下的预期 |
|---|---|---|
| `clean_tetrahedron.stl` | 无 | 4 个焊接顶点，无问题 |
| `open_tetrahedron.stl` | 缺少 BCD 面 | 3 条边界边、1 个孔 |
| `flipped_tetrahedron.stl` | BCD 面反向 | 3 条法向不一致边 |
| `non_manifold_fins.stl` | 三个面共享 AB 边 | 1 条非流形边 |
| `degenerate_faces.stl` | 干净四面体 + 重复顶点面 + 共线面 | 2 个退化面，不计入边拓扑 |
| `unwelded_tetrahedron.stl` | 一个 D 顶点偏移 1e-9 | 默认容差焊接 1 个近重复顶点；零容差时出现开放边 |
