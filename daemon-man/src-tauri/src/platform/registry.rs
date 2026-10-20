//! 注册表自启动项（HKCU/HKLM 的 Run、RunOnce，含 WOW6432Node 32 位视图）。
//!
//! 数据来源：
//! - `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! - `HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce`
//! - `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`
//! - `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce`
//! - `HKLM\SOFTWARE\WOW6432Node\...`（用 `KEY_WOW64_32KEY` 读 32 位视图）
//!
//! 启用/禁用完全走 `StartupApproved`（见 `startup_approved` 模块），原始 Run 值不动。
//!
//! id 形如 `registry-run::user::OneDrive`：`scope` 已经唯一确定了注册表视图（user /
//! machine / machine32），`locator` 就是值名，所以重新扫描后 id 稳定，且能反推出键路径。

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use winreg::enums::*;
use winreg::{RegKey, RegValue};

use crate::model::{AutostartItem, AutostartSource};

use super::startup_approved::{self, Hive, APPROVED_RUN, APPROVED_RUN32};

/// 64 位 / 用户视图下的 Run 键。
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// 64 位 / 用户视图下的 RunOnce 键。
const RUN_ONCE: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
/// WOW6432Node（32 位视图）下的 Run 键；配合 `KEY_WOW64_32KEY` 访问。
const RUN_WOW: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// WOW6432Node（32 位视图）下的 RunOnce 键。
const RUN_ONCE_WOW: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";

/// DaemonMan 自身在 HKCU Run 中使用的值名（开发实例用独立值名，避免覆盖生产自启）。
fn self_value_name() -> &'static str {
    if crate::config::is_dev() {
        "DaemonManDev"
    } else {
        "DaemonMan"
    }
}

/// 一个注册表自启动「位置」：由 (来源, 视图) 唯一确定。
#[derive(Debug, Clone, Copy)]
struct Location {
    source: AutostartSource,
    hive: Hive,
    key_path: &'static str,
    approved_key: &'static str,
    /// StartupApproved 键所在的视图：Run32 永远在 64 位（native）视图下，
    /// 只有 Run 键本身需要 KEY_WOW64_32KEY 才能看到 WOW6432Node。
    approved_hive: Hive,
    /// 写入 `AutostartItem::scope`，同时用于从 id 反查位置
    scope: &'static str,
}

/// 当前支持的所有注册表位置。
fn locations() -> [Location; 6] {
    [
        Location {
            source: AutostartSource::RegistryRun,
            hive: Hive::CurrentUser,
            key_path: RUN,
            approved_key: APPROVED_RUN,
            approved_hive: Hive::CurrentUser,
            scope: "user",
        },
        Location {
            source: AutostartSource::RegistryRunOnce,
            hive: Hive::CurrentUser,
            key_path: RUN_ONCE,
            approved_key: APPROVED_RUN,
            approved_hive: Hive::CurrentUser,
            scope: "user",
        },
        Location {
            source: AutostartSource::RegistryRun,
            hive: Hive::LocalMachine,
            key_path: RUN,
            approved_key: APPROVED_RUN,
            approved_hive: Hive::LocalMachine,
            scope: "machine",
        },
        Location {
            source: AutostartSource::RegistryRunOnce,
            hive: Hive::LocalMachine,
            key_path: RUN_ONCE,
            approved_key: APPROVED_RUN,
            approved_hive: Hive::LocalMachine,
            scope: "machine",
        },
        Location {
            source: AutostartSource::RegistryRun,
            hive: Hive::LocalMachine32,
            key_path: RUN_WOW,
            approved_key: APPROVED_RUN32,
            approved_hive: Hive::LocalMachine,
            scope: "machine32",
        },
        Location {
            source: AutostartSource::RegistryRunOnce,
            hive: Hive::LocalMachine32,
            key_path: RUN_ONCE_WOW,
            approved_key: APPROVED_RUN32,
            approved_hive: Hive::LocalMachine,
            scope: "machine32",
        },
    ]
}

fn location_for(source: AutostartSource, scope: &str) -> Option<Location> {
    locations()
        .into_iter()
        .find(|loc| loc.source == source && loc.scope == scope)
}

