//! 进程 / 窗口 / 程序启动。
//!
//! 这里集中了所有「和运行中的程序打交道」的能力：
//! - 查询进程（sysinfo）与主窗口（EnumWindows）；
//! - 启动程序（`CreateProcessW` + `STARTUPINFOEXW`，父进程设为 explorer/shell，
//!   让被启动的进程不挂在 DaemonMan 的进程树下；快捷方式先 COM 解析）；
//! - 提权启动（`ShellExecuteExW` + `runas`）；
//! - 等待主窗口（异步轮询，可取消）、关闭 / 置前窗口、结束进程。
//!
//! 约定：进程名比较一律忽略大小写与 `.exe` 后缀（Windows 上同一名字可能带也可能不带扩展名）。

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, FALSE, HANDLE, HWND, LPARAM, TRUE, WPARAM};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Com::{IPersistFile, STGM_READ};
use windows::Win32::System::Threading::{
    AttachThreadInput, CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetProcessId,
    InitializeProcThreadAttributeList, OpenProcess, OpenProcessToken, UpdateProcThreadAttribute,
    CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_CREATE_PROCESS,
    PROCESS_INFORMATION, PROCESS_TERMINATE, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, STARTUPINFOEXW,
    STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, IShellLinkW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClassNameW, GetForegroundWindow, GetShellWindow, GetWindow,
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

// ---------------------------------------------------------------------------
// 启动：CreateProcessW + 父进程属性（shell / explorer）
// ---------------------------------------------------------------------------

/// 子进程的父进程：shell（explorer）窗口所属进程的句柄。
///
/// 通过 `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` 指定它之后，被启动的进程在进程树里
/// 挂在 explorer 下而不是 DaemonMan 下；DaemonMan 退出/崩溃不会连带影响这些进程。
struct ShellParent {
    handle: HANDLE,
}

impl ShellParent {
    /// 打开 shell 进程：`GetShellWindow` → pid → `OpenProcess(PROCESS_CREATE_PROCESS)`。
    fn open() -> Option<Self> {
        unsafe {
            let shell_window = GetShellWindow();
            if shell_window.is_invalid() {
                eprintln!("[platform] 找不到 shell 窗口，启动的进程将挂在 DaemonMan 下");
                return None;
            }
            let mut pid = 0u32;
            if GetWindowThreadProcessId(shell_window, Some(&mut pid)) == 0 || pid == 0 {
                return None;
            }
            // shell 就是自己（几乎不可能）时没有意义。
            if pid == std::process::id() {
                return None;
            }
            match OpenProcess(PROCESS_CREATE_PROCESS, FALSE, pid) {
                Ok(handle) => Some(Self { handle }),
                Err(err) => {
                    eprintln!(
                        "[platform] 打开 shell 进程（pid {pid}）失败，启动的进程将挂在 DaemonMan 下：{err}"
                    );
                    None
                }
            }
        }
    }
}

impl Drop for ShellParent {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

/// `PROC_THREAD_ATTRIBUTE_LIST` 的缓冲区。
///
/// Windows 要求该结构按指针宽度对齐，所以底层用 `Vec<usize>` 而不是 `Vec<u8>`。
struct ParentAttributeList {
    /// 只负责持有缓冲区（`pointer` 指向它），生命周期内不能移动/重分配。
    _storage: Vec<usize>,
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl ParentAttributeList {
    /// 初始化属性列表并写入 `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`。
    fn new(parent: &ShellParent) -> Option<Self> {
        unsafe {
            let mut size = 0usize;
            // 第一次调用只用来查询所需字节数（预期返回 ERROR_INSUFFICIENT_BUFFER）。
            let _ = InitializeProcThreadAttributeList(
                LPPROC_THREAD_ATTRIBUTE_LIST(std::ptr::null_mut()),
                1,
                0,
                &mut size,
            );
            if size == 0 {
                eprintln!("[platform] 查询进程属性列表大小失败，将回退普通启动");
                return None;
            }
            let words = size.div_ceil(std::mem::size_of::<usize>());
            let mut storage = vec![0usize; words];
            let pointer = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
            if let Err(err) = InitializeProcThreadAttributeList(pointer, 1, 0, &mut size) {
                eprintln!("[platform] 初始化进程属性列表失败，将回退普通启动：{err}");
                return None;
            }
            let list = Self {
                _storage: storage,
                pointer,
            };
            let parent_value: *const HANDLE = &parent.handle;
            if let Err(err) = UpdateProcThreadAttribute(
                pointer,
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                Some(parent_value as *const core::ffi::c_void),
                std::mem::size_of::<HANDLE>(),
                None,
                None,
            ) {
                eprintln!("[platform] 设置父进程属性失败，将回退普通启动：{err}");
                return None;
            }
            Some(list)
        }
    }
}

impl Drop for ParentAttributeList {
    fn drop(&mut self) {
        if !self.pointer.is_invalid() {
            unsafe { DeleteProcThreadAttributeList(self.pointer) };
        }
    }
}

/// 启动可执行文件：父进程优先设为 shell，失败时逐级回退。
///
/// 回退顺序：
/// 1. shell 父进程 + `CREATE_BREAKAWAY_FROM_JOB`（DaemonMan 自身在 job 里时也能脱离）；
/// 2. shell 父进程（job 限制了 breakaway 的情况）；
/// 3. 普通 `CreateProcessW`（父进程 = DaemonMan，保证功能可用）。
fn spawn_executable(exe: &str, args: &[String], spec: &AppLaunchSpec) -> Result<u32> {
    let working_dir = resolve_working_dir(exe, spec).map(|dir| to_wide(&dir));
    let environment = build_environment_block(&spec.env);

    if let Some(parent) = ShellParent::open() {
        if let Some(list) = ParentAttributeList::new(&parent) {
            match create_process(
                exe,
                args,
                working_dir.as_deref(),
                environment.as_deref(),
                Some(&list),
                true,
            ) {
                Ok(pid) => return Ok(pid),
                Err(err) => eprintln!(
                    "[platform] 以 shell 为父进程 + BREAKAWAY 启动失败，去掉 BREAKAWAY 重试：{err:#}"
                ),
            }
            if let Ok(pid) = create_process(
                exe,
                args,
                working_dir.as_deref(),
                environment.as_deref(),
                Some(&list),
                false,
            ) {
                return Ok(pid);
            }
        }
    }

    create_process(
        exe,
        args,
        working_dir.as_deref(),
        environment.as_deref(),
        None,
        false,
    )
    .with_context(|| format!("启动程序失败：{exe}"))
}

/// 工作目录：优先 spec 指定，否则用目标 exe 所在目录（存在时）。
fn resolve_working_dir(exe: &str, spec: &AppLaunchSpec) -> Option<String> {
    if let Some(dir) = spec
        .working_dir
        .as_deref()
        .filter(|dir| !dir.trim().is_empty())
    {
        return Some(dir.to_string());
    }
    let parent = Path::new(exe).parent()?;
    if parent.is_dir() {
        Some(parent.display().to_string())
    } else {
        None
    }
}

/// 调一次 `CreateProcessW`；`parent` 为 None 时不使用 `EXTENDED_STARTUPINFO_PRESENT`。
fn create_process(
    exe: &str,
    args: &[String],
    working_dir: Option<&[u16]>,
    environment: Option<&[u16]>,
    parent: Option<&ParentAttributeList>,
    breakaway: bool,
) -> Result<u32> {
    // CreateProcessW 可能就地修改命令行缓冲区，每次尝试都必须用新的缓冲。
    let mut command_line: Vec<u16> = build_command_line(exe, args)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = if parent.is_some() {
        std::mem::size_of::<STARTUPINFOEXW>() as u32
    } else {
        std::mem::size_of::<STARTUPINFOW>() as u32
    };
    if let Some(list) = parent {
        startup.lpAttributeList = list.pointer;
    }

    let mut flags = CREATE_NEW_PROCESS_GROUP;
    if parent.is_some() {
        flags |= EXTENDED_STARTUPINFO_PRESENT;
    }
    if environment.is_some() {
        flags |= CREATE_UNICODE_ENVIRONMENT;
    }
    if breakaway {
        flags |= CREATE_BREAKAWAY_FROM_JOB;
    }

    let mut process_info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            // lpApplicationName 传 NULL，只用命令行：这样带参数的 .exe 与 cmd /C 都走同一条路径。
            PCWSTR::null(),
            PWSTR(command_line.as_mut_ptr()),
            None,
            None,
            FALSE,
            flags,
            environment.map(|block| block.as_ptr() as *const core::ffi::c_void),
            working_dir.map_or(PCWSTR::null(), |dir| PCWSTR(dir.as_ptr())),
            &startup.StartupInfo,
            &mut process_info,
        )
        .context("CreateProcessW 调用失败")?;
    }

    // 只保留 pid：进程/线程句柄立刻关闭，进程本身继续运行。
    unsafe {
        let _ = CloseHandle(process_info.hThread);
        let _ = CloseHandle(process_info.hProcess);
    }
    Ok(process_info.dwProcessId)
}

/// 命令行：`"<exe>" arg1 arg2 ...`。
pub(crate) fn build_command_line(exe: &str, args: &[String]) -> String {
    let mut command_line = String::with_capacity(exe.len() + 8);
    command_line.push('"');
    command_line.push_str(exe);
    command_line.push('"');
    for arg in args {
        command_line.push(' ');
        command_line.push_str(&escape_win_arg(arg));
    }
    command_line
}

/// 按 Windows argv 规则转义单个参数（只在必要时加引号）。
pub(crate) fn escape_win_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    if !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let chars: Vec<char> = arg.chars().collect();
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '\\' => {
                // 连续反斜杠只在「后面是引号 / 字符串结尾」时才需要加倍。
                let mut count = 0;
                while index < chars.len() && chars[index] == '\\' {
                    count += 1;
                    index += 1;
                }
                if index == chars.len() {
                    for _ in 0..count * 2 {
                        out.push('\\');
                    }
                } else if chars[index] == '"' {
                    for _ in 0..count * 2 + 1 {
                        out.push('\\');
                    }
                    out.push('"');
                    index += 1;
                } else {
                    for _ in 0..count {
                        out.push('\\');
                    }
                }
            }
            '"' => {
                out.push('\\');
                out.push('"');
                index += 1;
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }
    out.push('"');
    out
}

