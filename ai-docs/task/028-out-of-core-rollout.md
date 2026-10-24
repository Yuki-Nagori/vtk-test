# 028 — out-of-core 铺开：只读映射块读取、接入应用与大网格设置

- 状态：in-progress
- 阶段：PR 1/2（只读映射块读取）已实现；PR 2（接入应用与 026 设置）待做
- 依赖：[027](027-out-of-core-block-prototype.md)、[026](026-lod-user-settings.md)、[025](025-lod-streaming-diagnostics-export.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

027 原型已验证分块切面在 1 亿四面体下的延迟与内存。本任务把它铺开为应用能力，分两个 PR：

1. **只读映射块读取**（用户决策 2026-10-24 12:03 UTC+8：现在就加 mmap）。块读取抽象在 `ChunkSource` 接口后，映射与合并定位读都是实现：默认映射，定位读作回退。目标是减少 Windows 上的读系统调用，并为完整驻留模式提供映射读取。
2. **接入应用与 026 设置**：超过阈值的体网格激活时生成派生 `.pchunk`，切面、质量和导出走 `ChunkedVolume`。设置项包括：触发阈值、“禁用分块、完整驻留”、“按可用内存自动”、块缓存预算。Linux glibc 多 arena 内存滞留在 launcher 侧用 `MALLOC_ARENA_MAX` 处理。

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

## 前置条件与待决策

- 已决策（用户）：放开 unsafe 引入 mmap，只限一个隔离小模块。
- 已决策（本任务，保守）：映射打开失败（锁冲突、空文件、地址空间不足等）时自动回退定位读，不报错；仅 `ChunkReadMode::Mmap` 显式要求时报错。

## 实施步骤

1. PR 1：映射模块、接口改造、校验加强、测试、基准对比、nightly 与规范更新。
2. PR 2：派生分块缓存、设置、FFI / C++ / QML、launcher 环境变量、文档。

## 预计改动

- PR 1：`Cargo.toml`（workspace 依赖与 lint 注释）、`crates/panta-mesh/{Cargo.toml, src/lib.rs, src/mapped.rs（新建）, src/chunked/*, examples/ooc_bench.rs}`、`.github/workflows/nightly.yml`、`ai-docs/standards/rust.md`、`ai-docs/architecture/mesh.md`。
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

Windows 附注：文件映射存在时不能删除或替换该文件。PR 2 重建派生缓存前必须先释放对应的 `ChunkedVolume`。

## 验收标准

- [x] `unsafe` 仅在 `panta_mesh::mapped`，模块级 allow，一个 unsafe 块且带 `// SAFETY:`；rust.md 与根 Cargo.toml 注释已登记。
- [x] 映射与定位读的切面三角形集合、质量统计完全一致（单测）；映射读调用计数为 0。
- [x] 空文件、坏文件头、截断、块内翻转在两种实现下都被拒绝（单测）；他人持排他锁时自动回退定位读（单测）。
- [x] `ooc_bench --reader` 在 1 亿四面体下对比延迟、读调用、内存（见下表）；nightly Windows 小基准两种实现都跑。
- [ ] PR 2：应用接入与设置（待做）。

## 验证计划与结果

环境：box Linux x86_64，8 线程，Rust 1.99.0，`CMAKE_GENERATOR=Ninja`。

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | `cargo test --release -p panta-mesh chunked` | 全过 | 10/10 通过（新增 3 个：两实现一致、映射校验、锁冲突回退） |
| 2026-10-24 | `cargo lint clippy --check`、`cargo format` | 无告警 | 通过 |
| 2026-10-24 | nightly run 40060731130（027 合入后首次，12:02–12:06 UTC+8） | Windows 编译 `seek_read` 路径并跑小基准 | 成功。Windows 2M：打开 10.1 ms / 2 次读；冷精细 45.7 ms（5 次读）/ 53.3 ms（7 次读）；粗级别 max 3–4 ms；质量 0.15 s（2 次读）。内存在 Windows 上为 N/A |
| 2026-10-24 | `MALLOC_ARENA_MAX=2 ooc_bench --cells 100M --reuse --reader {pread,mmap} --budget-mib {1024,256}` | 映射不慢于定位读，匿名内存不增 | 见下表；日志 `/workspace/bench/mmap-compare.log`（box 本地，未入库） |

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

## 风险与回退

- 映射不可用或冷盘变慢：`ChunkReadMode::Read` 随时可切回定位读；自动模式在打开失败时已经回退。
- SIGBUS 残余风险见上表；回退方式同上。

## 决策与工作记录

- 2026-10-24：创建任务。PR 1：新增依赖 memmap2 0.9.12；unsafe 例外登记在 `panta_mesh::mapped`。`ChunkSource` 返回零拷贝 `ChunkBytes`，映射实现对合并区段做 `madvise(WILLNEED)`（仅 Unix）。打开新增文件头校验，定位读的打开读调用从 2 次变为 3 次（文件 > 1 MiB 时）。nightly 小基准改为 pread + mmap 两次运行。027 合入后首次 nightly（run 40060731130）Windows 成功。

## 完成摘要

未完成（PR 2 待做）。
