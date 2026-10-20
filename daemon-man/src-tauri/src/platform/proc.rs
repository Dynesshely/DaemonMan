//! 进程 / 窗口 / 程序启动。
//!
//! 这里集中了所有「和运行中的程序打交道」的能力：
//! - 查询进程（sysinfo）与主窗口（EnumWindows）；
//! - 启动程序（`std::process::Command` + `CREATE_NEW_PROCESS_GROUP`，快捷方式先 COM 解析）；
//! - 提权启动（`ShellExecuteExW` + `runas`）；
//! - 等待主窗口（异步轮询，可取消）、关闭 / 置前窗口、结束进程。
//!
//! 约定：进程名比较一律忽略大小写与 `.exe` 后缀（Windows 上同一名字可能带也可能不带扩展名）。

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sysinfo::{ProcessesToUpdate, System};
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, FALSE, HANDLE, HWND, LPARAM, TRUE, WPARAM};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Com::{IPersistFile, STGM_READ};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetProcessId, OpenProcess, OpenProcessToken,
    CREATE_NEW_PROCESS_GROUP, PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, IShellLinkW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE, SW_SHOWNORMAL, WM_CLOSE,
};

use crate::model::{AppLaunchSpec, ProcessInfo};

/// `ShellLink` 的 CLSID（windows 0.58 的绑定里没有这个常量）。
const CLSID_SHELL_LINK: windows::core::GUID =
    windows::core::GUID::from_u128(0x00021401_0000_0000_c000_000000000046);

/// 轮询间隔（等待窗口）。
const WAIT_WINDOW_POLL: Duration = Duration::from_millis(250);

/// `HANDLE` 的 RAII 包装。
struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 一个顶层窗口（有标题、无 owner）。
#[derive(Debug, Clone)]
struct WindowEntry {
    hwnd: HWND,
    pid: u32,
    title: String,
    visible: bool,
}

/// 快捷方式解析结果。
#[derive(Debug, Clone, Default)]
pub struct LnkTarget {
    pub target: String,
    pub args: Option<String>,
}

// ---------------------------------------------------------------------------
// 权限
// ---------------------------------------------------------------------------

/// 当前进程是否具备管理员权限。
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let _guard = HandleGuard(token);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();
        ok && elevation.TokenIsElevated != 0
    }
}

// ---------------------------------------------------------------------------
// 路径 / 名称
// ---------------------------------------------------------------------------

/// 解析路径：处理 .lnk 快捷方式与 NTFS 符号链接，返回真实目标。
///
/// 目前工作流执行器直接使用 `launch_app`，`resolve_path` 保留给需要「路径→真实目标」
/// 的场景（例如展示节点实际会启动什么）。
#[allow(dead_code)]
pub fn resolve_path(path: &str) -> Option<String> {
    let path = path.trim().trim_matches('"');
    if path.is_empty() {
        return None;
    }
    if path.to_ascii_lowercase().ends_with(".lnk") {
        return Some(
            resolve_lnk(path)
                .map(|link| link.target)
                .unwrap_or_else(|| path.to_string()),
        );
    }
    // canonicalize 会解析 NTFS 符号链接 / 目录联接，并归一化 .. 与 .
    let resolved = std::fs::canonicalize(path).ok();
    Some(match resolved {
        Some(resolved) => strip_verbatim_prefix(&resolved),
        None => path.to_string(),
    })
}

