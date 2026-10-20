//! `StartupApproved` 注册表开关（任务管理器使用的启用/禁用机制）。
//!
//! Windows 对「注册表 Run / 启动文件夹」这两类自启动项**没有**单独的启用标志，
//! 任务管理器的启用/禁用是通过在
//! `HKCU|HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\<类别>`
//! 下写一个 12 字节的 `REG_BINARY` 值实现的：
//!
//! | 偏移 | 含义 |
//! |------|------|
//! | 0    | 状态：`0x02` = 启用，`0x03` = 禁用（奇数一律视为禁用） |
//! | 1..4 | 保留（0） |
//! | 4..12| 写入时间 FILETIME，可为全 0 |
//!
//! 这种做法的好处是**非破坏性**：原始的 Run 值 / 启动文件夹里的 .lnk 文件保持原样，
//! 只是被系统跳过。恢复时把首字节改回 `0x02` 即可，不需要重新拼装命令行。

use anyhow::{bail, Context, Result};
use winreg::enums::*;
use winreg::{RegKey, RegValue};

/// 32 位 / 64 位 Run 项的 StartupApproved 子键（HKLM 下由 WOW64 视图区分）。
pub(crate) const APPROVED_RUN: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// 32 位视图（WOW6432Node）的 Run 项使用 `Run32`。
pub(crate) const APPROVED_RUN32: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32";
/// 启动文件夹使用 `StartupFolder`，值名是 .lnk / .exe 的**完整路径**。
pub(crate) const APPROVED_STARTUP_FOLDER: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";

/// 自启动项所在的注册表视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hive {
    /// HKEY_CURRENT_USER（当前用户）
    CurrentUser,
    /// HKEY_LOCAL_MACHINE 的 64 位视图
    LocalMachine,
    /// HKEY_LOCAL_MACHINE 的 32 位视图（WOW6432Node），需要 KEY_WOW64_32KEY
    LocalMachine32,
}

impl Hive {
    fn predef(self) -> winreg::HKEY {
        match self {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine | Hive::LocalMachine32 => HKEY_LOCAL_MACHINE,
        }
    }

    /// 32 位视图必须带 `KEY_WOW64_32KEY`，否则会读到 64 位视图同名键。
    fn view_flags(self) -> u32 {
        match self {
            Hive::LocalMachine32 => KEY_WOW64_32KEY,
            _ => 0,
        }
    }

    /// 是否属于 HKLM（写入需要管理员权限）。
    pub(crate) fn is_machine(self) -> bool {
        !matches!(self, Hive::CurrentUser)
    }

    /// 供错误信息使用的短名。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKCU",
            Hive::LocalMachine => "HKLM",
            Hive::LocalMachine32 => "HKLM\\WOW6432Node",
        }
    }

    /// 备份记录里使用的稳定短名：`HKCU` / `HKLM` / `HKLM32`。
    pub(crate) fn as_record_str(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKCU",
            Hive::LocalMachine => "HKLM",
            Hive::LocalMachine32 => "HKLM32",
        }
    }

    /// 从备份记录的字符串恢复。
    pub(crate) fn from_record_str(text: &str) -> Option<Hive> {
        match text {
            "HKCU" => Some(Hive::CurrentUser),
            "HKLM" => Some(Hive::LocalMachine),
            "HKLM32" => Some(Hive::LocalMachine32),
            _ => None,
        }
    }
}

/// `REG_*` 类型值（winreg 的枚举）转成备份记录里的 u32。
pub(crate) fn reg_type_to_u32(value_type: RegType) -> u32 {
    value_type as u32
}

/// 备份记录里的 u32 还原成 winreg 的 `RegType`。
///
/// 未知类型退回 `REG_BINARY`，保证原始字节仍然能被写回。
pub(crate) fn reg_type_from_u32(value_type: u32) -> RegType {
    match value_type {
        0 => REG_NONE,
        1 => REG_SZ,
        2 => REG_EXPAND_SZ,
        3 => REG_BINARY,
        4 => REG_DWORD,
        5 => REG_DWORD_BIG_ENDIAN,
        6 => REG_LINK,
        7 => REG_MULTI_SZ,
        8 => REG_RESOURCE_LIST,
        9 => REG_FULL_RESOURCE_DESCRIPTOR,
        10 => REG_RESOURCE_REQUIREMENTS_LIST,
        11 => REG_QWORD,
        _ => REG_BINARY,
    }
}

/// 打开一个注册表键。
pub(crate) fn open(hive: Hive, path: &str, write: bool) -> std::io::Result<RegKey> {
    let access = if write {
        KEY_READ | KEY_WRITE | hive.view_flags()
    } else {
        KEY_READ | hive.view_flags()
    };
    RegKey::predef(hive.predef()).open_subkey_with_flags(path, access)
}

/// 打开（不存在则创建）一个注册表键。
pub(crate) fn open_or_create(hive: Hive, path: &str) -> Result<RegKey> {
    let access = KEY_READ | KEY_WRITE | hive.view_flags();
    let (key, _disposition) = RegKey::predef(hive.predef())
        .create_subkey_with_flags(path, access)
        .with_context(|| format!("创建注册表键失败：{}\\{path}", hive.label()))?;
    Ok(key)
}

