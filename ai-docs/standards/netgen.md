# Netgen 网格生成适配

更新日期：2026-10-20。Netgen 6.2.2604 制品消费、OCC 网格适配与调用侧销毁 workaround 已存在；SDK 修复进入消费基线后的清理条件见下文兼容记录，历史验收不作为本仓库通过证据。

适用于 `native/mesh/src/netgen/`（适配层与 `geometry/src/occt`、`visualization/src/vtk` 同构：公共契约在模块 `include/panta/<module>/`，第三方头收敛在适配层目录）。只接入网格生成，不因此把 NGSolve 求解能力带入本仓库。

## 官方依据

Netgen 上游说明其网格生成及几何内核连接能力。[Netgen upstream](https://github.com/NGSolve/netgen)

官方 OCC 教程展示几何到网格的流程，并说明 fuse、glue、compound 对内部界面/分区的不同影响。教程包含 Python/NGSolve 展示代码，不等同于本项目 C++ API 选型。[OCC Geometry Tutorial](https://docu.ngsolve.org/latest/i-tutorials/unit-4.4-occ/occ.html)

## 职责边界（2026-09-23 项目约定）

Rust 拥有网格领域数据、业务校验与任务提交；C++ adapter 实际调用 Netgen 生成算法，索引 / 分组 / 节点顺序归一化属于接口转换。 当前实现与迁移项见 [重库适配审计](../architecture/native-domain-boundaries.md)，执行 [分层规则](layering.md)；规划不代表已落地。

## 项目规则

- 所锁定版本的 C++ 接口、导出 targets 和 OCC 支持以制品头文件与实际适配器为准；不从 Python 示例推断 C++ 签名，也不照搬文档中的 NGSolve solver 依赖。
- Netgen 和 OCCT 使用经过验证的一致依赖组合。记录网格库构建选项，特别是 OCC、线程、Python 与 GUI 相关选项；不编入无用途的额外 UI。
- 输出立刻转换到自有 Mesh IR；明确索引起点、局部节点顺序、单元类型、材料区和边界标签的映射。
- 生成参数包含单位与范围；输入采用几何修订快照。生成失败或取消时不覆盖已有网格。
- 首期串行提交网格任务，只有确认库实例与全局状态的线程约束后才启用并发。无法安全中断的阶段应明确报告，不强杀线程。
- 为保证材料区边界，几何操作选择须保留所需内部面；不能把融合后的单一实体误当成多个独立区域。
- 先完成可追踪转换，再根据测量优化复制。显示用表面网格和求解用体网格各有用途，不能相互替代。

## 集成验证与消费约定

- 当前锁定 Netgen v6.2.2604 的 OCC 名称表可能借用 `FaceDescriptor` 字符串，`Mesh::DeleteMesh()` 与析构又会重复释放部分名称。适配器的 `COMPAT(task 001; remove-task 002)` 先摘除描述符别名，再清空已释放槽位；mesh 销毁后才释放 OCC 几何。该调用侧处理依赖 SDK 私有布局，保留条件和退出验证登记在[Netgen 兼容清理](../task/002-netgen-cleanup-compatibility.md)。四面体方向按有向体积归一化，STEP 解析异常必须在适配边界转换为结构化状态。历史平台验证不代替本仓库的回归。

## 验证

用简单闭合实体生成小型体网格，检查索引、正体积/方向、边界分组和参数单位；构造失败场景验证旧资产不受影响。质量阈值属于项目/分析需求，不能把一次生成成功当成所有求解场景适用。

## 制品升级项（2026-09-20 登记，2026-09-24 更新）

锁定的 v6.2.2604 SDK 存在 OCC mesh 销毁所有权问题，不能只凭上游版本号推断已修复。仅在替代 SDK 已确认修复并被本仓库实际消费后，才恢复普通 `Ng_DeleteMesh`；必须重新验证重复生成与失败回收，并由升级任务明确所需的内存诊断方案及结果。当前消费已发布 Release，不通过本仓库 workflow 重产 SDK。