fn strip_verbatim_prefix(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

/// 用 COM `IShellLinkW` 解析 .lnk 快捷方式，拿到目标路径与参数。
pub fn resolve_lnk(path: &str) -> Option<LnkTarget> {
    // COM 对象必须与 guard 同生命周期：guard 先声明，最后析构。
    let _apartment = super::com::init_apartment().ok()?;
    let link: IShellLinkW = super::com::create_instance(&CLSID_SHELL_LINK).ok()?;
    let persist: IPersistFile = link.cast().ok()?;

    let wide = to_wide(path);
    unsafe { persist.Load(PCWSTR(wide.as_ptr()), STGM_READ) }.ok()?;

    let mut path_buffer = [0u16; 1024];
    unsafe { link.GetPath(&mut path_buffer, std::ptr::null_mut(), 0) }.ok()?;
    let target = super::registry::expand_env_vars(&from_wide_buffer(&path_buffer));
    if target.trim().is_empty() {
        return None;
    }

    let mut args_buffer = [0u16; 1024];
    let args = unsafe { link.GetArguments(&mut args_buffer) }
        .ok()
        .map(|_| from_wide_buffer(&args_buffer))
        .filter(|args| !args.trim().is_empty());

    Some(LnkTarget { target, args })
}

/// 从路径推导进程名（不含 .exe）。
pub fn process_name_from_path(path: &str) -> String {
    let path = path.trim().trim_matches('"');
    Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 路径是否指向 DaemonMan 自身。
pub fn is_current_exe(path: &str) -> bool {
    let path = path.trim().trim_matches('"');
    if path.is_empty() {
        return false;
    }
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    if path.eq_ignore_ascii_case(&current.display().to_string()) {
        return true;
    }
    match (
        Path::new(path).file_name().map(|name| name.to_string_lossy().to_ascii_lowercase()),
        current
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase()),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn normalize_process_name(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    lower
        .strip_suffix(".exe")
        .unwrap_or(lower.as_str())
        .trim()
        .to_string()
}

fn normalize_os_process_name(name: &OsStr) -> String {
    normalize_process_name(&name.to_string_lossy())
}

// ---------------------------------------------------------------------------
// 启动
// ---------------------------------------------------------------------------

/// 启动程序，返回 pid（0 表示无法获得）。
///
/// - `.exe`：`std::process::Command` + `CREATE_NEW_PROCESS_GROUP`，可拿到 pid；
/// - `.lnk`：先用 IShellLinkW 解析目标与参数，再按 `.exe` 启动；
/// - `.bat` / `.cmd`：交给 `cmd.exe /C`；
/// - 其它（URL、文档）：交给 ShellExecute；
/// - `run_as_admin`：ShellExecuteExW + `runas`（会弹 UAC）。
pub fn launch_app(spec: &AppLaunchSpec) -> Result<u32> {
    let path = spec.path.trim().trim_matches('"').to_string();
    if path.is_empty() {
        bail!("启动失败：可执行文件路径为空");
    }
    let lower = path.to_ascii_lowercase();

    if spec.run_as_admin {
        return shell_execute(&path, &spec.args, spec.working_dir.as_deref(), "runas");
    }

    if lower.ends_with(".lnk") {
        let link = resolve_lnk(&path)
            .with_context(|| format!("无法解析快捷方式：{path}"))?;
        let mut args = link
            .args
            .as_deref()
            .map(split_args)
            .unwrap_or_default();
        args.extend(spec.args.iter().cloned());
        return spawn_executable(&link.target, &args, spec);
    }

    if lower.ends_with(".bat") || lower.ends_with(".cmd") {
        let mut args = vec!["/C".to_string(), path.clone()];
        args.extend(spec.args.iter().cloned());
        return spawn_executable(&comspec(), &args, spec);
    }

    if lower.ends_with(".exe") || Path::new(&path).extension().is_none() {
        return spawn_executable(&path, &spec.args, spec);
    }

    // 非可执行文件（URL / 文档 / 其它已关联类型）交给 shell 打开。
    shell_execute(&path, &spec.args, spec.working_dir.as_deref(), "open")
}

fn comspec() -> String {
    std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
}

fn spawn_executable(exe: &str, args: &[String], spec: &AppLaunchSpec) -> Result<u32> {
    let mut command = std::process::Command::new(exe);
    command.args(args);

    if let Some(dir) = spec.working_dir.as_deref().filter(|dir| !dir.trim().is_empty()) {
        command.current_dir(dir);
    } else if let Some(parent) = Path::new(exe).parent() {
        if parent.is_dir() {
            command.current_dir(parent);
        }
    }

    for env in &spec.env {
        if !env.key.trim().is_empty() {
            command.env(&env.key, &env.value);
        }
    }

    // 独立进程组：Ctrl+C 之类不会连带影响 DaemonMan。
    command.creation_flags(CREATE_NEW_PROCESS_GROUP.0);

    let child = command
        .spawn()
        .with_context(|| format!("启动程序失败：{exe}"))?;
    Ok(child.id())
}

/// ShellExecuteExW 启动（用于提权 / URL / 文档）。返回 pid，拿不到时返回 0。
fn shell_execute(
    file: &str,
    args: &[String],
    working_dir: Option<&str>,
    verb: &str,
) -> Result<u32> {
    let file_wide = to_wide(file);
    let params_wide = if args.is_empty() {
        None
    } else {
        Some(to_wide(&join_args(args)))
    };
    let dir_wide = working_dir
        .filter(|dir| !dir.trim().is_empty())
        .map(to_wide);
    let verb_wide = to_wide(verb);

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
        lpVerb: PCWSTR(verb_wide.as_ptr()),
        lpFile: PCWSTR(file_wide.as_ptr()),
        lpParameters: params_wide
            .as_ref()
            .map(|params| PCWSTR(params.as_ptr()))
            .unwrap_or(PCWSTR::null()),
        lpDirectory: dir_wide
            .as_ref()
            .map(|dir| PCWSTR(dir.as_ptr()))
            .unwrap_or(PCWSTR::null()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    unsafe { ShellExecuteExW(&mut info) }
        .with_context(|| format!("通过 ShellExecuteExW（{verb}）启动失败：{file}"))?;

    // 大多数 shell 启动拿不到进程句柄（pid 未知），此时返回 0。
    if info.hProcess.is_invalid() {
        return Ok(0);
    }
    let handle = info.hProcess;
    let pid = unsafe { GetProcessId(handle) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    Ok(pid)
}

// ---------------------------------------------------------------------------
// 进程查询
// ---------------------------------------------------------------------------

/// 进程是否在运行。
pub fn is_process_running(name: &str) -> bool {
    !process_pids(name).is_empty()
}

fn process_pids(name: &str) -> Vec<u32> {
    let wanted = normalize_process_name(name);
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
        .processes()
        .iter()
        .filter(|(_, process)| normalize_os_process_name(process.name()) == wanted)
        .map(|(pid, _)| pid.as_u32())
        .collect()
}

/// 列出系统进程。
pub fn list_processes() -> Vec<ProcessInfo> {
    let mut system = System::new_all();
    system.refresh_all();

    // 部分受保护进程拿不到真实启动时间，run_time() 会给出几十万小时的异常值；
    // 用系统开机时长做上限钳制，避免界面显示「497913h」这类明显错误的数据。
    let system_uptime = System::uptime();

    // 一次 EnumWindows 收集所有主窗口标题，避免每个进程各枚举一遍。
    let mut titles: HashMap<u32, String> = HashMap::new();
    for window in all_titled_windows() {
        if window.visible {
            titles.entry(window.pid).or_insert(window.title);
        }
    }

    let mut processes: Vec<ProcessInfo> = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessInfo {
            pid: pid.as_u32(),
            name: process.name().to_string_lossy().to_string(),
            executable: process
                .exe()
                .map(|exe| exe.display().to_string()),
            memory_bytes: process.memory(),
            cpu_usage: process.cpu_usage(),
            running_secs: if system_uptime > 0 {
                process.run_time().min(system_uptime)
            } else {
                process.run_time()
            },
            managed: false,
            workflow_name: None,
            main_window_title: titles.get(&pid.as_u32()).cloned(),
        })
        .collect();

    processes.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.pid.cmp(&right.pid))
    });
    processes
}

/// 结束进程。
pub fn kill_process(pid: u32) -> Result<()> {
    if pid == 0 {
        bail!("无效的进程 ID：0");
    }
    if pid == std::process::id() {
        bail!("拒绝结束 DaemonMan 自身进程（pid {pid}）");
    }
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, FALSE, pid) }
        .with_context(|| format!("打开进程 {pid} 失败（可能进程已退出或权限不足）"))?;
    let _guard = HandleGuard(handle);
    unsafe { windows::Win32::System::Threading::TerminateProcess(handle, 1) }
        .with_context(|| format!("结束进程 {pid} 失败（可能需要管理员权限）"))?;
    Ok(())
}

