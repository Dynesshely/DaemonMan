//! 工作流引擎：DAG 校验、拓扑调度、节点执行。
//!
//! `graph` 负责图算法，`executor` 负责一次运行的调度与状态广播，`launcher` 封装
//! 单个「启动应用」节点的实际行为（跳过已运行、等待窗口、关闭启动画面等）。

pub mod executor;
pub mod graph;
pub mod launcher;

// `spawn_group_run` 供 commands 层的 `run_single_group` 使用（Lead 负责接入），
// 接入前先允许「未使用」以免产生告警。
#[allow(unused_imports)]
pub use executor::{spawn_group_run, spawn_single_node_run, spawn_workflow_run, RunRequest};
pub use graph::validate;