/// 扫描所有注册表自启动项。
pub fn scan() -> Vec<AutostartItem> {
    let mut items = Vec::new();
    for loc in locations() {
        match scan_location(&loc) {
            Ok(mut found) => items.append(&mut found),
            Err(err) => eprintln!(
                "[platform] 扫描注册表自启动（{} {}）失败，已跳过：{err:#}",
                loc.hive.label(),
                loc.key_path
            ),
        }
    }
    items
}

fn scan_location(loc: &Location) -> Result<Vec<AutostartItem>> {
    let key = match startup_approved::open(loc.hive, loc.key_path, false) {
        Ok(key) => key,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("打开 {}\\{} 失败", loc.hive.label(), loc.key_path)
            })
        }
    };

    // StartupApproved 先整键读一遍，避免每个值都开一次键。
    let approved = approved_map(loc);

    let mut items = Vec::new();
    for entry in key.enum_values() {
        let (value_name, _raw) = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!(
                    "[platform] 跳过 {}\\{} 中无法读取的值：{err}",
                    loc.hive.label(),
                    loc.key_path
                );
                continue;
            }
        };

        let raw_command = match key.get_value::<String, _>(&value_name) {
            Ok(command) => command,
            Err(err) => {
                // 值类型不是 REG_SZ / REG_EXPAND_SZ 的（极少见）直接跳过。
                eprintln!(
                    "[platform] 跳过注册表自启动项「{value_name}」：命令不是字符串（{err}）"
                );
                continue;
            }
        };
        let command = expand_env_vars(raw_command.trim());
        if command.trim().is_empty() {
            continue;
        }

        let (exe, args) = parse_command_line(&command);
        let executable = resolve_executable(&exe);
        let display_name = if value_name.trim().is_empty() {
            exe.rsplit('\\')
                .next()
                .unwrap_or(exe.as_str())
                .trim_end_matches(".exe")
                .to_string()
        } else {
            value_name.clone()
        };

        // RunOnce 由系统在登录时执行一次并自动删除，Windows 没有给它提供
        // StartupApproved 开关（写进去也不会生效），因此标为不支持开关。
        let toggle_supported = loc.source != AutostartSource::RegistryRunOnce;
        let enabled = if toggle_supported {
            approved.get(&value_name).copied().unwrap_or(true)
        } else {
            true
        };

        let is_self = value_name.eq_ignore_ascii_case(self_value_name())
            || executable
                .as_deref()
                .is_some_and(super::proc::is_current_exe);

        let mut details: Vec<&str> = Vec::new();
        if loc.scope == "machine32" {
            details.push("32 位视图（WOW6432Node）");
        }
        if !toggle_supported {
            details.push("一次性执行（RunOnce），Windows 未提供启停开关");
        }
        if is_self {
            details.push("DaemonMan 自身");
        }
        if loc.hive.is_machine() {
            details.push("HKLM 项，开关需要管理员权限");
        }

        items.push(AutostartItem {
            id: format!(
                "{}::{}::{}",
                super::source_key(loc.source),
                loc.scope,
                value_name
            ),
            name: display_name,
            source: loc.source,
            source_label: loc.source.label().to_string(),
            command,
            executable,
            args,
            enabled,
            toggle_supported,
            can_delete: true,
            requires_admin: loc.hive.is_machine(),
            is_self,
            taken_over: false,
            scope: loc.scope.to_string(),
            locator: value_name,
            detail: if details.is_empty() {
                None
            } else {
                Some(details.join("；"))
            },
        });
    }
    Ok(items)
}

/// 一次性读出某个位置的 StartupApproved 状态（值名 → 是否启用）。
fn approved_map(loc: &Location) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    let Ok(key) = startup_approved::open(loc.approved_hive, loc.approved_key, false) else {
        return map;
    };
    for entry in key.enum_values() {
        match entry {
            Ok((name, value)) => {
                if let Some(enabled) = startup_approved::parse_approval(&value.bytes) {
                    map.insert(name, enabled);
                }
            }
            Err(err) => eprintln!(
                "[platform] 读取 {}\\{} 的 StartupApproved 值失败：{err}",
                loc.approved_hive.label(),
                loc.approved_key
            ),
        }
    }
    map
}

