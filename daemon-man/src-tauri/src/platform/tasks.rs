//! 计划任务（登录触发器）扫描与启停。
//!
//! 使用 Task Scheduler 2.0 的 COM 接口（`ITaskService` / `ITaskFolder` /
//! `IRegisteredTask`）而不是 `schtasks` 的本地化文本输出：
//!
//! - 从 `\` 根目录开始递归遍历（深度受限），跳过 `\Microsoft\` 下的系统任务；
//! - 只保留「有登录触发器（TASK_TRIGGER_LOGON）」的任务；
//! - `enabled` 取任务真实状态（`IRegisteredTask::Enabled`）与登录触发器状态的与；
//! - 开关优先 `IRegisteredTask::put_Enabled`，失败（常见于权限不足）时回退
//!   `schtasks /Change /TN <完整路径> /ENABLE|/DISABLE`，参数按独立 argv 传递，
//!   任务名里的空格与中文不需要额外转义。

use anyhow::{bail, Context, Result};
use std::os::windows::process::CommandExt;
use std::path::Path;

use windows::core::{BSTR, Interface, VARIANT};
use windows::Win32::Foundation::VARIANT_BOOL;
use windows::Win32::System::TaskScheduler::*;

use crate::model::{AutostartItem, AutostartSource};

/// Task Scheduler 2.0 的 CLSID。windows crate 只提供了旧版 `CLSID_CTaskScheduler`，
/// 新版（ITaskService）的 CLSID 需要自己声明。
const CLSID_TASK_SCHEDULER: windows::core::GUID =
    windows::core::GUID::from_u128(0x0f87369f_a4e5_4cfc_bd3e_73e6154572dd);

/// 递归遍历的最大层数（正常系统不会超过 3~4 层）。
const MAX_FOLDER_DEPTH: usize = 8;

/// `schtasks.exe` 回退时不弹出控制台窗口。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 一个「登录时触发」的计划任务。
struct TaskRecord {
    name: String,
    /// 任务所在文件夹（`\` 或 `\Backup`），用于再次定位
    folder: String,
    /// 完整任务路径（`\TaskName` / `\Backup\TaskName`）
    full_path: String,
    action_path: String,
    action_args: Option<String>,
    enabled: bool,
    logon_user: Option<String>,
    /// 任务主体（运行身份），用于判断开关是否需要管理员权限
    principal_user: Option<String>,
}

/// 扫描登录时触发的计划任务（排除 `\Microsoft\` 系统任务）。
pub fn scan() -> Vec<AutostartItem> {
    match scan_com() {
        Ok(records) => records.into_iter().map(record_to_item).collect(),
        Err(err) => {
            eprintln!("[platform] 扫描计划任务失败，已跳过任务类自启动项：{err:#}");
            Vec::new()
        }
    }
}

fn scan_com() -> Result<Vec<TaskRecord>> {
    // COM 对象必须在 guard 作用域内创建与销毁。
    let _apartment = super::com::init_apartment()?;

    let service: ITaskService = super::com::create_instance(&CLSID_TASK_SCHEDULER)
        .context("创建 Task Scheduler COM 对象失败")?;
    unsafe {
        service
            .Connect(
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
            )
            .context("连接本机任务计划服务失败")?
    };

    let root: ITaskFolder = unsafe { service.GetFolder(&BSTR::from("\\")) }
        .context("打开任务计划根目录失败")?;

    let mut records = Vec::new();
    let mut pending: Vec<(ITaskFolder, usize)> = vec![(root, 0)];
    while let Some((folder, depth)) = pending.pop() {
        let folder_path = unsafe { folder.Path() }
            .map(|path| path.to_string())
            .unwrap_or_else(|_| "\\".to_string());
        if is_system_folder(&folder_path) {
            continue;
        }

        collect_folder_tasks(&folder, &folder_path, &mut records);

        if depth >= MAX_FOLDER_DEPTH {
            continue;
        }
        let children = match unsafe { folder.GetFolders(0) } {
            Ok(children) => children,
            Err(err) => {
                eprintln!(
                    "[platform] 枚举计划任务子目录 {} 失败，已跳过：{err}",
                    folder_path
                );
                continue;
            }
        };
        let count = unsafe { children.Count() }.unwrap_or(0);
        for index in 1..=count {
            match unsafe { children.get_Item(&VARIANT::from(index)) } {
                Ok(child) => pending.push((child, depth + 1)),
                Err(err) => eprintln!(
                    "[platform] 读取计划任务子目录（{folder_path} 第 {index} 项）失败，已跳过：{err}"
                ),
            }
        }
    }
    Ok(records)
}