/// 构造 Unicode 环境块（`KEY=VALUE\0...\0\0`）。
///
/// `CreateProcessW` 不能像 `Command::env` 那样「继承 + 追加覆盖」，所以先复制当前进程环境，
/// 再应用 spec 里的覆盖，最后按不区分大小写排序（Windows 要求）。
/// 返回 `None` 表示没有额外环境变量，直接继承父进程环境（`lpEnvironment = NULL`）。
pub(crate) fn build_environment_block(overrides: &[crate::model::EnvVar]) -> Option<Vec<u16>> {
    if overrides.is_empty() {
        return None;
    }
    let mut entries: Vec<(String, OsString, OsString)> = Vec::new();
    for (key, value) in std::env::vars_os() {
        // `=C:` 这类「每驱动器当前目录」变量由系统管理，不放进环境块。
        if key.to_string_lossy().starts_with('=') {
            continue;
        }
        entries.push((key.to_string_lossy().to_uppercase(), key, value));
    }
    for env in overrides {
        if env.key.trim().is_empty() {
            continue;
        }
        let upper = env.key.to_uppercase();
        entries.retain(|(existing, _, _)| *existing != upper);
        entries.push((upper, OsString::from(&env.key), OsString::from(&env.value)));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut block: Vec<u16> = Vec::new();
    for (_, key, value) in entries {
        for unit in key.as_os_str().encode_wide() {
            block.push(unit);
        }
        block.push(b'=' as u16);
        for unit in value.as_os_str().encode_wide() {
            block.push(unit);
        }
        block.push(0);
    }
    block.push(0);
    Some(block)
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

// ---------------------------------------------------------------------------
// 「已在运行」检测（含浏览器 PWA / Web App）
// ---------------------------------------------------------------------------

/// 浏览器类可执行文件名（小写、不含 `.exe`）。
///
/// 这些「应用」的快捷方式目标通常是浏览器本体，进程名无法区分具体是哪个 PWA，
/// 只能靠窗口标题（或进程命令行里的 `--app-id=` / `--app=`）判断。
pub(crate) const BROWSER_PROCESS_NAMES: [&str; 6] =
    ["chrome", "msedge", "brave", "chromium", "opera", "vivaldi"];

/// 普通浏览器标签窗口的标题后缀（PWA 独立窗口一般不带这些后缀）。
const BROWSER_TITLE_SUFFIXES: [&str; 6] = [
    " - google chrome",
    " - microsoft edge",
    " - brave",
    " - chromium",
    " - opera",
    " - vivaldi",
];

/// 浏览器安装的 PWA / Web App 快捷方式有时直接指向浏览器安装目录里的
/// `chrome_proxy.exe` / `msedge_proxy.exe`（而不是浏览器本体），需要按安装路径
/// 判断它属于哪个浏览器；同时区分「浏览器应用节点」与「PWA 节点」：
/// - 浏览器应用（Chrome/Edge/Edge Beta 本体）：进程在跑即视为已存在；
/// - PWA（proxy 快捷方式或 `--app-id` / `--app=`）：必须匹配到对应窗口才算存在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserTargetInfo {
    pub browser: &'static str,
    pub proxy: &'static str,
    /// 是否为浏览器内安装的 PWA / Web App。
    pub is_pwa: bool,
    /// 安装目录（小写），用于区分 Edge / Edge Beta 等同名进程。
    pub install_root: Option<String>,
    /// Chrome/Edge 的 `--profile-directory`（已去引号）；用于区分不同 profile。
    pub profile: Option<String>,
}

pub(crate) fn browser_target_info(
    target: &str,
    explicit_process_name: Option<&str>,
    args: &str,
    display_name: &str,
) -> Option<BrowserTargetInfo> {
    let lower = target.to_ascii_lowercase().replace('/', "\\");
    // 先看安装路径：Brave 的 PWA 快捷方式就叫 chrome_proxy.exe，
    // 只能靠 `Brave-Browser` 目录区分，不能按文件名猜。
    let from_path = if lower.contains("brave-browser") || lower.contains("\\brave\\") {
        Some(("brave", "brave_proxy"))
    } else if lower.contains("microsoft\\edge") || lower.contains("\\msedge") {
        Some(("msedge", "msedge_proxy"))
    } else if lower.contains("google\\chrome") || lower.contains("\\chrome\\application") {
        Some(("chrome", "chrome_proxy"))
    } else if lower.contains("chromium") {
        Some(("chromium", "chromium_proxy"))
    } else if lower.contains("\\opera") {
        Some(("opera", "opera_proxy"))
    } else if lower.contains("vivaldi") {
        Some(("vivaldi", "vivaldi_proxy"))
    } else {
        None
    };

    let matched = if let Some(value) = from_path {
        Some(value)
    } else {
        let stem = process_name_from_path(target);
        let mut found = None;
        for candidate in [explicit_process_name, Some(stem.as_str())] {
            let Some(candidate) = candidate else { continue };
            let normalized = normalize_process_name(candidate);
            if let Some(base) = proxy_browser_name(&normalized) {
                found = Some((base, proxy_stem(base)));
                break;
            }
            if is_browser_process_name(&normalized) {
                if let Some(base) = browser_static_name(&normalized) {
                    found = Some((base, proxy_stem(base)));
                    break;
                }
            }
        }
        found
    };
    let (browser, proxy) = matched?;

    let lower_args = args.to_ascii_lowercase();
    let target_name = normalize_process_name(&process_name_from_path(target));
    let is_proxy = target_name.ends_with("_proxy");
    let profile = profile_directory_from_args(args);
    let has_app_args =
        lower_args.contains("--app-id") || lower_args.contains("--app=");
    let has_profile_args = profile.is_some() || lower_args.contains("--profile-directory");
    // 显示名明确是浏览器本体（Chrome / Edge / Edge Beta / Brave …）时，
    // 即使目标只是浏览器 exe，也按「浏览器应用」处理：进程在跑即跳过。
    let display = normalize_display_name(display_name);
    let browser_brand = [
        "chrome",
        "google chrome",
        "edge",
        "edge beta",
        "microsoft edge",
        "brave",
        "chromium",
        "opera",
        "vivaldi",
    ]
    .iter()
    .any(|brand| display == *brand || display.starts_with(&format!("{brand} ")));
    let is_pwa = is_proxy || has_app_args || (!has_profile_args && !browser_brand);
    let install_root = std::path::Path::new(target)
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| {
            path.to_string_lossy()
                .to_ascii_lowercase()
                .replace('/', "\\")
        });

    Some(BrowserTargetInfo {
        browser,
        proxy,
        is_pwa,
        install_root,
        profile,
    })
}

