# 多格式导入夹具（任务 010）

全部由 VTK 9.7.1 Python writer 生成，期望三角形数用 `vtkDataSetSurfaceFilter + vtkTriangleFilter`
交叉得到；断言见 `crates/panta-import/src/formats/mod.rs` 与 `tests/rust/cli.rs`。

| 文件 | 来源数据 | 编码 | 期望三角形 |
|---|---|---|---|
| `tet_ball_ascii.vtu` | 球面点 Delaunay3D 四面体 | ascii，UInt32 头 | 64 |
| `tet_ball_appended_zlib.vtu` | 同上 | appended base64，zlib，UInt64 头 | 64 |
| `voxel_block_binary.vtu` | 3×2×2 ImageData → 2 个 VOXEL | inline binary | 20 |
| `voxel_block_binary_zlib.vtu` | 同上 | inline binary，zlib，UInt64 头 | 20 |
| `voxel_block_appended_raw.vtu` | 同上 | appended raw | 20 |
| `sphere_appended_raw.vtp` | SphereSource(10, 7) | appended raw | 100 |
| `sphere_binary_zlib.vtp` | 同上 | inline binary，zlib | 100 |
| `tet_ball_legacy42_ascii.vtk` | 四面体球 | 旧版 4.2 ASCII | 64 |
| `tet_ball_legacy51_binary.vtk` | 四面体球 | 旧版 5.1 BINARY（OFFSETS/CONNECTIVITY） | 64 |
| `sphere_legacy51_ascii.vtk` | 球面 | 旧版 5.1 ASCII | 100 |
| `sphere_legacy42_binary.vtk` | 球面 | 旧版 4.2 BINARY | 100 |
| `sphere.obj` | 球面 | vtkOBJWriter | 100 |
| `sphere_ascii.ply` / `sphere_binary_le.ply` | 球面 | PLY ascii / binary little endian | 100 |

所有夹具经 `mesh inspect` 诊断均为闭合、法向一致。