/// 是否是 Windows 自带的 `\Microsoft\...` 系统任务目录。
fn is_system_folder(path: &str) -> bool {
    let normalized = path.trim_start_matches('\\').to_ascii_lowercase();
    normalized == "microsoft" || normalized.starts_with("microsoft\\")
}

fn collect_folder_tasks(folder: &ITaskFolder, folder_path: &str, out: &mut Vec<TaskRecord>) {
    let tasks = match unsafe { folder.GetTasks(0) } {
        Ok(tasks) => tasks,
        Err(err) => {
            eprintln!(
                "[platform] 枚举文件夹 {folder_path} 中的计划任务失败，已跳过：{err}"
            );
            return;
        }
    };
    let count = match unsafe { tasks.Count() } {
        Ok(count) => count,
        Err(err) => {
            eprintln!(
                "[platform] 读取文件夹 {folder_path} 中计划任务数量失败，已跳过：{err}"
            );
            return;
        }
    };
    for index in 1..=count {
        match read_task(&tasks, index, folder_path) {
            Ok(Some(record)) => out.push(record),
            // 没有登录触发器的任务不是「登录自启动」，静默忽略。
            Ok(None) => {}
            Err(err) => eprintln!(
                "[platform] 跳过计划任务（{folder_path} 第 {index} 项）：{err:#}"
            ),
        }
    }
}

fn read_task(
    tasks: &IRegisteredTaskCollection,
    index: i32,
    folder_path: &str,
) -> Result<Option<TaskRecord>> {
    let task: IRegisteredTask = unsafe { tasks.get_Item(&VARIANT::from(index)) }
        .with_context(|| format!("读取第 {index} 个任务失败"))?;

    let full_path = unsafe { task.Path() }
        .map(|path| path.to_string())
        .unwrap_or_default();
    if is_system_folder(&full_path) {
        return Ok(None);
    }
    let name = unsafe { task.Name() }
        .map(|name| name.to_string())
        .unwrap_or_default();

    let task_enabled = unsafe { task.Enabled() }
        .map(|value| value.0 != 0)
        .unwrap_or(false);
    let definition = unsafe { task.Definition() }.context("读取任务定义失败")?;

    // ---- 触发器：必须至少有一个登录触发器 ----
    let triggers = unsafe { definition.Triggers() }.context("读取任务触发器失败")?;
    let mut trigger_count = 0i32;
    unsafe { triggers.Count(&mut trigger_count) }.context("读取触发器数量失败")?;
    let mut has_logon_trigger = false;
    let mut logon_trigger_enabled = false;
    let mut logon_user: Option<String> = None;
    for index in 1..=trigger_count {
        let trigger: ITrigger = match unsafe { triggers.get_Item(index) } {
            Ok(trigger) => trigger,
            Err(err) => {
                eprintln!("[platform] 读取任务「{full_path}」的触发器失败：{err}");
                continue;
            }
        };
        let mut trigger_type = TASK_TRIGGER_TYPE2(0);
        if unsafe { trigger.Type(&mut trigger_type) }.is_err() {
            continue;
        }
        if trigger_type != TASK_TRIGGER_LOGON {
            continue;
        }
        has_logon_trigger = true;
        let mut enabled = VARIANT_BOOL(0);
        if unsafe { trigger.Enabled(&mut enabled) }.is_ok() && enabled.0 != 0 {
            logon_trigger_enabled = true;
        }
        if let Ok(logon) = trigger.cast::<ILogonTrigger>() {
            // UserId 是 out 参数（*mut BSTR），空串表示「任意用户」。
            let mut user = BSTR::new();
            if unsafe { logon.UserId(&mut user) }.is_ok() {
                let user = user.to_string();
                if !user.trim().is_empty() {
                    logon_user = Some(user);
                }
            }
        }
    }
    if !has_logon_trigger {
        return Ok(None);
    }

    // ---- 动作：取第一个 Exec 动作（ComHandler / SendEmail 无法映射成可执行命令）----
    let actions = unsafe { definition.Actions() }.context("读取任务动作失败")?;
    let mut action_count = 0i32;
    unsafe { actions.Count(&mut action_count) }.context("读取动作数量失败")?;
    let mut action_path = String::new();
    let mut action_args: Option<String> = None;
    for index in 1..=action_count {
        let action: IAction = match unsafe { actions.get_Item(index) } {
            Ok(action) => action,
            Err(err) => {
                eprintln!("[platform] 读取任务「{full_path}」的动作失败：{err}");
                continue;
            }
        };
        let mut action_type = TASK_ACTION_TYPE(0);
        if unsafe { action.Type(&mut action_type) }.is_err() {
            continue;
        }
        if action_type != TASK_ACTION_EXEC {
            continue;
        }
        if let Ok(exec) = action.cast::<IExecAction>() {
            // IExecAction 的 Path / Arguments 同样是 out 参数形式。
            let mut path = BSTR::new();
            if unsafe { exec.Path(&mut path) }.is_ok() {
                action_path = path.to_string();
            }
            let mut args = BSTR::new();
            if unsafe { exec.Arguments(&mut args) }.is_ok() {
                let args = args.to_string();
                if !args.trim().is_empty() {
                    action_args = Some(args);
                }
            }
            break;
        }
    }

    // 任务主体（运行身份）：与自己不同的（SYSTEM / 其他用户）通常需要提权才能改。
    let principal_user = unsafe { definition.Principal() }
        .ok()
        .and_then(|principal| {
            let mut user = BSTR::new();
            unsafe { principal.UserId(&mut user) }.ok().map(|_| user.to_string())
        })
        .filter(|user| !user.trim().is_empty());

    Ok(Some(TaskRecord {
        name,
        folder: folder_path.to_string(),
        full_path,
        action_path,
        action_args,
        // 任务本身被禁用、或登录触发器被禁用，都不会在登录时运行。
        enabled: task_enabled && logon_trigger_enabled,
        logon_user,
        principal_user,
    }))
}