/// 从浏览器命令行参数里提取 `--profile-directory`（支持 `=` 与空格两种写法、
/// 引号包裹的空格 profile 名）。参数可能来自快捷方式。
pub(crate) fn profile_directory_from_args(args: &str) -> Option<String> {
    let lower = args.to_ascii_lowercase();
    let needle = "--profile-directory";
    let mut search_from = 0usize;
    while let Some(offset) = lower[search_from..].find(needle) {
        let index = search_from + offset;
        let mut rest = &args[index + needle.len()..];
        rest = rest.trim_start();
        if let Some(stripped) = rest.strip_prefix('=') {
            rest = stripped.trim_start();
        }
        if rest.is_empty() {
            break;
        }
        let value = if let Some(quoted) = rest.strip_prefix('"') {
            quoted.split('"').next().unwrap_or("").to_string()
        } else {
            rest.split_whitespace().next().unwrap_or("").to_string()
        };
        if !value.is_empty() && value != "=" {
            return Some(value);
        }
        search_from = index + 1;
    }
    None
}

fn process_cmd_line(process: &sysinfo::Process) -> String {
    process
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

fn command_line_has_profile(process: &sysinfo::Process, profile: &str) -> bool {
    let command_line = process_cmd_line(process);
    !command_line.is_empty()
        && profile_directory_from_args(&command_line)
            .is_some_and(|found| found.eq_ignore_ascii_case(profile))
}

/// 把规范化后的浏览器进程名映射到 `'static` 字符串。
fn browser_static_name(name: &str) -> Option<&'static str> {
    match name {
        "chrome" => Some("chrome"),
        "msedge" => Some("msedge"),
        "brave" => Some("brave"),
        "chromium" => Some("chromium"),
        "opera" => Some("opera"),
        "vivaldi" => Some("vivaldi"),
        _ => None,
    }
}