/// 解析形如 `HKCU\Software\...` / `HKLM\Software\...` / `HKLM32\Software\...`
/// （也接受 `HKLM\WOW6432Node\Software\...`）的路径。
#[allow(dead_code)] // 字符串形式的辅助函数，保留给 `set_enabled` / 测试使用
pub(crate) fn parse_hive_path(spec: &str) -> Result<(Hive, &str)> {
    if let Some(rest) = spec.strip_prefix(r"HKCU\") {
        return Ok((Hive::CurrentUser, rest));
    }
    if let Some(rest) = spec.strip_prefix(r"HKLM32\") {
        return Ok((Hive::LocalMachine32, rest));
    }
    if let Some(rest) = spec.strip_prefix(r"HKLM\WOW6432Node\") {
        return Ok((Hive::LocalMachine32, rest));
    }
    if let Some(rest) = spec.strip_prefix(r"HKLM\") {
        return Ok((Hive::LocalMachine, rest));
    }
    bail!("无法识别的注册表路径：{spec}（需要 HKCU\\ / HKLM\\ / HKLM32\\ 前缀）")
}

/// 构造 StartupApproved 的 12 字节二进制值。
///
/// 首字节 `0x02` 表示启用、`0x03` 表示禁用；其余字节写 0（FILETIME 全 0 是合法的，
/// 任务管理器/资源管理器只看首字节）。
pub(crate) fn approval_bytes(enabled: bool) -> [u8; 12] {
    let mut bytes = [0u8; 12];
    bytes[0] = if enabled { 0x02 } else { 0x03 };
    bytes
}

/// 解析 StartupApproved 的二进制值，返回条目是否启用。
///
/// 规则：首字节为偶数（`0x02` / `0x06`）视为启用；奇数（`0x03` / `0x07`）视为禁用。
/// 真实系统上任务管理器在禁用后再启用会写 `0x06`，所以不能只认 `0x02`。
pub(crate) fn parse_approval(bytes: &[u8]) -> Option<bool> {
    let first = *bytes.first()?;
    Some(first & 1 == 0)
}

/// 读取某个条目的启用状态；`None` 表示系统里没有对应 StartupApproved 值（默认启用）。
pub(crate) fn read_enabled(hive: Hive, key_path: &str, locator: &str) -> Option<bool> {
    let key = open(hive, key_path, false).ok()?;
    let raw = key.get_raw_value(locator).ok()?;
    parse_approval(&raw.bytes)
}

/// 写入某个条目的启用状态（不存在则创建子键与值）。
pub(crate) fn write_enabled(
    hive: Hive,
    key_path: &str,
    locator: &str,
    enabled: bool,
) -> Result<()> {
    let key = open_or_create(hive, key_path)?;
    let value = RegValue {
        bytes: approval_bytes(enabled).to_vec(),
        vtype: REG_BINARY,
    };
    key.set_raw_value(locator, &value).with_context(|| {
        format!(
            "写入 StartupApproved 失败：{}\\{key_path} → {locator}（{}）",
            hive.label(),
            if hive.is_machine() {
                "修改 HKLM 需要管理员权限"
            } else {
                "当前用户注册表写入失败"
            }
        )
    })?;
    Ok(())
}

/// 删除某个条目的 StartupApproved 值（键不存在时静默成功）。
pub(crate) fn delete_value(hive: Hive, key_path: &str, locator: &str) -> Result<()> {
    let Ok(key) = open(hive, key_path, true) else {
        return Ok(());
    };
    if let Err(err) = key.delete_value(locator) {
        // NotFound 表示本来就没有该值，属于预期情况。
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err).with_context(|| format!("删除 StartupApproved 值失败：{locator}"));
        }
    }
    Ok(())
}

/// 设置某个 `StartupApproved` 值的启用状态。
///
/// `hive_and_key` 形如 `HKCU\Software\...\StartupApproved\Run`，
/// `locator` 是值名（Run 项的值名 / 启动文件夹项的完整路径）。
///
/// `registry` / `startup_folder` 内部使用强类型的 `write_enabled`，
/// 这里保留字符串形式，便于以后的命令行工具或测试直接调用。
#[allow(dead_code)]
pub fn set_enabled(hive_and_key: &str, locator: &str, enabled: bool) -> Result<()> {
    let (hive, key_path) = parse_hive_path(hive_and_key)?;
    write_enabled(hive, key_path, locator, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_bytes_roundtrip() {
        assert_eq!(approval_bytes(true)[0], 0x02);
        assert_eq!(approval_bytes(false)[0], 0x03);
        assert_eq!(approval_bytes(true).len(), 12);
        assert_eq!(parse_approval(&approval_bytes(true)), Some(true));
        assert_eq!(parse_approval(&approval_bytes(false)), Some(false));
    }

    #[test]
    fn parse_approval_accepts_odd_and_even_first_byte() {
        // 任务管理器「禁用后再启用」会写 0x06。
        assert_eq!(parse_approval(&[0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Some(true));
        assert_eq!(parse_approval(&[0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Some(false));
        assert_eq!(parse_approval(&[]), None);
    }

    #[test]
    fn parse_hive_path_variants() {
        let (hive, path) = parse_hive_path(r"HKCU\Software\A").unwrap();
        assert_eq!(hive, Hive::CurrentUser);
        assert_eq!(path, r"Software\A");

        let (hive, path) = parse_hive_path(r"HKLM32\Software\A").unwrap();
        assert_eq!(hive, Hive::LocalMachine32);
        assert_eq!(path, r"Software\A");

        let (hive, path) = parse_hive_path(r"HKLM\WOW6432Node\Software\A").unwrap();
        assert_eq!(hive, Hive::LocalMachine32);
        assert_eq!(path, r"Software\A");

        let (hive, _) = parse_hive_path(r"HKLM\Software\A").unwrap();
        assert_eq!(hive, Hive::LocalMachine);

        assert!(parse_hive_path(r"Software\A").is_err());
    }
}