/// 任务主体是否是当前登录用户（决定开关是否需要提权）。
fn principal_is_current_user(user: &str) -> bool {
    let user = user.trim();
    if user.is_empty() {
        return false;
    }
    let Ok(current) = std::env::var("USERNAME") else {
        return false;
    };
    if current.trim().is_empty() {
        return false;
    }
    // UserId 可能是 `DOMAIN\user` / `MACHINE\user`，也可能只有一个名字。
    let leaf = user.rsplit('\\').next().unwrap_or(user);
    leaf.eq_ignore_ascii_case(current.trim()) || user.eq_ignore_ascii_case(current.trim())
}

fn record_to_item(record: TaskRecord) -> AutostartItem {
    let TaskRecord {
        name,
        folder,
        full_path,
        action_path,
        action_args,
        enabled,
        logon_user,
        principal_user,
    } = record;

    let command = if action_path.trim().is_empty() {
        String::new()
    } else if let Some(args) = action_args.as_deref() {
        format!("\"{action_path}\" {args}")
    } else {
        format!("\"{action_path}\"")
    };
    let executable = super::registry::resolve_executable(&action_path);

    let mut details = vec![if enabled {
        "登录时触发".to_string()
    } else {
        "登录时触发（当前已禁用）".to_string()
    }];
    if let Some(user) = logon_user.as_deref() {
        details.push(format!("登录用户：{user}"));
    }
    if let Some(user) = principal_user.as_deref() {
        details.push(format!("运行身份：{user}"));
    }
    details.push(format!("任务路径：{full_path}"));
    if action_path.trim().is_empty() {
        details.push("没有可执行的动作（可能是 COM 处理程序）".to_string());
    }
    if action_path.contains("svchost.exe") || action_path.contains("schtasks") {
        details.push("系统辅助进程".to_string());
    }

    // 以当前用户身份运行的任务，其 Enable/Disable 一般不需要提权；
    // SYSTEM / 其他用户的任务才需要管理员。
    let requires_admin = principal_user
        .as_deref()
        .map(|user| !principal_is_current_user(user))
        .unwrap_or(true);
    if requires_admin {
        details.push("运行身份不是当前用户，开关需要管理员权限".to_string());
    }

    // daemon-man 自身若被注册成登录任务，也标记出来。
    let is_self = executable
        .as_deref()
        .is_some_and(super::proc::is_current_exe);

    AutostartItem {
        id: format!(
            "{}::{folder}::{name}",
            super::source_key(AutostartSource::ScheduledTask)
        ),
        name: if name.trim().is_empty() {
            full_path.clone()
        } else {
            name.clone()
        },
        source: AutostartSource::ScheduledTask,
        source_label: AutostartSource::ScheduledTask.label().to_string(),
        command,
        executable,
        args: action_args,
        enabled,
        // 计划任务可以整任务 Enable/Disable。
        toggle_supported: true,
        can_delete: true,
        requires_admin,
        is_self,
        taken_over: false,
        scope: folder,
        locator: name,
        detail: Some(details.join("；")),
    }
}