fn proxy_stem(browser: &str) -> &'static str {
    match browser {
        "msedge" => "msedge_proxy",
        "brave" => "brave_proxy",
        "chromium" => "chromium_proxy",
        "opera" => "opera_proxy",
        "vivaldi" => "vivaldi_proxy",
        _ => "chrome_proxy",
    }
}

fn proxy_browser_name(name: &str) -> Option<&'static str> {
    match name {
        "chrome_proxy" => Some("chrome"),
        "msedge_proxy" => Some("msedge"),
        "brave_proxy" => Some("brave"),
        "chromium_proxy" => Some("chromium"),
        "opera_proxy" => Some("opera"),
        "vivaldi_proxy" => Some("vivaldi"),
        _ => None,
    }
}

/// 浏览器 + 代理进程的 pid 列表，按安装目录与 profile 过滤。
fn browser_target_pids(info: &BrowserTargetInfo) -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    let wanted = [
        normalize_process_name(info.browser),
        normalize_process_name(info.proxy),
    ];
    let mut pids = Vec::new();
    for (pid, process) in system.processes() {
        let name = normalize_os_process_name(process.name());
        if !wanted.contains(&name) {
            continue;
        }
        if let Some(root) = info.install_root.as_deref() {
            let Some(exe) = process.exe() else { continue };
            let exe = exe.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
            if !exe.starts_with(root) {
                continue;
            }
        }
        if let Some(profile) = info.profile.as_deref() {
            if !command_line_has_profile(process, profile) {
                continue;
            }
        }
        pids.push(pid.as_u32());
    }
    pids
}

/// 是否是已知的浏览器进程名（忽略大小写与 `.exe`）。
pub(crate) fn is_browser_process_name(name: &str) -> bool {
    let normalized = normalize_process_name(name);
    BROWSER_PROCESS_NAMES.contains(&normalized.as_str())
}

/// 规范化「显示名」：压缩空白 + 小写，便于做标题包含匹配。
pub(crate) fn normalize_display_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// 标题是否看起来像「普通浏览器标签页」窗口（而不是 PWA 独立窗口）。
///
/// 普通标签页的标题一般以 ` - Google Chrome` / ` - Microsoft Edge` 等结尾；
/// PWA（`--app-id=` / `--app=` 启动）的独立窗口标题通常只有应用/页面名。
/// 命中这里就**不算**「该 PWA 已打开」，避免「只是开了个网页标签」被误判成应用运行。
pub(crate) fn looks_like_browser_tab_title(title: &str) -> bool {
    let lower = normalize_display_name(title);
    if BROWSER_TITLE_SUFFIXES
        .iter()
        .any(|suffix| lower.ends_with(suffix))
    {
        return true;
    }
    // 标签组窗口标题形如 "A, B and 3 more pages - ..."，后缀已被上面覆盖；
    // 这里再兜底一次不带后缀的变体。
    lower.contains(" and ") && lower.ends_with(" more pages")
}

/// 窗口标题与显示名的匹配度：3=完全相同、2=前缀、1=包含、0=不匹配。
pub(crate) fn window_title_match_score(title: &str, display_name: &str) -> u8 {
    let display = normalize_display_name(display_name);
    // 单字符名字匹配噪音太大，直接不匹配。
    if display.chars().count() < 2 {
        return 0;
    }
    let title = normalize_display_name(title);
    if title.is_empty() || looks_like_browser_tab_title(&title) {
        return 0;
    }
    if title == display {
        3
    } else if title.starts_with(&display) {
        2
    } else if title.contains(&display) {
        1
    } else {
        0
    }
}

/// 在「属于该浏览器的可见窗口」里找标题匹配 display_name 的那个，返回其 pid。
fn browser_window_pid(browser_pids: &[u32], display_name: &str) -> Option<u32> {
    if browser_pids.is_empty() {
        return None;
    }
    let mut best: Option<(u8, u32)> = None;
    for window in all_titled_windows() {
        // 窗口必须可见，且所属进程确实是目标浏览器的进程（exe 名校验）。
        if !window.visible || !browser_pids.contains(&window.pid) {
            continue;
        }
        let score = window_title_match_score(&window.title, display_name);
        if score == 0 {
            continue;
        }
        if best.is_none_or(|(best_score, _)| score > best_score) {
            best = Some((score, window.pid));
        }
    }
    best.map(|(_, pid)| pid)
}

/// 解析启动目标：`.lnk` 先解析出真实 target，其余原样返回（同时展开 `%VAR%`）。
fn resolve_launch_target(spec: &AppLaunchSpec) -> String {
    let path = super::registry::expand_env_vars(spec.path.trim().trim_matches('"'));
    if path.is_empty() {
        return String::new();
    }
    if path.to_ascii_lowercase().ends_with(".lnk") {
        if let Some(link) = resolve_lnk(&path) {
            let target = link.target.trim().to_string();
            if !target.is_empty() {
                return target;
            }
        }
    }
    path
}

