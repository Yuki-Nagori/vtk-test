# OpenFOAM 导入夹具（任务 032）

由 OpenFOAM v1912（Debian `openfoam 1912.200626`）生成，脚本见同目录 `make.sh`（ascii / binary / poly）
与 `multi.sh`（多区域）。只保留 `constant/polyMesh`、结果时间目录与标记文件；日志、`processor*` 与
中间时间步已删除。断言见 `crates/panta-io/src/import/formats/openfoam/tests.rs` 与
`crates/panta-core/src/project/results.rs`。

| 目录 | 来源 | 编码 | 网格（checkMesh） | 结果场 |
|---|---|---|---|---|
| `ascii` | cavity 改 3D（3×2×2 blockMesh，scale 0.1），icoFoam 两步 | ascii | 12 hex，3 个 wall patch | `0/`、`0.01/` 的 `p`（m²/s²）、`U`（m/s），`phi` 为 surfaceScalarField（不导入） |
| `binary` | 同 `ascii`，foamFormatConvert | binary | 同上 | `0.01/{U,p}` |
| `poly` | 3×3×2 blockMesh 后 `polyDualMesh 80` | ascii + gzip | 160 点、224 面、12 hex + 36 polyhedra；非正交 0，最大偏斜 1.0000001 | `0.01/{U,p}.gz` |
| `skew` | 斜六面体块 4×3×2 blockMesh（simpleGrading 3 1 0.5）后 `perturb.py points 0.006` 扰动全部点 | ascii | 24 hex，98 面 / 46 内部面；非正交 最大 32.663972 / 平均 21.620442，最大偏斜 1.4240861 | `0/{U,p}` |
| `skewdual` | `skew` 再 `polyDualMesh 80` | ascii | 189 点、272 面 / 133 内部面、14 hex + 46 polyhedra；非正交 最大 41.626226 / 平均 21.297343，最大偏斜 2.4449241 | `0/{U,p}` |
| `multi` | 4×2×1 blockMesh，topoSet + `splitMeshRegions -cellZones` | ascii | `fluid` / `solid` 两区域各 4 hex，含 `*_to_*` 界面 patch | `0/<区域>/{T,U,cellToRegion}`；T 按区域 300 / 350 / 400 K |

`poly` 的对偶网格面含共线的边中点；导入时对这类面换扇形起点（见任务 032 决策记录）。
