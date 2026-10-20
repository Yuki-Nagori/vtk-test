# geometry STEP 冒烟夹具（任务 009）

断言值见 `native/geometry/CMakeLists.txt` 消费的测试
`tests/cpp/geometry/step_import_test.cpp`；修改夹具必须同一 commit 同步测试与下表。

| 文件 | 来源 | 单位 | 预期摘要（毫米） |
|---|---|---|---|
| `box_mm.step` | `BRepPrimAPI_MakeBox(0,0,0 → 10,20,30)` 经 OCCT 8.0.1 `STEPControl_Writer`（`STEPControl_AsIs`，`write.step.unit=MM`）写出 | `SI_UNIT(.MILLI.,.METRE.)` | source_unit=Millimetre、系数 1、bbox `(0,0,0)-(10,20,30)`、solids=1、faces=6、edges=12、候选根全部转移 |
| `box_metre.step` | 同上，尺寸 `1000×1000×1000`，`write.step.unit=M` | `SI_UNIT($,.METRE.)` | source_unit=Metre、系数 1000、bbox `(0,0,0)-(1000,1000,1000)`、solids=1、faces=6、edges=12、候选根全部转移 |
| `empty.step` | 手写零字节文件 | 无 | 导入返回 kParseFailed；只读夹具，不使用共享临时路径 |
| `invalid_text.step` | 手写纯文本（非 STEP） | 无 | 导入返回结构化失败（kParseFailed），进程不得异常退出 |

包围盒断言为名义角点 ± 1e-6 mm：摘要报告的是含形状公差（Bnd_Box gap，
典型 `Precision::Confusion` ≈ 1e-7 mm）的保守包络，策略记录在
`step_import.hpp` 契约注释。

无效样例不存在"合法 STEP 之外的部分语义"；空文件使用固定零字节夹具，避免并行测试覆盖或删除同名临时文件。