/// 启用 / 禁用计划任务。
pub fn set_enabled(item: &AutostartItem, enabled: bool) -> Result<()> {
    // 优先走 COM；失败再回退 schtasks（本地化文本只影响输出，不影响参数）。
    match set_enabled_com(item, enabled) {
        Ok(()) => Ok(()),
        Err(com_err) => match run_schtasks(item, enabled) {
            Ok(()) => Ok(()),
            Err(fallback_err) => bail!(
                "切换计划任务「{}」失败：通过任务计划服务修改失败（{com_err:#}）；\
                 改用 schtasks 回退也失败（{fallback_err:#}）。\
                 系统级/其他用户创建的任务需要以管理员身份运行 DaemonMan",
                item.name
            ),
        },
    }
}

fn set_enabled_com(item: &AutostartItem, enabled: bool) -> Result<()> {
    let _apartment = super::com::init_apartment()?;
    let service = connect_service()?;
    let folder = folder_for(&service, &folder_path_of(item))?;
    let task = task_for(&folder, item)?;

    let flag = VARIANT_BOOL(if enabled { -1 } else { 0 });
    unsafe { task.SetEnabled(flag) }
        .with_context(|| format!("设置计划任务「{}」启用状态失败", item.name))
}

// ---------------------------------------------------------------------------
// 备份 / 删除 / 还原：导出任务 XML、删除任务、从 XML 重新注册
// ---------------------------------------------------------------------------

fn connect_service() -> Result<ITaskService> {
    let service: ITaskService = super::com::create_instance(&CLSID_TASK_SCHEDULER)
        .context("创建 Task Scheduler COM 对象失败")?;
    unsafe {
        service
            .Connect(
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
            )
            .context("连接本机任务计划服务失败")?
    };
    Ok(service)
}

fn folder_path_of(item: &AutostartItem) -> String {
    if item.scope.trim().is_empty() {
        "\\".to_string()
    } else {
        item.scope.clone()
    }
}

fn folder_for(service: &ITaskService, folder_path: &str) -> Result<ITaskFolder> {
    unsafe { service.GetFolder(&BSTR::from(folder_path)) }
        .with_context(|| format!("打开任务文件夹失败：{folder_path}"))
}

fn task_for(folder: &ITaskFolder, item: &AutostartItem) -> Result<IRegisteredTask> {
    unsafe { folder.GetTask(&BSTR::from(item.locator.as_str())) }
        .with_context(|| format!("找到计划任务失败：{}", item.locator))
}

/// 导出一项计划任务的完整 XML（备份用）。
pub(crate) fn export_xml(item: &AutostartItem) -> Result<String> {
    let _apartment = super::com::init_apartment()?;
    let service = connect_service()?;
    let folder = folder_for(&service, &folder_path_of(item))?;
    let task = task_for(&folder, item)?;
    let xml = unsafe { task.Xml() }.with_context(|| {
        format!(
            "导出计划任务 XML 失败：{}",
            task_full_path(&item.scope, &item.locator)
        )
    })?;
    Ok(xml.to_string())
}