/// 路径比较用的规范化：去引号、统一分隔符、小写、去 verbatim 前缀。
fn normalize_path_key(path: &str) -> String {
    let trimmed = path.trim().trim_matches('"');
    trimmed
        .strip_prefix(r"\\?\")
        .unwrap_or(trimmed)
        .replace('/', "\\")
        .to_ascii_lowercase()
}

/// 进程命令行第一个参数或 exe 路径是否等于目标路径。
///
/// Sandboxie 等沙箱进程的 `ExecutablePath` 是宿主路径，但命令行里保留沙箱路径，
/// 因此必须优先比较命令行，才能区分同一 exe 在不同沙箱中的实例。
fn process_matches_target(process: &sysinfo::Process, target: &str) -> bool {
    let target_key = normalize_path_key(target);
    if target_key.is_empty() {
        return false;
    }
    if let Some(first) = process.cmd().first() {
        if normalize_path_key(&first.to_string_lossy()) == target_key {
            return true;
        }
    }
    process
        .exe()
        .is_some_and(|exe| normalize_path_key(&exe.to_string_lossy()) == target_key)
}

/// 找出命令行/exe 路径匹配目标路径的进程。
fn process_pids_for_target(target: &str) -> Vec<u32> {
    let key = normalize_path_key(target);
    if key.is_empty() {
        return Vec::new();
    }
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    system
        .processes()
        .iter()
        .filter(|(_, process)| process_matches_target(process, target))
        .map(|(pid, _)| pid.as_u32())
        .collect()
}

fn is_sandbox_target(target: &str) -> bool {
    target.to_ascii_lowercase().contains("\\sandbox\\")
}

/// 判断某个启动配置对应的应用是否已在运行，返回已存在的 pid。
///
/// `display_name` 是节点/应用显示名，用于窗口标题匹配（浏览器 PWA / Web App）。
///
/// 策略：
/// 1. 普通应用：优先按「解析后的目标路径 + 进程命令行第一个参数」匹配，能区分沙箱；
///    非沙箱路径匹配不到时再退回进程名；
/// 2. `.lnk`：先 `resolve_lnk` 拿 target 与参数再判断；
/// 3. 浏览器（chrome/msedge/brave/chromium/opera/vivaldi）：按安装目录判断归属，
///    浏览器应用支持 `--profile-directory` 区分 profile；PWA 只按可见窗口标题匹配
///    display_name，匹配不到就认为该 App 没打开；
/// 4. 沙箱目标（`\Sandbox\`）绝不按进程名兜底，避免把另一个沙箱的同名 QQ/微信误判；
/// 5. 任何不确定的情况返回 `None`（调用方会正常启动）。
// 说明：本函数是给 engine launcher 接入用的公开入口；在 engine 切换过来之前，
// 单编译 crate 会把它（及其内部 helper）判为「未使用」，这里显式放行，避免污染构建告警。
#[allow(dead_code)]
pub fn find_running_app(spec: &AppLaunchSpec, display_name: &str) -> Option<u32> {
    if spec.path.trim().is_empty() {
        return None;
    }
    let target = resolve_launch_target(spec);
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    // URL / shell: 协议类目标无法从进程或窗口可靠判定，交给启动流程。
    let lower_target = target.to_ascii_lowercase();
    if lower_target.contains("://") || lower_target.starts_with("shell:") {
        return None;
    }

    let exe_stem = process_name_from_path(target);
    let explicit = spec
        .process_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let process_name = normalize_process_name(explicit.unwrap_or(&exe_stem));
    if process_name.is_empty() {
        return None;
    }

    // 浏览器判定同时看「目标 exe / 安装路径」与「显式 process_name」：
    // PWA 快捷方式可能指向 chrome_proxy.exe / msedge_proxy.exe，也可能显式指定
    // processName=chrome；浏览器应用节点则只要对应安装目录的进程在跑就算已存在。
    let link_args = if spec.path.trim().to_ascii_lowercase().ends_with(".lnk") {
        resolve_lnk(spec.path.trim())
            .and_then(|link| link.args)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let args = format!("{} {}", spec.args.join(" "), link_args);
    if let Some(info) = browser_target_info(target, explicit, &args, display_name) {
        let pids = browser_target_pids(&info);
        if pids.is_empty() {
            return None;
        }
        if !info.is_pwa {
            // 浏览器应用（Chrome / Edge / Edge Beta、带 profile 的 Chrome 快捷方式）：
            // 有匹配 profile 的进程才算已存在。
            return pids.first().copied();
        }
        if normalize_display_name(display_name).chars().count() >= 2 {
            return browser_window_pid(&pids, display_name);
        }
        // 没有可用显示名时退回「浏览器本体在跑就算在跑」。
        return pids.first().copied();
    }

    // 非浏览器：优先按启动目标路径匹配，区分不同沙箱的同名进程；
    // 沙箱目标绝不按进程名兜底，避免把另一个沙箱的 QQ/微信误判为已存在。
    if let Some(pid) = process_pids_for_target(target).into_iter().next() {
        return Some(pid);
    }
    if is_sandbox_target(target) {
        return None;
    }
    process_pids(&process_name).first().copied()
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
    find_window_by_pids(&pids)
}

fn find_window_by_pids(pids: &[u32]) -> Option<WindowEntry> {
    let windows = all_titled_windows();
    windows
        .iter()
        .find(|window| window.visible && pids.contains(&window.pid))
        .or_else(|| windows.iter().find(|window| pids.contains(&window.pid)))
        .cloned()
}

fn find_window_by_target(target: &str) -> Option<WindowEntry> {
    let pids = process_pids_for_target(target);
    if pids.is_empty() {
        return None;
    }
    find_window_by_pids(&pids)
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

/// 等待指定 target 的窗口出现。
///
/// 与 `wait_for_window` 的区别：同一 exe 可能存在多个沙箱/profile 实例，
/// 这里只认启动目标对应的进程，不会误等/误关另一个实例。
pub async fn wait_for_target_window(
    target: &str,
    require_visible: bool,
    timeout: Duration,
    cancel: &Arc<AtomicBool>,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return false;
        }
        let pids = process_pids_for_target(target);
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
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(WAIT_WINDOW_POLL).await;
    }
}

/// 等待指定 pid 的窗口出现。
pub async fn wait_for_pid_window(
    pid: u32,
    require_visible: bool,
    timeout: Duration,
    cancel: &Arc<AtomicBool>,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return false;
        }
        if !require_visible && process_pids_for_pid(pid).is_empty() {
            // 进程可能已退出；继续等窗口没有意义。
            return false;
        }
        if find_main_window(pid).is_some() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(WAIT_WINDOW_POLL).await;
    }
}

fn process_pids_for_pid(pid: u32) -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    system
        .processes()
        .keys()
        .filter(|id| id.as_u32() == pid)
        .map(|id| id.as_u32())
        .collect()
}