/// 启用 / 禁用某个注册表自启动项（写 StartupApproved，非破坏性）。
pub fn set_enabled(item: &AutostartItem, enabled: bool) -> Result<()> {
    if item.source == AutostartSource::RegistryRunOnce {
        anyhow::bail!(
            "「{}」是 RunOnce 项，Windows 不支持对一次性启动项做启用/禁用；请在注册表中直接删除该值",
            item.name
        );
    }
    let loc = location_for(item.source, &item.scope).ok_or_else(|| {
        anyhow::anyhow!(
            "未知的注册表自启动位置：{:?} / {}",
            item.source,
            item.scope
        )
    })?;
    if item.locator.is_empty() {
        anyhow::bail!("「{}」没有可定位的注册表值名，无法开关", item.name);
    }
    startup_approved::write_enabled(loc.approved_hive, loc.approved_key, &item.locator, enabled)
        .with_context(|| format!("切换注册表自启动项「{}」失败", item.name))
}

// ---------------------------------------------------------------------------
// 备份 / 删除 / 还原：需要原始注册表值（保留类型与字节）
// ---------------------------------------------------------------------------

/// 一个注册表自启动项的原始值，用于备份后还原。
#[derive(Debug, Clone)]
pub(crate) struct RawRegistryValue {
    pub hive: Hive,
    pub key_path: &'static str,
    pub value_name: String,
    pub value_type: u32,
    pub bytes: Vec<u8>,
}

/// 读取某个注册表自启动项的原始值；键或值不存在时返回 `Ok(None)`。
pub(crate) fn read_raw_value(
    source: AutostartSource,
    scope: &str,
    locator: &str,
) -> Result<Option<RawRegistryValue>> {
    let loc = location_for(source, scope).ok_or_else(|| {
        anyhow::anyhow!("未知的注册表自启动位置：{source:?} / {scope}")
    })?;
    let key = match startup_approved::open(loc.hive, loc.key_path, false) {
        Ok(key) => key,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("打开 {}\\{} 失败", loc.hive.label(), loc.key_path)
            })
        }
    };
    match key.get_raw_value(locator) {
        Ok(raw) => Ok(Some(RawRegistryValue {
            hive: loc.hive,
            key_path: loc.key_path,
            value_name: locator.to_string(),
            value_type: startup_approved::reg_type_to_u32(raw.vtype),
            bytes: raw.bytes,
        })),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| {
            format!(
                "读取注册表值失败：{}\\{} → {locator}",
                loc.hive.label(),
                loc.key_path
            )
        }),
    }
}

/// 把备份的原始注册表值写回（restore 用）。
pub(crate) fn write_raw_value(
    hive: Hive,
    key_path: &str,
    value_name: &str,
    value_type: u32,
    bytes: &[u8],
) -> Result<()> {
    let key = startup_approved::open_or_create(hive, key_path)?;
    let value = RegValue {
        bytes: bytes.to_vec(),
        vtype: startup_approved::reg_type_from_u32(value_type),
    };
    key.set_raw_value(value_name, &value)
        .with_context(|| {
            format!(
                "写入注册表值失败：{}\\{key_path} → {value_name}（HKLM 需要管理员权限）",
                hive.label()
            )
        })
}

/// 永久删除注册表自启动项：删除 Run/RunOnce 值，并清掉对应的 StartupApproved 开关值。
pub(crate) fn delete_value(source: AutostartSource, scope: &str, locator: &str) -> Result<()> {
    let loc = location_for(source, scope).ok_or_else(|| {
        anyhow::anyhow!("未知的注册表自启动位置：{source:?} / {scope}")
    })?;
    if let Ok(key) = startup_approved::open(loc.hive, loc.key_path, true) {
        match key.delete_value(locator) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "删除注册表值失败：{}\\{} → {locator}",
                        loc.hive.label(),
                        loc.key_path
                    )
                })
            }
        }
    }
    // StartupApproved 只是辅助开关，删除失败不影响主值已删除的事实。
    if let Err(err) =
        startup_approved::delete_value(loc.approved_hive, loc.approved_key, locator)
    {
        eprintln!("[platform] 清理 StartupApproved 值失败（可忽略）：{err:#}");
    }
    Ok(())
}