/// 永久删除一项计划任务（COM `DeleteTask`，失败回退 `schtasks /Delete /F`）。
pub(crate) fn delete_task(item: &AutostartItem) -> Result<()> {
    match delete_task_com(item) {
        Ok(()) => Ok(()),
        Err(com_err) => match run_schtasks_delete(item) {
            Ok(()) => Ok(()),
            Err(fallback_err) => bail!(
                "删除计划任务「{}」失败：通过任务计划服务删除失败（{com_err:#}）；\
                 改用 schtasks 回退也失败（{fallback_err:#}）。\
                 系统级/其他用户创建的任务需要以管理员身份运行 DaemonMan",
                item.name
            ),
        },
    }
}

fn delete_task_com(item: &AutostartItem) -> Result<()> {
    let _apartment = super::com::init_apartment()?;
    let service = connect_service()?;
    let folder = folder_for(&service, &folder_path_of(item))?;
    unsafe { folder.DeleteTask(&BSTR::from(item.locator.as_str()), 0) }.with_context(|| {
        format!(
            "删除计划任务失败：{}",
            task_full_path(&item.scope, &item.locator)
        )
    })
}

fn run_schtasks_delete(item: &AutostartItem) -> Result<()> {
    let full_path = task_full_path(&item.scope, &item.locator);
    let mut command = std::process::Command::new("schtasks.exe");
    command
        .arg("/Delete")
        .arg("/TN")
        .arg(&full_path)
        .arg("/F");
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().context("启动 schtasks.exe 失败")?;
    if output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    bail!(
        "schtasks /Delete 返回 {}：{}{}",
        output.status,
        if stdout.is_empty() {
            String::new()
        } else {
            format!("{stdout} ")
        },
        stderr
    )
}

/// 用备份的 XML 重新注册计划任务（COM `RegisterTask`，失败回退 `schtasks /Create /XML`）。
pub(crate) fn register_from_xml(task_path: &str, xml_path: &Path) -> Result<()> {
    let xml = std::fs::read_to_string(xml_path)
        .with_context(|| format!("读取任务 XML 备份失败：{}", xml_path.display()))?;
    match register_com(task_path, &xml) {
        Ok(()) => Ok(()),
        Err(com_err) => match run_schtasks_create(task_path, xml_path) {
            Ok(()) => Ok(()),
            Err(fallback_err) => bail!(
                "从 XML 还原计划任务「{task_path}」失败：COM 注册失败（{com_err:#}）；\
                 改用 schtasks 回退也失败（{fallback_err:#}）。\
                 需要凭据（密码登录类型）的任务必须手动导入 XML"
            ),
        },
    }
}

fn register_com(task_path: &str, xml: &str) -> Result<()> {
    let (folder_path, task_name) = split_task_path(task_path)?;
    let _apartment = super::com::init_apartment()?;
    let service = connect_service()?;
    let folder = match folder_for(&service, &folder_path) {
        Ok(folder) => folder,
        Err(_) => create_folder_chain(&service, &folder_path)?,
    };
    unsafe {
        folder.RegisterTask(
            &BSTR::from(task_name.as_str()),
            &BSTR::from(xml),
            TASK_CREATE_OR_UPDATE.0,
            &VARIANT::default(),
            &VARIANT::default(),
            TASK_LOGON_NONE,
            &VARIANT::default(),
        )
    }
    .with_context(|| format!("注册计划任务失败：{task_path}"))?;
    Ok(())
}

/// 逐级创建任务文件夹（`\A\B` → 先 `\A` 再 `\A\B`）。
fn create_folder_chain(service: &ITaskService, folder_path: &str) -> Result<ITaskFolder> {
    let root = folder_for(service, "\\")?;
    let mut current = root;
    let mut current_path = String::new();
    for segment in folder_path.trim_matches('\\').split('\\') {
        if segment.is_empty() {
            continue;
        }
        current_path.push('\\');
        current_path.push_str(segment);
        current = match folder_for(service, &current_path) {
            Ok(folder) => folder,
            Err(_) => unsafe {
                current
                    .CreateFolder(&BSTR::from(segment), &VARIANT::default())
            }
            .with_context(|| format!("创建任务文件夹失败：{current_path}"))?,
        };
    }
    Ok(current)
}

