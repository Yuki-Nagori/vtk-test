//! Panta 应用核心：Rust 拥有工程、命令、后台作业与跨平台路径服务。
//! 当前工程清单契约为 schema 4，撤销 / 重做尚未接入。

mod activation;
pub mod animation;
mod derived_chunks;
mod execution;
pub mod path;
pub mod project;

/// 宿主事件循环退出、工程服务销毁后关闭写入准入并等待已接受的清单提交。
/// 不在 UI 交互或写入 worker 内调用；读取作业不阻塞此收尾。
pub fn finish_background_writes() {
    execution::writes().close_and_wait();
}
