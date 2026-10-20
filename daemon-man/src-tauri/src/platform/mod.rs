//! 平台能力层（Windows 实现）。
//!
//! 对外暴露两组能力：
//! 1. 自启动项扫描 / 开关 / 自身开机自启；
//! 2. 进程与窗口操作（启动程序、检测运行、等待窗口、关闭/置前窗口、结束进程）。
//!
//! 本模块的公开函数签名是后端其余模块的契约，实现位于 `registry` / `startup_folder` /
//! `startup_approved` / `tasks` / `services` / `proc` 子模块中。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;

use crate::model::{AppLaunchSpec, AutostartItem, ProcessInfo};

mod com;
pub mod proc;
pub mod registry;
pub mod services;
pub mod startup_approved;
pub mod startup_folder;
pub mod tasks;

/// 自启动来源在稳定 id 中使用的短名（与 `AutostartSource` 的 serde kebab-case 表示一致）。
fn source_key(source: crate::model::AutostartSource) -> &'static str {
    use crate::model::AutostartSource::*;
    match source {
        RegistryRun => "registry-run",
        RegistryRunOnce => "registry-run-once",
        StartupFolder => "startup-folder",
        ScheduledTask => "scheduled-task",
        Service => "service",
    }
}

/// 扫描系统中所有自启动项（已考虑系统层面的启用/禁用状态）。
pub fn scan_autostart() -> Vec<AutostartItem> {
    let mut items = Vec::new();
    items.extend(registry::scan());
    items.extend(startup_folder::scan());
    items.extend(tasks::scan());
    items.extend(services::scan());
    items.sort_by(|a, b| {
        a.source_label
            .cmp(&b.source_label)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    items
}

/// 按稳定 id 启用/禁用某个自启动项，返回最新状态。
pub fn set_autostart_enabled(item_id: &str, enabled: bool) -> Result<AutostartItem> {
    let items = scan_autostart();
    let item = items
        .iter()
        .find(|i| i.id == item_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("未找到自启动项：{item_id}"))?;

    if item.is_self {
        anyhow::bail!("不能通过此接口开关 DaemonMan 自身");
    }
    if !item.toggle_supported {
        anyhow::bail!("该自启动项不支持由 DaemonMan 开关");
    }

    match item.source {
        crate::model::AutostartSource::RegistryRun
        | crate::model::AutostartSource::RegistryRunOnce => {
            registry::set_enabled(&item, enabled)?;
        }
        crate::model::AutostartSource::StartupFolder => {
            startup_folder::set_enabled(&item, enabled)?;
        }
        crate::model::AutostartSource::ScheduledTask => {
            tasks::set_enabled(&item, enabled)?;
        }
        crate::model::AutostartSource::Service => {
            services::set_enabled(&item, enabled)?;
        }
    }

    // 以「重新扫描后的真实状态」为准，而不是假定修改一定生效：
    // 个别项（例如登录触发器被单独禁用的计划任务）表面启用、实际不会运行，
    // 此时应把差异如实报告给上层，而不是掩盖成「已切换」。
    match scan_autostart().into_iter().find(|i| i.id == item_id) {
        Some(updated) => {
            if updated.enabled != enabled {
                anyhow::bail!(
                    "系统未接受「{}」的自启动状态修改（可能需要管理员权限，或该计划任务的登录触发器已被单独禁用）",
                    updated.name
                );
            }
            Ok(updated)
        }
        None => {
            let mut fallback = item;
            fallback.enabled = enabled;
            Ok(fallback)
        }
    }
}

/// DaemonMan 自身是否已设置开机自启。
pub fn is_self_autostart_enabled() -> bool {
    registry::is_self_enabled()
}

/// 设置 / 取消 DaemonMan 自身的开机自启。
pub fn set_self_autostart(enabled: bool) -> Result<bool> {
    registry::set_self_enabled(enabled)?;
    Ok(enabled)
}

/// 当前进程是否具备管理员权限。
pub fn is_elevated() -> bool {
    proc::is_elevated()
}

/// 启动一个程序，返回其 pid（0 表示无法获得 pid）。
pub fn launch_app(spec: &AppLaunchSpec) -> Result<u32> {
    proc::launch_app(spec)
}

/// 进程是否在运行（按进程名，不含扩展名）。
pub fn is_process_running(name: &str) -> bool {
    proc::is_process_running(name)
}

/// 列出所有进程（`managed` 字段由上层填充）。
pub fn list_processes() -> Vec<ProcessInfo> {
    proc::list_processes()
}

/// 结束进程。
pub fn kill_process(pid: u32) -> Result<()> {
    proc::kill_process(pid)
}

/// 等待进程主窗口出现。`require_visible` 为 false 时进程存在即返回。
pub async fn wait_for_window(
    process_name: &str,
    require_visible: bool,
    timeout: Duration,
    cancel: &Arc<AtomicBool>,
) -> bool {
    proc::wait_for_window(process_name, require_visible, timeout, cancel).await
}

/// 向主窗口发送 WM_CLOSE。
pub fn close_main_window(process_name: &str) -> bool {
    proc::close_main_window(process_name)
}

/// 把主窗口置前。
pub fn focus_main_window(process_name: &str) -> bool {
    proc::focus_main_window(process_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};

    /// 冒烟测试：真实扫描本机自启动项并打印来源分布。
    ///
    /// 手动运行：`cargo test --lib scan_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scan_smoke() {
        let items = scan_autostart();
        println!("自启动项总数 = {}", items.len());

        let mut by_source: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for item in &items {
            let entry = by_source.entry(item.source_label.as_str()).or_default();
            entry.0 += 1;
            if !item.enabled {
                entry.1 += 1;
            }
        }
        for (source, (total, disabled)) in &by_source {
            println!("  {source}: {total} 项（已禁用 {disabled}）");
        }

        let unique_ids: HashSet<&str> = items.iter().map(|item| item.id.as_str()).collect();
        println!("唯一 id 数 = {}", unique_ids.len());

        // 每种来源各抽几条打印，方便人工确认字段是否合理。
        let mut printed: HashSet<&str> = HashSet::new();
        for item in items.iter() {
            let source = item.source_label.as_str();
            if !printed.insert(source) {
                continue;
            }
            println!("--- {source} 示例 ---");
            for sample in items.iter().filter(|i| i.source_label == source).take(6) {
                println!(
                    "  - {} | enabled={} self={} toggle={} admin={}",
                    sample.name,
                    sample.enabled,
                    sample.is_self,
                    sample.toggle_supported,
                    sample.requires_admin
                );
                println!("      id: {}", sample.id);
                println!("      命令: {}", sample.command);
                if let Some(detail) = sample.detail.as_deref() {
                    println!("      说明: {detail}");
                }
            }
        }

        // id 必须在同一次扫描内唯一，否则 set_autostart_enabled 会定位到错误条目。
        assert_eq!(
            unique_ids.len(),
            items.len(),
            "自启动项 id 出现重复：{:?}",
            {
                let mut seen = HashSet::new();
                items
                    .iter()
                    .filter(|item| !seen.insert(item.id.as_str()))
                    .map(|item| item.id.as_str())
                    .collect::<Vec<_>>()
            }
        );
    }

    /// 冒烟测试：自身自启动读写（会真实写 HKCU，跑完自动恢复原状态）。
    #[test]
    #[ignore]
    fn self_autostart_smoke() {
        let before = is_self_autostart_enabled();
        println!("修改前 is_self_autostart_enabled = {before}");
        set_self_autostart(true).expect("写入自身自启动失败");
        assert!(is_self_autostart_enabled());
        set_self_autostart(false).expect("删除自身自启动失败");
        assert!(!is_self_autostart_enabled());
        set_self_autostart(before).expect("恢复原状态失败");
        println!("已恢复为 {before}");
    }
}