/// 还原后清除禁用状态，让系统重新认可该自启动项。
pub(crate) fn clear_disabled_state(
    source: AutostartSource,
    scope: &str,
    locator: &str,
) -> Result<()> {
    // RunOnce 没有 StartupApproved 机制。
    if source == AutostartSource::RegistryRunOnce {
        return Ok(());
    }
    let loc = location_for(source, scope).ok_or_else(|| {
        anyhow::anyhow!("未知的注册表自启动位置：{source:?} / {scope}")
    })?;
    startup_approved::write_enabled(loc.approved_hive, loc.approved_key, locator, true)
        .with_context(|| format!("清除注册表自启动项的禁用状态失败：{locator}"))
}

/// DaemonMan 自身是否已注册开机自启。
pub fn is_self_enabled() -> bool {
    let Ok(key) = startup_approved::open(Hive::CurrentUser, RUN, false) else {
        return false;
    };
    if key.get_value::<String, _>(self_value_name()).is_err() {
        return false;
    }
    // 存在 Run 值还不够：可能被任务管理器/StartupApproved 关掉了。
    startup_approved::read_enabled(Hive::CurrentUser, APPROVED_RUN, self_value_name()).unwrap_or(true)
}

/// 注册 / 取消 DaemonMan 自身的开机自启。
///
/// 命令固定为 `"<当前 exe>" --autostart`，与 `lib.rs` 里读取的参数一致。
pub fn set_self_enabled(enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if enabled {
        let exe = std::env::current_exe().context("无法获取 DaemonMan 可执行文件路径")?;
        let command = format!("\"{}\" --autostart", exe.display());
        let (key, _) = hkcu
            .create_subkey(RUN)
            .context("打开 HKCU 的 Run 键失败")?;
        key.set_value(self_value_name(), &command)
            .context("写入 DaemonMan 自启动注册表值失败")?;
        // 若这个值以前被禁用过（StartupApproved 里是 0x03），必须一起改回启用，
        // 否则写 Run 值也不会生效。
        startup_approved::write_enabled(Hive::CurrentUser, APPROVED_RUN, self_value_name(), true)
            .context("启用 DaemonMan 自启动的 StartupApproved 状态失败")?;
    } else {
        if let Ok(key) = hkcu.open_subkey_with_flags(RUN, KEY_SET_VALUE) {
            match key.delete_value(self_value_name()) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err).context("删除 DaemonMan 自启动注册表值失败"),
            }
        }
        startup_approved::delete_value(Hive::CurrentUser, APPROVED_RUN, self_value_name())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 纯函数：命令行解析 / 环境变量展开 / 可执行文件定位（便于单测）
// ---------------------------------------------------------------------------

/// 展开命令行中的 `%VAR%` 环境变量。
pub(crate) fn expand_env_vars(raw: &str) -> String {
    expand_with(raw, |name| std::env::var(name).ok())
}