/// 向指定 pid 的主窗口发送 WM_CLOSE。
pub fn close_window_for_pid(pid: u32) -> bool {
    let Some(window) = find_main_window(pid) else {
        return false;
    };
    unsafe { PostMessageW(window.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok() }
}

/// 把指定 pid 的主窗口置前。
pub fn focus_window_for_pid(pid: u32) -> bool {
    let Some(window) = find_main_window(pid) else {
        return false;
    };
    activate_window(&window)
}

/// 向主窗口发送 WM_CLOSE。
pub fn close_main_window(process_name: &str) -> bool {
    let Some(window) = find_window_by_process_name(process_name) else {
        return false;
    };
    unsafe { PostMessageW(window.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok() }
}

/// 向指定启动目标（同一 exe 的多个实例也能区分）的主窗口发送 WM_CLOSE。
pub fn close_target_main_window(target: &str) -> bool {
    let Some(window) = find_window_by_target(target) else {
        return false;
    };
    unsafe { PostMessageW(window.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok() }
}

/// 把主窗口置前。
pub fn focus_main_window(process_name: &str) -> bool {
    let Some(window) = find_window_by_process_name(process_name) else {
        return false;
    };
    activate_window(&window)
}

/// 把指定启动目标的主窗口置前。
pub fn focus_target_main_window(target: &str) -> bool {
    let Some(window) = find_window_by_target(target) else {
        return false;
    };
    activate_window(&window)
}

fn activate_window(window: &WindowEntry) -> bool {
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

    #[test]
    fn escape_win_arg_cases() {
        // 不需要引号的原样返回
        assert_eq!(escape_win_arg("simple"), "simple");
        assert_eq!(escape_win_arg(r"a\b\c"), r"a\b\c");
        // 含空格 → 加引号
        assert_eq!(
            escape_win_arg(r"C:\Program Files\App\app.exe"),
            r#""C:\Program Files\App\app.exe""#
        );
        // 空参数要保留成一个空字符串
        assert_eq!(escape_win_arg(""), r#""""#);
        // 内嵌引号 → 反斜杠转义
        assert_eq!(escape_win_arg(r#"say "hi""#), r#""say \"hi\"""#);
        // 引号前的反斜杠要加倍，结尾反斜杠也要加倍
        assert_eq!(escape_win_arg(r#"a\b c\"d"#), r#""a\b c\\\"d""#);
        assert_eq!(escape_win_arg("dir with space\\"), r#""dir with space\\""#);
    }

    #[test]
    fn build_command_line_quotes_exe_and_args() {
        assert_eq!(
            build_command_line(
                r"C:\Windows\System32\notepad.exe",
                &["--x".to_string(), "hello world".to_string()]
            ),
            r#""C:\Windows\System32\notepad.exe" --x "hello world""#
        );
        assert_eq!(
            build_command_line(r"C:\Program Files\App\app.exe", &[]),
            r#""C:\Program Files\App\app.exe""#
        );
    }

    #[test]
    fn environment_block_applies_overrides() {
        let overrides = vec![
            crate::model::EnvVar {
                key: "DAEMONMAN_SMOKE".to_string(),
                value: "1".to_string(),
            },
            // 与 PATH 大小写不同，应该覆盖而不是新增第二份
            crate::model::EnvVar {
                key: "path".to_string(),
                value: r"C:\daemonman-custom".to_string(),
            },
        ];
        let block = build_environment_block(&overrides).expect("有覆盖时应生成环境块");

        let mut entries: Vec<(String, String)> = Vec::new();
        let mut start = 0usize;
        for index in 0..block.len() {
            if block[index] != 0 {
                continue;
            }
            if index == start {
                break; // 双 NUL 结尾
            }
            let text = String::from_utf16_lossy(&block[start..index]);
            if let Some((key, value)) = text.split_once('=') {
                entries.push((key.to_uppercase(), value.to_string()));
            }
            start = index + 1;
        }
        assert!(entries.contains(&("DAEMONMAN_SMOKE".to_string(), "1".to_string())));
        assert!(entries.contains(&(
            "PATH".to_string(),
            r"C:\daemonman-custom".to_string()
        )));
        assert_eq!(
            entries.iter().filter(|(key, _)| key == "PATH").count(),
            1,
            "环境变量名不区分大小写，覆盖后不应出现两份 PATH"
        );
        // 以双 NUL 结尾，且不出现 `=C:` 这类隐藏变量
        assert_eq!(block.last(), Some(&0u16));
        assert_eq!(block.get(block.len().saturating_sub(2)), Some(&0u16));
        assert!(!entries.iter().any(|(key, _)| key.starts_with('=')));

        // 没有覆盖时直接继承父进程环境
        assert!(build_environment_block(&[]).is_none());
    }

    #[test]
    fn browser_process_detection() {
        for name in ["chrome", "Chrome.EXE", " msedge ", "brave", "chromium", "opera", "vivaldi"] {
            assert!(is_browser_process_name(name), "{name} 应识别为浏览器");
        }
        for name in ["firefox", "chrome_proxy", "msedgewebview2", "explorer", ""] {
            assert!(!is_browser_process_name(name), "{name} 不应识别为浏览器");
        }
    }

    #[test]
    fn profile_directory_parsing() {
        assert_eq!(
            profile_directory_from_args(r#"--profile-directory="Profile 1""#).as_deref(),
            Some("Profile 1")
        );
        assert_eq!(
            profile_directory_from_args("--profile-directory=Default").as_deref(),
            Some("Default")
        );
        assert_eq!(
            profile_directory_from_args(r#"--profile-directory "Profile 2""#).as_deref(),
            Some("Profile 2")
        );
        assert_eq!(profile_directory_from_args("--profile-directory").as_deref(), None);
        assert_eq!(profile_directory_from_args("--app-id=abc").as_deref(), None);
    }

    #[test]
    fn browser_proxy_target_mapping() {
        // Brave 的 PWA 快捷方式指向 Brave 安装目录里的 chrome_proxy.exe。
        let brave = browser_target_info(
            r"C:\Users\me\AppData\Local\BraveSoftware\Brave-Browser\Application\chrome_proxy.exe",
            None,
            "",
            "Discord",
        )
        .expect("Brave PWA");
        assert_eq!(brave.browser, "brave");
        assert_eq!(brave.proxy, "brave_proxy");
        assert!(brave.is_pwa);

        let edge = browser_target_info(
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge_proxy.exe",
            None,
            "",
            "Element",
        )
        .expect("Edge PWA");
        assert_eq!(edge.browser, "msedge");
        assert!(edge.is_pwa);

        let chrome = browser_target_info(
            r"C:\Program Files\Google\Chrome\Application\chrome_proxy.exe",
            None,
            "",
            "Some App",
        )
        .expect("Chrome PWA");
        assert_eq!(chrome.browser, "chrome");
        assert!(chrome.is_pwa);

        // 普通浏览器应用（带 --profile-directory）不是 PWA，进程在跑就应跳过。
        let profile = browser_target_info(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            Some("chrome"),
            "--profile-directory=Profile 2",
            "Chrome (Dynesshely)",
        )
        .expect("Chrome app");
        assert_eq!(profile.browser, "chrome");
        assert!(!profile.is_pwa);

        assert_eq!(
            browser_target_info(
                r"C:\Program Files (x86)\Microsoft\Edge Beta\Application\msedge.exe",
                None,
                "",
                "Edge Beta"
            )
            .map(|info| info.browser),
            Some("msedge")
        );
        assert!(browser_target_info(r"C:\Windows\notepad.exe", None, "", "记事本").is_none());
    }

    #[test]
    fn display_name_normalization() {
        assert_eq!(normalize_display_name("  Gmail  "), "gmail");
        assert_eq!(normalize_display_name("Visual   Studio   Code"), "visual studio code");
        assert_eq!(normalize_display_name("网易云音乐"), "网易云音乐");
        assert_eq!(normalize_display_name("   "), "");
    }

    #[test]
    fn browser_tab_title_filter() {
        // 普通浏览器标签窗口：带浏览器名后缀 → 不算 PWA 已打开
        assert!(looks_like_browser_tab_title("Gmail - Google Chrome"));
        assert!(looks_like_browser_tab_title("Inbox - Microsoft Edge"));
        assert!(looks_like_browser_tab_title("YouTube - Brave"));
        assert!(looks_like_browser_tab_title("A, B and 3 more pages - Google Chrome"));
        // PWA 独立窗口
        assert!(!looks_like_browser_tab_title("Gmail"));
        assert!(!looks_like_browser_tab_title("Notion – Notes"));
        assert!(!looks_like_browser_tab_title("Inbox (3) - Gmail"));
    }

    #[test]
    fn window_title_match_scoring() {
        assert_eq!(window_title_match_score("Gmail", "gmail"), 3);
        assert_eq!(window_title_match_score("Gmail - Inbox", "Gmail"), 2);
        assert_eq!(window_title_match_score("Inbox - Gmail", "gmail"), 1);
        assert_eq!(window_title_match_score("Notion", "gmail"), 0);
        // 单字符显示名不参与匹配
        assert_eq!(window_title_match_score("Gmail", "g"), 0);
        // 普通浏览器标签页不参与匹配
        assert_eq!(window_title_match_score("Gmail - Google Chrome", "Gmail"), 0);
        assert_eq!(window_title_match_score("", "Gmail"), 0);
    }

    #[test]
    fn find_running_app_detects_self_process() {
        let exe = std::env::current_exe().expect("无法获取当前测试进程路径");
        let spec = AppLaunchSpec {
            path: exe.display().to_string(),
            ..Default::default()
        };
        let found = find_running_app(&spec, "");
        println!("当前测试进程 pid = {}，识别结果 = {found:?}", std::process::id());
        assert_eq!(found, Some(std::process::id()), "普通应用应通过进程名识别到自身");
    }

    #[test]
    fn find_running_app_returns_none_for_urls_and_empty_path() {
        let url = AppLaunchSpec {
            path: "https://example.com/app".to_string(),
            ..Default::default()
        };
        assert!(find_running_app(&url, "Example").is_none());

        let shell_target = AppLaunchSpec {
            path: "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".to_string(),
            ..Default::default()
        };
        assert!(find_running_app(&shell_target, "计算器").is_none());

        assert!(find_running_app(&AppLaunchSpec::default(), "任意").is_none());
    }

    #[test]
    fn find_running_app_browser_needs_title_match() {
        // 浏览器目标 + 不可能存在的显示名：即使 chrome 正在运行也不能判定为「已运行」。
        let spec = AppLaunchSpec {
            path: r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string(),
            ..Default::default()
        };
        assert!(
            find_running_app(&spec, "DaemonMan-Impossible-Window-Title-9f3c1a").is_none(),
            "浏览器本体在跑不代表某个 PWA 已打开"
        );
        // 显示名过短（<2 字符）时退回「浏览器在跑就算在跑」，不会误判 None 以外的值。
        let _ = find_running_app(&spec, "x");
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

    /// 冒烟测试：验证普通启动的子进程父进程是 shell（explorer）而不是 DaemonMan。
    ///
    /// 手动运行：`cargo test --lib launch_parent_smoke -- --ignored --nocapture`
    /// （会短暂启动 mstsc 再结束它；用 GUI 进程是为了保证查询父进程时它仍然存活）
    #[test]
    #[ignore]
    fn launch_parent_smoke() {
        let shell_pid = unsafe {
            let shell = GetShellWindow();
            let mut pid = 0u32;
            if !shell.is_invalid() {
                GetWindowThreadProcessId(shell, Some(&mut pid));
            }
            (pid != 0).then_some(pid)
        };
        println!("当前 shell(explorer) pid = {shell_pid:?}，DaemonMan 测试进程 pid = {}", std::process::id());

        let spec = AppLaunchSpec {
            path: r"C:\Windows\System32\mstsc.exe".to_string(),
            ..Default::default()
        };
        let pid = launch_app(&spec).expect("启动 mstsc 失败");
        println!("启动的 mstsc pid = {pid}");
        assert!(pid > 0, "CreateProcessW 应返回有效 pid");

        // 父进程 PID：sysinfo 的 parent() 只有在刷新 parent 信息后才有值，
        // 这里用 new_all()（refresh everything）并等待 cmd 真正起来；仍取不到则用 CIM 兜底。
        let child = sysinfo::Pid::from_u32(pid);
        let mut system = sysinfo::System::new_all();
        let mut parent_pid = None;
        for _ in 0..20 {
            system.refresh_all();
            parent_pid = system
                .process(child)
                .and_then(|process| process.parent())
                .map(|parent| parent.as_u32());
            if parent_pid.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        println!(
            "子进程 {pid} 的父进程 pid = {parent_pid:?}（当前进程 = {}）",
            std::process::id()
        );
        if parent_pid.is_none() {
            parent_pid = parent_pid_via_cim(pid);
            println!("改用 Get-CimInstance 查询父进程 = {parent_pid:?}");
        }

        assert_ne!(
            parent_pid,
            Some(std::process::id()),
            "父进程不应是 DaemonMan"
        );
        if let Some(shell_pid) = shell_pid {
            assert_eq!(
                parent_pid,
                Some(shell_pid),
                "父进程应为 shell(explorer) 进程；若失败请检查 GetShellWindow/OpenProcess 回退日志"
            );
        } else {
            println!("未取到 shell pid（可能无交互桌面），只断言父进程不是 DaemonMan");
        }

        // 清理：直接结束 mstsc（未建立连接，结束无副作用）。
        let _ = kill_process(pid);
        println!("已结束 mstsc");
    }

    /// 用 CIM 查询某个进程的父进程 pid（sysinfo 取不到时的兜底，只在冒烟测试里用）。
    fn parent_pid_via_cim(pid: u32) -> Option<u32> {
        let script = format!(
            "(Get-CimInstance Win32_Process -Filter 'ProcessId={pid}').ParentProcessId"
        );
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }

    /// 冒烟测试：已运行检测（含浏览器 PWA 快捷方式）。
    ///
    /// 只读：不启动任何程序。会扫描开始菜单里的 .lnk，找出目标为浏览器且带
    /// `--app-id=` / `--app=` 的 PWA 快捷方式，打印它们当前是否被识别为「已打开」。
    ///
    /// 手动运行：`cargo test --lib find_running_app_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn find_running_app_smoke() {
        // 1) 当前测试进程应被识别为「已在运行」。
        let current_exe = std::env::current_exe().expect("无法获取当前测试进程路径");
        let self_spec = AppLaunchSpec {
            path: current_exe.display().to_string(),
            ..Default::default()
        };
        let self_found = find_running_app(&self_spec, "");
        println!(
            "自身进程识别：pid = {self_found:?}（当前 = {}）",
            std::process::id()
        );
        assert_eq!(self_found, Some(std::process::id()));

        // 2) 浏览器进程现状。
        for browser in BROWSER_PROCESS_NAMES {
            let pids = process_pids(browser);
            if pids.is_empty() {
                continue;
            }
            let titles: Vec<(u32, Option<String>)> = pids
                .iter()
                .map(|pid| (*pid, main_window_title(*pid)))
                .collect();
            println!("浏览器 {browser} 正在运行：{titles:?}");
        }

        // 3) 扫描开始菜单里的 PWA 快捷方式（只读，不启动）。
        fn scan_lnks(dir: &std::path::Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
            if depth > 3 {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    scan_lnks(&path, depth + 1, out);
                } else if path
                    .extension()
                    .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("lnk"))
                {
                    out.push(path);
                }
            }
        }

        let mut start_menus: Vec<std::path::PathBuf> = Vec::new();
        if let Some(appdata) = std::env::var_os("APPDATA") {
            start_menus.push(
                std::path::PathBuf::from(appdata)
                    .join(r"Microsoft\Windows\Start Menu\Programs"),
            );
        }
        if let Some(program_data) = std::env::var_os("ProgramData") {
            start_menus.push(
                std::path::PathBuf::from(program_data)
                    .join(r"Microsoft\Windows\Start Menu\Programs"),
            );
        }
        if let Some(desktop) = dirs::desktop_dir() {
            start_menus.push(desktop);
        }

        let mut lnks: Vec<std::path::PathBuf> = Vec::new();
        for dir in &start_menus {
            scan_lnks(dir, 0, &mut lnks);
        }

        let mut browser_apps = 0usize;
        for lnk in &lnks {
            let Some(link) = resolve_lnk(&lnk.display().to_string()) else {
                continue;
            };
            let exe_stem = process_name_from_path(&link.target);
            if !is_browser_process_name(&exe_stem) {
                continue;
            }
            let args = link.args.unwrap_or_default();
            if !args.contains("--app-id=") && !args.contains("--app=") {
                continue;
            }
            browser_apps += 1;
            let display_name = lnk
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default();
            let spec = AppLaunchSpec {
                path: lnk.display().to_string(),
                args: split_args(&args),
                ..Default::default()
            };
            let found = find_running_app(&spec, &display_name);
            println!(
                "PWA 快捷方式「{display_name}」→ target={}，识别 pid = {found:?}",
                link.target
            );
        }
        // 4) 用真实窗口标题验证标题匹配：PWA 风格标题应命中，普通标签页标题应被过滤。
        let mut browser_pid_names: std::collections::HashMap<u32, &'static str> =
            std::collections::HashMap::new();
        for browser in BROWSER_PROCESS_NAMES {
            for pid in process_pids(browser) {
                browser_pid_names.insert(pid, browser);
            }
        }
        for window in all_titled_windows() {
            let Some(browser) = browser_pid_names.get(&window.pid).copied() else {
                continue;
            };
            if !window.visible {
                continue;
            }
            let display = window
                .title
                .split(" - ")
                .next()
                .unwrap_or(&window.title)
                .trim()
                .to_string();
            if display.chars().count() < 2 {
                continue;
            }
            let spec = AppLaunchSpec {
                path: format!("{browser}.exe"),
                ..Default::default()
            };
            let found = find_running_app(&spec, &display);
            let tab_like = looks_like_browser_tab_title(&window.title);
            println!(
                "窗口「{}」（pid={}，{}）→ display「{display}」匹配 pid = {found:?}，普通标签页={tab_like}",
                window.title, window.pid, browser
            );
            if !tab_like {
                // PWA 风格标题必须能被识别为「已运行」。
                // （不强制 pid 相等：同一浏览器可能有两个标题相近的窗口。）
                assert!(
                    found.is_some(),
                    "PWA 风格窗口「{}」未被识别为已在运行",
                    window.title
                );
            }
        }
        println!("扫描 {} 个 .lnk，其中浏览器 PWA 快捷方式 {browser_apps}。", lnks.len());
    }
}
