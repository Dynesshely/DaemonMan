//! 「启动应用」节点的实现。
//!
//! 负责：已运行则跳过、启动、启动后缓冲、等待主窗口、关闭启动画面、置前窗口。
//! 「已运行则跳过」由调用方传入**有效开关**（`workflow.skip_existing_processes
//! && spec.skip_if_running`），为 false 时必定真正启动。
//! 所有分支都会给出中文 `message`，失败时返回带上下文的 `Err`。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};

use crate::engine::executor::{is_cancelled, sleep_cancellable};
use crate::model::AppLaunchSpec;
use crate::platform::{self, proc};

#[derive(Debug, Clone, Default)]
pub struct LaunchOutcome {
    pub pid: Option<u32>,
    pub already_running: bool,
    pub message: String,
}

/// 执行一次应用启动。
///
/// `skip_existing` 是**有效**的「已运行则跳过」开关：调用方负责把
/// `workflow.skip_existing_processes && spec.skip_if_running` 合进来
/// （见 [`crate::engine::executor::skip_existing_enabled`]）。为 false 时即使进程
/// 已在运行也必须真正启动。
pub async fn launch_spec(
    spec: &AppLaunchSpec,
    cancel: &Arc<AtomicBool>,
    skip_existing: bool,
) -> Result<LaunchOutcome> {
    if spec.path.trim().is_empty() {
        bail!("启动路径为空，无法启动");
    }

    let process_name = resolve_process_name(spec);

    // 1. 已经在运行则跳过（默认行为，避免重复拉起常驻程序）。
    //    只有「有效开关为真」时才去查进程，避免无谓的枚举开销。
    let is_running = if skip_existing && !process_name.is_empty() {
        platform::is_process_running(&process_name)
    } else {
        false
    };
    if should_skip(skip_existing, &process_name, is_running) {
        return Ok(LaunchOutcome {
            pid: None,
            already_running: true,
            message: format!("「{process_name}」已在运行，跳过启动"),
        });
    }

    if is_cancelled(cancel) {
        bail!("已取消");
    }

    // 2. 真正启动。
    let raw_pid = platform::launch_app(spec)
        .map_err(|err| anyhow::anyhow!("启动「{}」失败：{err}", spec.path))?;
    let pid = if raw_pid == 0 { None } else { Some(raw_pid) };
    let mut message = match pid {
        Some(pid) => format!("已启动「{process_name}」（pid={pid}）"),
        None => format!("已启动「{process_name}」"),
    };

    // 3. 启动后的固定缓冲。
    if spec.settle_ms > 0 {
        if !sleep_cancellable(Duration::from_millis(spec.settle_ms), cancel).await {
            bail!("已取消（「{process_name}」已启动，未做后续处理）");
        }
        message.push_str(&format!("，等待 {} 毫秒", spec.settle_ms));
    }

    // 4. 等待主窗口。
    if spec.wait_for_window {
        if process_name.is_empty() {
            bail!("无法从路径「{}」推导进程名，不能等待主窗口", spec.path);
        }
        let timeout_ms = if spec.window_timeout_ms == 0 {
            15_000
        } else {
            spec.window_timeout_ms
        };
        let appeared = platform::wait_for_window(
            &process_name,
            true,
            Duration::from_millis(timeout_ms),
            cancel,
        )
        .await;
        if !appeared {
            if is_cancelled(cancel) {
                bail!("已取消（等待「{process_name}」主窗口期间）");
            }
            bail!("等待「{process_name}」主窗口出现超时（{timeout_ms} 毫秒）");
        }
        message.push_str("，主窗口已出现");
    }

    // 5. 关闭启动画面 / 托盘类程序的主窗口。
    if spec.close_main_window {
        if platform::close_main_window(&process_name) {
            message.push_str("，已请求关闭其主窗口");
        } else {
            message.push_str("，但未能关闭其主窗口（可能已关闭或没有窗口）");
        }
    }

    // 6. 置前窗口。
    if spec.focus_window {
        if platform::focus_main_window(&process_name) {
            message.push_str("，主窗口已置前");
        } else {
            message.push_str("，但未能置前主窗口");
        }
    }

    Ok(LaunchOutcome {
        pid,
        already_running: false,
        message,
    })
}

/// 是否因为「已在运行」而跳过启动。
///
/// 三个条件同时成立才跳过：有效开关为真、进程名可推导、进程确实在运行。
/// 工作流级开关关闭时 `skip_existing` 为 false，这里必然是 false。
fn should_skip(skip_existing: bool, process_name: &str, is_running: bool) -> bool {
    skip_existing && !process_name.trim().is_empty() && is_running
}

/// 进程名：优先使用显式的 `process_name`，否则从启动路径推导（不含 .exe）。
pub fn resolve_process_name(spec: &AppLaunchSpec) -> String {
    match spec
        .process_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => name.to_string(),
        None => proc::process_name_from_path(&spec.path).trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_name_prefers_explicit_override() {
        let spec = AppLaunchSpec {
            path: r"C:\Apps\WeChat.exe".to_string(),
            process_name: Some("  Weixin  ".to_string()),
            ..Default::default()
        };
        assert_eq!(resolve_process_name(&spec), "Weixin");
    }

    #[test]
    fn process_name_falls_back_to_path_stem() {
        let spec = AppLaunchSpec {
            path: r"C:\Apps\WeChat.exe".to_string(),
            process_name: Some("   ".to_string()),
            ..Default::default()
        };
        assert_eq!(resolve_process_name(&spec), "WeChat");
    }

    #[test]
    fn process_name_is_empty_for_empty_path() {
        let spec = AppLaunchSpec::default();
        assert!(resolve_process_name(&spec).is_empty());
    }

    #[test]
    fn should_skip_requires_effective_switch() {
        // 工作流级开关为 false 时即使进程在跑也绝不跳过。
        assert!(!should_skip(false, "WeChat", true));
        assert!(should_skip(true, "WeChat", true));
        assert!(!should_skip(true, "WeChat", false));
        assert!(!should_skip(true, "   ", true));
    }
}