/// 把 `\Sub\Name` 拆成 (文件夹路径, 任务名)。
pub(crate) fn split_task_path(full: &str) -> Result<(String, String)> {
    let trimmed = full.trim();
    if !trimmed.starts_with('\\') {
        bail!("任务路径必须以 \\ 开头：{full}");
    }
    match trimmed.rfind('\\') {
        Some(0) if trimmed.len() > 1 => {
            let name = &trimmed[1..];
            if name.is_empty() {
                bail!("任务路径缺少任务名：{full}");
            }
            Ok(("\\".to_string(), name.to_string()))
        }
        Some(index) if index + 1 < trimmed.len() => Ok((
            trimmed[..index].to_string(),
            trimmed[index + 1..].to_string(),
        )),
        _ => bail!("任务路径缺少任务名：{full}"),
    }
}

fn run_schtasks_create(task_path: &str, xml_path: &Path) -> Result<()> {
    let mut command = std::process::Command::new("schtasks.exe");
    command
        .arg("/Create")
        .arg("/XML")
        .arg(xml_path)
        .arg("/TN")
        .arg(task_path)
        .arg("/F");
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().context("启动 schtasks.exe 失败")?;
    if output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    bail!(
        "schtasks /Create /XML 返回 {}：{}{}",
        output.status,
        if stdout.is_empty() {
            String::new()
        } else {
            format!("{stdout} ")
        },
        stderr
    )
}

/// 完整任务路径（`\Sub\Name`），schtasks 需要它。
pub(crate) fn task_full_path(folder: &str, name: &str) -> String {
    let folder = folder.trim_end_matches('\\');
    if folder.is_empty() {
        format!("\\{name}")
    } else {
        format!("{folder}\\{name}")
    }
}

fn run_schtasks(item: &AutostartItem, enabled: bool) -> Result<()> {
    let full_path = task_full_path(&item.scope, &item.locator);
    let mut command = std::process::Command::new("schtasks.exe");
    // 参数逐项传入，任务名里的空格 / 中文 / 引号由 argv 边界保证，不拼字符串。
    command
        .arg("/Change")
        .arg("/TN")
        .arg(&full_path)
        .arg(if enabled { "/ENABLE" } else { "/DISABLE" });
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .context("启动 schtasks.exe 失败")?;
    if output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    bail!(
        "schtasks 返回 {}：{}{}",
        output.status,
        if stdout.is_empty() { String::new() } else { format!("{stdout} ") },
        stderr
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_folder_detection() {
        assert!(is_system_folder("\\Microsoft"));
        assert!(is_system_folder("\\Microsoft\\Windows"));
        assert!(is_system_folder("\\microsoft\\windows\\update"));
        assert!(!is_system_folder("\\"));
        assert!(!is_system_folder("\\MyTasks"));
        assert!(!is_system_folder("\\MicrosoftBackup"));
    }

    #[test]
    fn task_full_path_join() {
        assert_eq!(task_full_path("\\", "Foo"), "\\Foo");
        assert_eq!(task_full_path("\\Sub", "Foo"), "\\Sub\\Foo");
        assert_eq!(task_full_path("\\Sub\\", "Foo"), "\\Sub\\Foo");
    }

    #[test]
    fn split_task_path_cases() {
        assert_eq!(
            split_task_path("\\Foo").unwrap(),
            ("\\".to_string(), "Foo".to_string())
        );
        assert_eq!(
            split_task_path("\\Sub\\Foo").unwrap(),
            ("\\Sub".to_string(), "Foo".to_string())
        );
        assert_eq!(
            split_task_path("\\中文 目录\\任务 名").unwrap(),
            ("\\中文 目录".to_string(), "任务 名".to_string())
        );
        assert!(split_task_path("Foo").is_err());
        assert!(split_task_path("\\").is_err());
        assert!(split_task_path("\\Sub\\").is_err());
    }

    #[test]
    fn principal_is_current_user_match() {
        let Ok(current) = std::env::var("USERNAME") else {
            return;
        };
        if current.trim().is_empty() {
            return;
        }
        assert!(principal_is_current_user(&current));
        assert!(principal_is_current_user(&format!(r"MYHOST\{current}")));
        assert!(!principal_is_current_user(r"NT AUTHORITY\SYSTEM"));
        assert!(!principal_is_current_user(""));
    }
}