/// 可注入查表函数的环境变量展开，便于单测。
pub(crate) fn expand_with(raw: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        // `%` 之间不允许出现 `%` 本身，长度上限防止把一整行都当成变量名。
        match after.find('%') {
            Some(end) if end > 0 && end <= 128 && after.is_char_boundary(end) => {
                let name = &after[..end];
                match lookup(name) {
                    Some(value) => out.push_str(&value),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                // 落单的 `%`：原样输出并继续；-1 字节避免死循环（`%` 是 1 字节）
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 把「注册表里的命令行」切成 (可执行文件, 参数串)。
///
/// 需要处理的真实形态：
/// - `"C:\Program Files\App\app.exe" --min`
/// - `C:\Program Files\App\app.exe --min`
/// - `C:\Windows\System32\rundll32.exe C:\app.dll,Entry`
/// - `OneDrive.exe /background`（不带路径，后面在 PATH 里找）
pub(crate) fn parse_command_line(raw: &str) -> (String, Option<String>) {
    let text = raw.trim();
    if text.is_empty() {
        return (String::new(), None);
    }

    let (exe, rest) = if let Some(stripped) = text.strip_prefix('"') {
        match stripped.find('"') {
            Some(end) => (&stripped[..end], &stripped[end + 1..]),
            None => (stripped, ""),
        }
    } else if let Some(end) = find_program_end(text) {
        (&text[..end], &text[end..])
    } else {
        match text.find(char::is_whitespace) {
            Some(end) => (&text[..end], &text[end..]),
            None => (text, ""),
        }
    };

    let exe = exe.trim().trim_matches('"').trim().to_string();
    let rest = rest.trim();
    (
        exe,
        if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        },
    )
}

/// 在未加引号的命令行里找到可执行文件部分的结束位置（扩展名之后）。
fn find_program_end(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let mut best: Option<usize> = None;
    for ext in [".exe", ".com", ".bat", ".cmd", ".dll"] {
        let mut offset = 0usize;
        while let Some(pos) = lower[offset..].find(ext) {
            let end = offset + pos + ext.len();
            // 扩展名必须紧跟空白或字符串结尾，避免匹配 `foo.exe.config`。
            let boundary = text
                .get(end..)
                .map(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace) || tail.starts_with(','))
                .unwrap_or(false);
            if boundary {
                best = Some(match best {
                    Some(prev) => prev.min(end),
                    None => end,
                });
                break;
            }
            offset = end;
        }
    }
    best
}

/// 定位可执行文件的绝对路径（尽力而为）。
///
/// - 绝对路径 / 带目录的相对路径：原样返回（即使当前不存在，也交给启动阶段报错）；
/// - 只有文件名：在 PATH 中查找，找不到则原样返回文件名。
pub(crate) fn resolve_executable(exe: &str) -> Option<String> {
    let exe = exe.trim().trim_matches('"');
    if exe.is_empty() {
        return None;
    }
    let path = Path::new(exe);
    if path.is_file() {
        return Some(path.display().to_string());
    }
    if path.is_absolute() || exe.contains('\\') || exe.contains('/') {
        return Some(exe.to_string());
    }
    Some(find_in_path(exe).unwrap_or_else(|| exe.to_string()))
}

/// 在 PATH 中查找可执行文件。
fn find_in_path(name: &str) -> Option<String> {
    let has_ext = Path::new(name).extension().is_some();
    let candidates: Vec<String> = if has_ext {
        vec![name.to_string()]
    } else {
        vec![format!("{name}.exe"), name.to_string()]
    };
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for candidate in &candidates {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full.display().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quoted_command() {
        let (exe, args) = parse_command_line(r#""C:\Program Files\App\app.exe" --min --silent"#);
        assert_eq!(exe, r"C:\Program Files\App\app.exe");
        assert_eq!(args.as_deref(), Some("--min --silent"));
    }

    #[test]
    fn parse_unquoted_command_with_spaces_in_args() {
        let (exe, args) = parse_command_line(r"C:\Program Files\App\app.exe --title hello world");
        assert_eq!(exe, r"C:\Program Files\App\app.exe");
        assert_eq!(args.as_deref(), Some("--title hello world"));
    }

    #[test]
    fn parse_bare_exe_name() {
        let (exe, args) = parse_command_line("OneDrive.exe /background");
        assert_eq!(exe, "OneDrive.exe");
        assert_eq!(args.as_deref(), Some("/background"));

        let (exe, args) = parse_command_line("onenote.exe");
        assert_eq!(exe, "onenote.exe");
        assert_eq!(args, None);
    }

    #[test]
    fn parse_rundll32_style_command() {
        let (exe, args) = parse_command_line(r"C:\Windows\System32\rundll32.exe C:\x\a.dll,Entry");
        assert_eq!(exe, r"C:\Windows\System32\rundll32.exe");
        assert_eq!(args.as_deref(), Some(r"C:\x\a.dll,Entry"));
    }

    #[test]
    fn parse_does_not_stop_at_exe_dot_config() {
        // `.exe.config` 不是可执行文件，不应在 `.exe` 处截断
        let (exe, args) = parse_command_line(r"C:\a\b.exe.config --x");
        assert_eq!(exe, r"C:\a\b.exe.config");
        assert_eq!(args.as_deref(), Some("--x"));

        // 多个候选时取最靠前的合法边界
        let (exe, args) = parse_command_line(r"C:\Tools\a.exe C:\x\b.exe --flag");
        assert_eq!(exe, r"C:\Tools\a.exe");
        assert_eq!(args.as_deref(), Some(r"C:\x\b.exe --flag"));
    }

    #[test]
    fn parse_empty_command() {
        assert_eq!(parse_command_line("   "), (String::new(), None));
    }

    #[test]
    fn expand_env_vars_with_lookup() {
        let vars = |name: &str| match name {
            "APPDATA" => Some(r"C:\Users\me\AppData\Roaming".to_string()),
            "MISSING" => None,
            _ => None,
        };
        assert_eq!(
            expand_with(r#""%APPDATA%\App\a.exe""#, vars),
            r#""C:\Users\me\AppData\Roaming\App\a.exe""#
        );
        // 未知变量保持原样
        assert_eq!(expand_with("100%", vars), "100%");
        assert_eq!(expand_with("%MISSING%\\x", vars), r"%MISSING%\x");
    }

    #[test]
    fn location_roundtrip_by_id_scope() {
        let loc = location_for(AutostartSource::RegistryRun, "machine32").unwrap();
        assert_eq!(loc.hive, Hive::LocalMachine32);
        assert_eq!(loc.approved_key, APPROVED_RUN32);
        // 关键回归点：Run32 的 StartupApproved 键在 64 位（native）视图下，
        // 不能用 KEY_WOW64_32KEY 去 WOW6432Node 里找（那里没有这个键）。
        assert_eq!(loc.approved_hive, Hive::LocalMachine);
        assert!(location_for(AutostartSource::RegistryRun, "nope").is_none());

        let user = location_for(AutostartSource::RegistryRun, "user").unwrap();
        assert_eq!(user.approved_hive, Hive::CurrentUser);
    }

    /// 端到端冒烟测试：在 HKCU Run 里建一个临时值，验证
    /// 「扫描 → 关闭 → 重新扫描（id 稳定、enabled=false、原始值未破坏）→ 恢复 → 清理」。
    ///
    /// 手动运行：`cargo test --lib registry_toggle_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn registry_toggle_smoke() {
        const TEST_NAME: &str = "DaemonManSmokeTest";
        const TEST_COMMAND: &str = r#""C:\Windows\System32\notepad.exe" --smoke"#;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (run_key, _) = hkcu.create_subkey(RUN).expect("打开 HKCU Run 失败");
        run_key
            .set_value(TEST_NAME, &TEST_COMMAND)
            .expect("写入临时 Run 值失败");
        // 上一次异常退出可能残留 StartupApproved，先清掉。
        let _ = startup_approved::delete_value(Hive::CurrentUser, APPROVED_RUN, TEST_NAME);

        let find = || {
            scan()
                .into_iter()
                .find(|item| item.id == format!("registry-run::user::{TEST_NAME}"))
        };

        let item = find().expect("临时 Run 值没有出现在扫描结果里");
        println!("扫描到临时项：id={} enabled={}", item.id, item.enabled);
        assert!(item.enabled, "新建的 Run 值默认应为启用");
        assert_eq!(item.executable.as_deref(), Some(r"C:\Windows\System32\notepad.exe"));

        // 关闭：只写 StartupApproved，不动 Run 值本身。
        set_enabled(&item, false).expect("关闭临时自启动项失败");
        let disabled = find().expect("关闭后条目消失（说明 id 不稳定）");
        println!("关闭后：id={} enabled={}", disabled.id, disabled.enabled);
        assert_eq!(disabled.id, item.id, "重新扫描后 id 必须保持不变");
        assert!(!disabled.enabled, "关闭后 enabled 应为 false");
        let raw: String = run_key.get_value(TEST_NAME).expect("原始 Run 值被破坏了");
        assert_eq!(raw, TEST_COMMAND, "关闭自启动项不应修改原始命令");

        // 恢复
        set_enabled(&disabled, true).expect("恢复临时自启动项失败");
        let enabled = find().expect("恢复后条目消失");
        assert!(enabled.enabled, "恢复后 enabled 应为 true");
        println!("恢复后：id={} enabled={}", enabled.id, enabled.enabled);

        // 清理
        let _ = run_key.delete_value(TEST_NAME);
        let _ = startup_approved::delete_value(Hive::CurrentUser, APPROVED_RUN, TEST_NAME);
        assert!(find().is_none(), "清理后不应再扫描到临时项");
        println!("已清理临时注册表值");
    }
}