/// 取进程主窗口标题（`list_processes` 内部走批量枚举，这里是单进程入口）。
#[allow(dead_code)]
pub fn main_window_title(pid: u32) -> Option<String> {
    find_main_window(pid).map(|window| window.title)
}

// ---------------------------------------------------------------------------
// 窗口
// ---------------------------------------------------------------------------

fn all_titled_windows() -> Vec<WindowEntry> {
    let mut entries: Vec<WindowEntry> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_callback),
            LPARAM(&mut entries as *mut _ as isize),
        );
    }
    entries
}

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let entries = &mut *(lparam.0 as *mut Vec<WindowEntry>);

    // 只保留顶层、无 owner（排除对话框/工具窗口）、有标题的窗口。
    if GetWindow(hwnd, GW_OWNER).is_ok() {
        return TRUE;
    }

    // 排除桌面外壳窗口（Progman / WorkerW）：它们的标题是 "Program Manager"，
    // 不是任何一个进程的「主窗口」，更不能对它发 WM_CLOSE。
    let mut class_buffer = [0u16; 64];
    let class_length = GetClassNameW(hwnd, &mut class_buffer);
    if class_length > 0 {
        let class = String::from_utf16_lossy(&class_buffer[..class_length as usize]);
        if class.eq_ignore_ascii_case("Progman") || class.eq_ignore_ascii_case("WorkerW") {
            return TRUE;
        }
    }

    let length = GetWindowTextLengthW(hwnd);
    if length <= 0 {
        return TRUE;
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = GetWindowTextW(hwnd, &mut buffer);
    if copied <= 0 {
        return TRUE;
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    entries.push(WindowEntry {
        hwnd,
        pid,
        title: String::from_utf16_lossy(&buffer[..copied as usize]),
        visible: IsWindowVisible(hwnd).as_bool(),
    });
    TRUE
}

fn find_main_window(pid: u32) -> Option<WindowEntry> {
    let windows = all_titled_windows();
    windows
        .iter()
        .find(|window| window.pid == pid && window.visible)
        .or_else(|| windows.iter().find(|window| window.pid == pid))
        .cloned()
}

fn find_window_by_process_name(process_name: &str) -> Option<WindowEntry> {
    let pids = process_pids(process_name);
    if pids.is_empty() {
        return None;
    }
    let windows = all_titled_windows();
    windows
        .iter()
        .find(|window| window.visible && pids.contains(&window.pid))
        .or_else(|| windows.iter().find(|window| pids.contains(&window.pid)))
        .cloned()
}

/// 等待进程主窗口出现。
///
/// `require_visible` 为 false 时只要进程存在就返回；轮询间隔 250ms，
/// 每轮都检查 `cancel`，避免工作流取消后还傻等。
pub async fn wait_for_window(
    process_name: &str,
    require_visible: bool,
    timeout: Duration,
    cancel: &Arc<AtomicBool>,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return false;
        }
        if !process_name.trim().is_empty() {
            let pids = process_pids(process_name);
            if !pids.is_empty() {
                if !require_visible {
                    return true;
                }
                if all_titled_windows()
                    .iter()
                    .any(|window| window.visible && pids.contains(&window.pid))
                {
                    return true;
                }
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(WAIT_WINDOW_POLL).await;
    }
}

/// 向主窗口发送 WM_CLOSE。
pub fn close_main_window(process_name: &str) -> bool {
    let Some(window) = find_window_by_process_name(process_name) else {
        return false;
    };
    unsafe { PostMessageW(window.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok() }
}

/// 把主窗口置前。
pub fn focus_main_window(process_name: &str) -> bool {
    let Some(window) = find_window_by_process_name(process_name) else {
        return false;
    };
    unsafe {
        let _ = ShowWindow(window.hwnd, SW_RESTORE);
        if SetForegroundWindow(window.hwnd).as_bool() {
            return true;
        }
        // Windows 的前台锁：非前台进程直接 SetForegroundWindow 会失败。
        // 临时把当前线程附加到「当前前台窗口所在线程」，再置前后分离，是最常用的绕过方式。
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return false;
        }
        let target_thread = GetWindowThreadProcessId(window.hwnd, None);
        let foreground_thread = GetWindowThreadProcessId(foreground, None);
        if target_thread == 0 || foreground_thread == 0 || target_thread == foreground_thread {
            return false;
        }
        if !AttachThreadInput(foreground_thread, target_thread, TRUE).as_bool() {
            return false;
        }
        let _ = BringWindowToTop(window.hwnd);
        let ok = SetForegroundWindow(window.hwnd).as_bool();
        let _ = AttachThreadInput(foreground_thread, target_thread, FALSE);
        ok
    }
}

// ---------------------------------------------------------------------------
// 命令行字符串处理（纯函数，便于单测）
// ---------------------------------------------------------------------------

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide_buffer(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// 按 Windows 习惯切分参数串：支持双引号包裹的空格参数。
pub(crate) fn split_args(raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_content = false;

    for ch in raw.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                has_content = true;
            }
            ' ' | '\t' if !in_quotes => {
                if has_content {
                    args.push(std::mem::take(&mut current));
                    has_content = false;
                }
            }
            _ => {
                current.push(ch);
                has_content = true;
            }
        }
    }
    if has_content {
        args.push(current);
    }
    args
}

