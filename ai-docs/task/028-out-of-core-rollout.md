# 028 — out-of-core 铺开：只读映射块读取、接入应用与大网格设置

- 状态：done
- 阶段：PR 1/2（只读映射块读取）已合入；PR 2/2（接入应用与 026 设置）已实现
- 依赖：[027](027-out-of-core-block-prototype.md)、[026](026-lod-user-settings.md)、[025](025-lod-streaming-diagnostics-export.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

027 原型已验证分块切面在 1 亿四面体下的延迟与内存。本任务把它铺开为应用能力，分两个 PR：

1. **只读映射块读取**（用户决策 2026-10-24 12:03 UTC+8：现在就加 mmap）。块读取抽象在 `ChunkSource` 接口后，映射与合并定位读都是实现：默认映射，定位读作回退。目标是减少 Windows 上的读系统调用。映射只用于分块按需读；完整驻留模式不用映射（见 PR 2 用户决策）。
2. **接入应用与 026 设置**：超过阈值的体网格激活时生成派生 `.pchunk`，切面和质量走 `ChunkedVolume`（导出仍读源资产，见 PR 2 设计）。设置项包括：触发阈值、“禁用分块、完整驻留”、“按可用内存自动”、块缓存预算。Linux glibc 多 arena 内存滞留在应用入口用 `mallopt(M_ARENA_MAX, 2)` 处理（等价于 `MALLOC_ARENA_MAX=2`）。

## 必读

- [Rust 规范](../standards/rust.md)（unsafe 例外登记）、[Cargo 规范](../standards/cargo.md)、[提交规范](../standards/commits.md)、[注释规范](../standards/comments.md)、[测试规范](../standards/testing.md)
- [网格架构](../architecture/mesh.md) out-of-core 一节
- memmap2 0.9.12 文档（docs.rs/memmap2，2026-10-24 查阅）：`Mmap::map` 是 unsafe，安全前提是映射期间文件不被修改或截断；`advise_range(Advice::WillNeed)` 仅 Unix。
- Rust 1.99 std：`File::try_lock_shared`（Unix `flock`，Windows `LockFileEx`），`std::os::windows::fs::OpenOptionsExt::share_mode`。

## 范围与非目标

- PR 1：`panta_mesh::mapped::MappedFile`（唯一 unsafe 模块）、`MmapChunkSource`、`ChunkReadMode {Auto, Mmap, Read}`、`open_chunk_source`、零拷贝 `ChunkBytes`、`IoCounts` 结构；`ooc_bench --reader`；nightly Windows 小基准跑两种实现。
- PR 2：应用接入与设置（见目标）。
- 非目标：远程块源；完全流式（不驻留原网格）的分块构建（候选 task 5“超大体文件流式导入”）；Windows `PrefetchVirtualMemory` 预读（memmap2 无安全接口，需另开 unsafe，暂不做）。

## 涉及分组与跨组依赖

PR 1 只改 `panta-mesh`（领域库），新增第三方依赖 `memmap2`（MIT/Apache-2.0，Unix 依赖 libc，Windows 无额外依赖），不新增跨组依赖。

PR 2 沿既有方向接线：`panta-mesh`（`build_chunked_from_asset`、`plane_through_nodes`）→ `panta-core`（激活、派生缓存、工程服务）→ `panta-ffi`（CXX 桥）→ `native/qt-adapter`（可用物理内存）→ `native/bridge`（ViewModel、QSettings）→ QML 设置对话框；`native/app` 入口加 `mallopt`。不新增第三方依赖。

## 前置条件与待决策

- 已决策（用户）：放开 unsafe 引入 mmap，只限一个隔离小模块。
- 已决策（用户，2026-10-24 12:31 UTC+8）：“禁用分块、完整驻留”模式不用 mmap，用 MiB 级缓冲大块顺序读一次性读入进程自有内存，读完立即关闭文件、释放锁，不保留映射；mmap 只用于分块按需读，继续保持锁和校验。
- 已决策（本任务，保守）：映射打开失败（锁冲突、空文件、地址空间不足等）时自动回退定位读，不报错；仅 `ChunkReadMode::Mmap` 显式要求时报错。

## 实施步骤

1. PR 1：映射模块、接口改造、校验加强、测试、基准对比、nightly 与规范更新。
2. PR 2：派生分块缓存、设置、FFI / C++ / QML、应用入口 `mallopt`、文档。

## 预计改动

- PR 1：`Cargo.toml`（workspace 依赖与 lint 注释）、`crates/panta-mesh/{Cargo.toml, src/lib.rs, src/mapped.rs（新建）, src/chunked/*, examples/ooc_bench.rs}`、`.github/workflows/nightly.yml`、`ai-docs/standards/rust.md`、`ai-docs/architecture/mesh.md`。
- PR 2：`crates/panta-mesh/src/chunked/{build.rs, volume.rs, mod.rs, tests.rs}`、`crates/panta-core/src/{activation.rs, derived_chunks.rs（新建）, lib.rs, project/*}`、`crates/panta-ffi/src/{lib.rs, limits.rs, display.rs}`、`native/qt-adapter`（`system_memory`，新建）、`native/bridge/src/{large_mesh_settings.cpp, project_view_model.*}`、`native/app/main.cpp`、`qml/Dialogs/LargeMeshSettingsDialog.qml`、C++ / QML 测试、`ai-docs/{architecture/mesh.md, standards/rust.md, task-index.md}`。`setLargeMeshLimits` 由 3 参数改为 5 参数，唯一调用方为设置对话框，不保留旧签名。
- `ChunkSource::read_at` 返回 `ChunkBytes`（原为 `Vec<u8>`），`io_counts` 返回 `IoCounts`（原为三元组）。均为 crate 内部新接口（027 刚合入，无外部调用方），不保留旧签名。

## 清理与兼容例外

无废弃项；无兼容例外。`.pchunk` 格式未变（仍为版本 1），打开时新增文件头 magic / 版本校验。

## 外部修改 / 截断的防护（PR 1）

| 层 | 措施 |
|---|---|
| 打开 | 只读打开。Windows 共享模式只给 `FILE_SHARE_READ`：已有写句柄时打开失败，打开后他人不能写、删、改名。整文件共享锁 `try_lock_shared`：Windows 为强制锁，Unix 为建议锁；他人持排他锁时映射失败，自动回退定位读 |
| 打开校验 | 文件长度 ≥ 头 + 尾；尾 magic / 版本；块表区间必须正好结束在 `len - 32`；文件头 magic / 版本（新增，一次 16 B 读，小文件在尾窗口内则免）；表面段与每块单元区间都在文件内 |
| 每块校验 | 每块是带校验和的 zstd 帧，解压后核对节点 / 单元计数；`read_at` 越界返回错误 |
| 残余风险 | Unix 上不遵守建议锁的外部进程截断文件时，访问已失效页会触发 SIGBUS（进程崩溃）。用户态无法完全消除，已在 rust.md 登记。本项目写入方都写临时文件再 rename，不就地改写已发布文件；PR 2 的派生缓存放在应用缓存目录，同样只 rename 发布 |

Windows 附注：他人持排他锁时，Windows 强制锁会让定位读回退也失败（`ERROR_LOCK_VIOLATION`，os error 33），打开以 IO 错误结束，不会读到半写数据；Unix 建议锁不拦读，自动模式回退定位读。两种行为都由单测按平台断言（nightly Windows 首跑发现，已修）。文件映射存在时不能删除或替换该文件。PR 2 因此从不重写已有的有效派生文件，清理时删除失败直接忽略。

## 验收标准

- [x] `unsafe` 仅在 `panta_mesh::mapped`，模块级 allow，一个 unsafe 块且带 `// SAFETY:`；rust.md 与根 Cargo.toml 注释已登记。
- [x] 映射与定位读的切面三角形集合、质量统计完全一致（单测）；映射读调用计数为 0。
- [x] 空文件、坏文件头、截断、块内翻转在两种实现下都被拒绝（单测）；他人持排他锁时映射拒绝打开：Unix 自动回退定位读，Windows 因强制锁以 IO 错误失败（单测按平台断言，nightly Windows 验证）。
- [x] `ooc_bench --reader` 在 1 亿四面体下对比延迟、读调用、内存（见下表）；nightly Windows 小基准两种实现都跑。
- [x] PR 2：超过有效阈值的体网格激活后生成 / 复用派生 `.pchunk`，质量与切面走分块并与驻留路径一致（`chunked_streamed_commands_match_resident_commands`、`plane_through_nodes_matches_resident_section_position`）；从 v3 资产构建与从驻留网格构建字节一致（`build_from_v3_asset_matches_build_from_resident_mesh`）。
- [x] PR 2：派生文件只建一次、重启复用、内存不足跳过、按时间保留最新 4 个（`derived_chunks_are_built_once_reused_and_gated_by_memory`、`derived_chunk_cache_keeps_the_newest_files`）。
- [x] PR 2：三种模式与块缓存预算可在设置中修改、校验、持久化，旧设置文件缺键时取默认（Rust / FFI / C++ ViewModel 测试）；可用物理内存查询在 Linux 返回合理值（qt-adapter 测试，nightly 覆盖 macOS / Windows）。
- [x] PR 2：完整驻留模式用 8 MiB 缓冲顺序读，不映射；`mapped.rs` SAFETY 注释与本文写明 UB 风险、缓解与 Unix 残余风险。

## 验证计划与结果

环境：box Linux x86_64，8 线程，Rust 1.99.0，`CMAKE_GENERATOR=Ninja`。

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | `cargo test --release -p panta-mesh chunked` | 全过 | 10/10 通过（新增 3 个：两实现一致、映射校验、锁冲突回退） |
| 2026-10-24 | `cargo lint clippy --check`、`cargo format` | 无告警 | 通过 |
| 2026-10-24 | nightly run 40060731130（027 合入后首次，12:02–12:06 UTC+8） | Windows 编译 `seek_read` 路径并跑小基准 | 成功。Windows 2M：打开 10.1 ms / 2 次读；冷精细 45.7 ms（5 次读）/ 53.3 ms（7 次读）；粗级别 max 3–4 ms；质量 0.15 s（2 次读）。内存在 Windows 上为 N/A |
| 2026-10-24 | 分支 nightly run 40061488592，之后修复并重跑 run 40061921092（12:19–12:27 UTC+8） | Windows 编译映射模块，跑单测和两种实现的小基准 | 首次失败：锁冲突单测撞上 Windows 强制锁语义，已修。重跑三平台全部成功。Windows 2M（热页缓存）：打开 pread 11.4 ms / 3 次读，mmap 10.7 ms / 0 次读；oblique 冷精细 49.0 → 47.5 ms；axis_z 55.8 → 50.9 ms；拖动读调用 18 / 8 → 0；质量 0.156 → 0.147 s。内存为 N/A |
| 2026-10-24 | `MALLOC_ARENA_MAX=2 ooc_bench --cells 100M --reuse --reader {pread,mmap} --budget-mib {1024,256}` | 映射不慢于定位读，匿名内存不增 | 见下表；日志 `/workspace/bench/mmap-compare.log`（box 本地，未入库） |
| 2026-10-24 | PR 2：`cargo test --locked --workspace`（CTest 68/68）、clippy / qmllint / format、offscreen 启动 | 全过 | 通过；新增 Rust 测试 6 项、C++ ViewModel 设置断言、qt-adapter 可用内存测试 |
| 2026-10-24 | PR 2 分支 nightly run 40063506975（12:44–12:47 UTC+8） | macOS / Windows 编译 `system_memory`（`host_statistics64` / `GlobalMemoryStatusEx`）、`mallopt` 条件编译，CTest 全过 | 成功，两平台全部通过 |

1 亿四面体（99,488,250），文件 0.84 GiB，页缓存热（box 无 root 无法清缓存，冷盘差异未测）：

| 指标 | pread 1024 MiB | mmap 1024 MiB | pread 256 MiB | mmap 256 MiB |
|---|---|---|---|---|
| 打开 | 98.0 ms，3 次读 | 88.8 ms，0 次读 / 3 次映射访问 | 119.5 ms | 88.2 ms |
| oblique 冷精细 | 408 ms，28 次读 | 315 ms，0 次读 | 347 ms | 300 ms |
| axis_z 冷精细 | 278 ms，32 次读 | 216 ms | 267 ms | 247 ms |
| oblique 逐步精细 p50 / max | 104 / 177 ms | 101 / 162 ms | 266 / 320 ms | 222 / 247 ms |
| axis_z 逐步精细 p50 / max | 73 / 190 ms | 69 / 179 ms | 177 / 264 ms | 162 / 248 ms |
| 拖动粗级别 max | 30.9 / 23.3 ms | 20.8 / 23.7 ms | 19.6 / 27.0 ms | 20.8 / 16.2 ms |
| 质量首次 | 2.38 s，19 次读 | 1.89 s，0 次读 | 2.58 s，25 次读 | 2.00 s |
| 读调用合计（各阶段） | ≈ 680 | 0 | ≈ 2,550 | 0 |
| 峰值 RSS（VmHWM） | 2.54 GiB | 3.08 GiB | 1.24 GiB | 1.82 GiB |
| 结束时匿名 / 文件页 RSS | 2.41 GiB / 3.5 MiB | 2.22 GiB / 868 MiB | 1.05 GiB / 3.3 MiB | 0.91 GiB / 868 MiB |

结论：

- 映射在所有阶段持平或更快，冷精细快 5–25%，质量快约 22%；读系统调用归零。
- 匿名内存（真正占用）持平或略低，因为少了一份读缓冲。
- VmHWM 看起来更高，是因为它把映射到的文件页（最多整个文件，可回收）也算进去了；定位读同样占用这么多 OS 页缓存，只是不计入进程 RSS。
- Windows 任务管理器默认显示“内存（专用工作集）”，不含映射文件页；“工作集”列则包含。

### Windows 待测项（nightly 只能部分覆盖）

- 映射在 Windows 冷盘（非页缓存）下的缺页 IO 粒度（通常 4–64 KiB 簇）与合并定位读（MiB 级单次读）的延迟对比。如果冷盘映射更慢，PR 2 可把 Windows 默认改为定位读，或者以后另开 unsafe 用 `PrefetchVirtualMemory` 预读。
- Windows 进程内存（工作集 / 专用字节）在 nightly 中仍为 N/A。
- Defender 实时扫描对映射与读两种方式的影响。

## PR 2 设计

### 激活路径

1. 有效阈值：`VolumeMode::Threshold` 用手设阈值；`AutoMemory` 用 `可用物理内存 / 2 / 190 B`（190 B 为完整驻留每四面体的估计占用），限在阈值合法范围内，查询不到（0）时退回手设阈值；`Resident` 为无穷大，始终完整驻留。
2. 超过阈值：先走 025 流式路径只驻留外表面，激活即可显示；随后在同一后台任务里调用 `derived_chunks::open_or_build`：
   - 目录为 `QStandardPaths::CacheLocation/volume-chunks`，文件名 `<资产 SHA-256>.pchunk`。摘要覆盖资产目录，资产变了文件名就变，已有有效文件（打开校验通过且四面体 / 节点数一致）直接复用，从不重写。
   - 生成前估算内存：四面体 × 48 B + 节点 × 24 B + 边界三角 × 16 B。超过可用内存一半时跳过（参考：1 亿四面体合成数据仅分块构建峰值 RSS 1.82 GiB，约 18 B / 四面体；应用路径还要驻留源四面体与节点，48 B 是保守值）。
   - 构建写临时文件（8 MiB 缓冲）再 rename，期间响应取消；其他进程同时生成了有效文件时保留对方的。失败、取消、内存不足都不影响激活，回退 025 流式诊断与切面。
   - 清理：按修改时间（复用时会 touch）保留最新 4 个；超过 24 小时的残留临时文件删除；Windows 上正被映射的文件删不掉时忽略。
3. 分块存在时，质量与切面走 `ChunkedVolume`；切面平面用 `plane_through_nodes`，与驻留路径 `section_plane_origin` 位级一致。导出仍读 v3 源资产（导出需要完整原始单元顺序，块文件不是导出源）。
4. 块缓存预算修改后立即通过 `ChunkCache::set_budget` 作用于已打开的分块体网格；模式与阈值只影响之后的激活。
5. 宿主在激活、关闭、打开导入记录前刷新可用物理内存：Windows `GlobalMemoryStatusEx`，macOS `host_statistics64`（free + inactive + purgeable），Linux `/proc/meminfo` 的 `MemAvailable`。导入完成后自动激活沿用最近一次的值。

### 完整驻留模式的读取

“禁用分块、完整驻留”与低于阈值的体网格走驻留路径：`BufReader::with_capacity(8 MiB)` 顺序读 v3 资产到进程自有内存，读完立即关闭文件（锁随句柄释放），不保留任何映射。

### 只读映射的 UB 风险与缓解

映射把文件内容暴露为 `&[u8]`。映射存活期间文件被任何进程**修改或截断**都是未定义行为：修改违反共享引用内容不变的前提；截断后访问越界页在 Unix 上 SIGBUS、在 Windows 上访问冲突，进程崩溃。缓解措施：

| 措施 | 作用 |
|---|---|
| 整文件共享锁，持有到映射释放 | 他人持排他锁时映射打开失败，回退定位读 |
| Windows 共享模式只给 `FILE_SHARE_READ`，不给 `FILE_SHARE_WRITE` / `FILE_SHARE_DELETE`；`LockFileEx` 为强制锁 | Windows 上其他进程无法写入、截断、删除或改名，风险在 OS 层排除 |
| 打开时校验长度、头尾与块表；每次读取校验区间不越过文件长度 | 不按未校验偏移访问映射 |
| 每块 zstd 帧校验和 + 解压后节点 / 单元计数 | 内容不一致返回错误，不继续使用 |
| 写入方只写临时文件再 rename | 本项目从不就地改写已发布文件；rename 不影响已打开的映射 |

**残余风险（Unix）**：`flock` 是建议锁，拦不住不合作的进程。外部程序或用户手工就地改写、截断正在映射的派生文件时，校验只能发现已读入的不一致，无法阻止并发修改；截断后访问相应页仍会 SIGBUS。用户态无法消除，派生文件放在应用缓存目录、不面向用户编辑，以降低触发概率。

### Linux glibc arena

应用入口在未设置 `MALLOC_ARENA_MAX` 环境变量时调用 `mallopt(M_ARENA_MAX, 2)`（仅 `__GLIBC__`）。原因：分块读取与 Rayon 线程各自分配 MiB 级缓冲，glibc 默认每核最多 8 个 arena，释放后的块滞留在各 arena，RSS 长期偏高（027 实测见该任务）。上限 2 风险低：本应用热点是少量大块分配，arena 竞争影响小；用户显式设置环境变量时尊重用户。Windows（系统堆）与 macOS（libmalloc）不受影响。

### 需要 GUI 验收 / 推迟项

- 设置对话框新增的模式下拉框与块缓存字段只有 offscreen 冒烟与 QML 组件测试，真实窗口观感、窄窗口布局未验收。
- 切面拖动时的粗级别预览（027 的体素 + 简化外表面）尚未接入视口：应用目前在松手后显示精细切面。接入需要视口交互与 GPU 验收，推迟（不属于本次批准范围的新增工作，记为候选，未实施）。
- 派生文件生成期间没有单独的进度显示（与激活共用后台任务）；大文件首次激活会多花分块构建时间（1 亿四面体约 13 s 量级，box 实测为合成数据）。

## 风险与回退

- 映射不可用或冷盘变慢：`ChunkReadMode::Read` 随时可切回定位读；自动模式在打开失败时已经回退。
- SIGBUS 残余风险见上文；回退方式同上。
- 派生缓存异常（磁盘满、权限、损坏）：生成失败不影响激活，回退 025 流式；用户可删除缓存目录 `volume-chunks` 强制重建。
- 自动模式估计偏差：190 B / 四面体与 48 B / 四面体都是估计值，用户可改用手设阈值或完整驻留。

## 决策与工作记录

- 2026-10-24：PR 2。用户决策（12:31 UTC+8）完整驻留不用 mmap，改为 8 MiB 缓冲顺序读。派生缓存放在应用缓存目录（不入工程，可随时重建），生成门槛为可用内存一半；导出不走分块（保守，保持导出源唯一）。glibc 用 `mallopt` 而不是 launcher 环境变量：应用入口一处生效，不影响子进程。
- 2026-10-24：分支 nightly run 40061488592 的 Windows 跑出锁冲突单测失败。原因是 Windows 强制锁也拒绝定位读，属于预期语义，单测改为按平台断言。
- 2026-10-24：创建任务。PR 1：新增依赖 memmap2 0.9.12；unsafe 例外登记在 `panta_mesh::mapped`。`ChunkSource` 返回零拷贝 `ChunkBytes`，映射实现对合并区段做 `madvise(WILLNEED)`（仅 Unix）。打开新增文件头校验，定位读的打开读调用从 2 次变为 3 次（文件 > 1 MiB 时）。nightly 小基准改为 pread + mmap 两次运行。027 合入后首次 nightly（run 40060731130）Windows 成功。

## 完成摘要

- PR 1：`panta_mesh::mapped` 只读映射、`MmapChunkSource` 默认、定位读回退、IO 计数、nightly 三平台通过。
- PR 2：体网格三种模式 + 块缓存预算设置（Rust 校验、FFI、QSettings 持久化、QML 对话框）；派生分块缓存（按资产摘要命名、只建一次、内存门槛、保留最新 4 个）；分块质量与切面接入工程服务；完整驻留改为 8 MiB 顺序读、不映射；可用物理内存宿主查询；glibc arena 上限；UB 风险与残余风险写入 SAFETY 注释与本文。
- 未做（推迟，需 GUI / GPU）：拖动粗级别切面预览接入视口；派生文件生成进度单独显示。