/// 把参数数组拼成 ShellExecute 需要的参数串（含空格的参数加引号）。
pub(crate) fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains(' ') || arg.contains('\t') || arg.contains('"') {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_name_normalization() {
        assert_eq!(normalize_process_name("Explorer.EXE"), "explorer");
        assert_eq!(normalize_process_name("  chrome.exe "), "chrome");
        assert_eq!(normalize_process_name("node"), "node");
        assert_eq!(process_name_from_path(r"C:\Program Files\App\app.exe"), "app");
        assert_eq!(process_name_from_path(r#""C:\a\b.exe""#), "b");
    }

    #[test]
    fn split_args_handles_quotes() {
        assert_eq!(split_args(r#"--title "hello world" --x"#), vec![
            "--title".to_string(),
            "hello world".to_string(),
            "--x".to_string()
        ]);
        assert_eq!(split_args("  "), Vec::<String>::new());
        assert_eq!(split_args(r#""C:\a b\c.txt""#), vec![r"C:\a b\c.txt".to_string()]);
    }

    #[test]
    fn join_args_quotes_when_needed() {
        assert_eq!(
            join_args(&["--min".to_string(), "hello world".to_string()]),
            r#"--min "hello world""#
        );
        assert_eq!(join_args(&[]), "");
    }

    #[test]
    fn from_wide_buffer_stops_at_nul() {
        let buffer = ['a' as u16, 'b' as u16, 0, 'c' as u16];
        assert_eq!(from_wide_buffer(&buffer), "ab");
    }

    /// 冒烟测试：真实环境下列出进程 / 判断是否提权。
    /// 手动运行：`cargo test --lib process_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn process_smoke() {
        println!("is_elevated = {}", is_elevated());
        let processes = list_processes();
        println!("进程数量 = {}", processes.len());
        let with_window = processes
            .iter()
            .filter(|process| process.main_window_title.is_some())
            .count();
        println!("有主窗口标题的进程 = {with_window}");
        assert!(!processes.is_empty());
        assert!(is_process_running("explorer"));
        println!("explorer.exe pid = {:?}", process_pids("explorer"));
        let explorer_pid = process_pids("explorer").first().copied();
        if let Some(pid) = explorer_pid {
            println!("explorer 主窗口 = {:?}", main_window_title(pid));
        }
        // 解析当前 exe 自身应恒为真。
        let current = std::env::current_exe().unwrap().display().to_string();
        assert!(is_current_exe(&current));
    }

    /// 冒烟测试：启动记事本 → 等主窗口 → 取标题 → 置前 → WM_CLOSE → 确认退出。
    ///
    /// 手动运行：`cargo test --lib launch_window_smoke -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn launch_window_smoke() {
        // 用户可能正开着记事本，避免干扰真实会话。
        if is_process_running("mstsc") {
            println!("远程桌面连接已在运行，为避免打扰用户跳过本测试");
            return;
        }

        let spec = AppLaunchSpec {
            path: r"C:\Windows\System32\mstsc.exe".to_string(),
            ..Default::default()
        };
        let pid = launch_app(&spec).expect("启动 mstsc 失败");
        println!("mstsc pid = {pid}");
        assert!(pid > 0, "启动 exe 应该能拿到 pid");
        assert!(is_process_running("mstsc"), "启动后进程应存在");

        let cancel = Arc::new(AtomicBool::new(false));
        let found = wait_for_window("mstsc", true, Duration::from_secs(10), &cancel).await;
        println!("wait_for_window(require_visible=true) = {found}");
        assert!(found, "10 秒内应等到 mstsc 主窗口");

        let title = main_window_title(pid);
        println!("主窗口标题（启动 pid={pid}）= {title:?}");
        // 注意：部分应用（如 Win11 记事本）会由「启动的进程」再拉起的子进程持有窗口，
        // 因此按名字查到的 pid 集合里可能才有窗口。这里两种都验证。
        let pids = process_pids("mstsc");
        let titles: Vec<(u32, Option<String>)> = pids
            .iter()
            .map(|pid| (*pid, main_window_title(*pid)))
            .collect();
        println!("mstsc pids 与标题 = {titles:?}");
        assert!(
            title.is_some() || titles.iter().any(|(_, title)| title.is_some()),
            "应能取到 mstsc 主窗口标题"
        );

        let focused = focus_main_window("mstsc");
        println!("focus_main_window = {focused}");

        let closed = close_main_window("mstsc");
        println!("close_main_window(WM_CLOSE) = {closed}");
        assert!(closed, "应能向主窗口发送 WM_CLOSE");

        for _ in 0..24 {
            if !is_process_running("mstsc") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let still_running = is_process_running("mstsc");
        println!("WM_CLOSE 后仍在运行 = {still_running}");
        if still_running {
            kill_process(pid).expect("兜底结束 mstsc 失败");
        }
        assert!(!is_process_running("mstsc"), "最终不应残留 mstsc 进程");
    }
}
