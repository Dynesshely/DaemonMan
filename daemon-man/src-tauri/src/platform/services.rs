//! 自动启动的 Windows 服务扫描与启停。
//!
//! 用 SCM API（不是 `sc.exe` 文本输出）：
//! - `OpenSCManagerW` + `EnumServicesStatusExW` 枚举；
//! - `QueryServiceConfigW` 读启动类型，只保留 `SERVICE_AUTO_START`；
//! - 开关用 `ChangeServiceConfigW` 把启动类型改成自动 / 手动（demand）。
//!
//! 所有服务操作都需要管理员权限，因此 `requires_admin = true`。

use anyhow::{Context, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_MORE_DATA};
use windows::Win32::System::Services::*;

use crate::model::{AutostartItem, AutostartSource};

use super::registry::{expand_env_vars, parse_command_line};

/// `SC_HANDLE` 的 RAII 包装。
struct ServiceHandle(SC_HANDLE);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

/// 一个自动启动服务的原始信息。
struct ServiceRecord {
    name: String,
    display_name: String,
    binary_path: String,
    running: bool,
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Windows HRESULT 里编码的 Win32 错误码。
fn win32_code(err: &windows::core::Error) -> u32 {
    (err.code().0 as u32) & 0xFFFF
}

fn open_scm() -> Result<ServiceHandle> {
    let handle = unsafe {
        OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        )
    }
    .context("打开服务控制管理器失败")?;
    Ok(ServiceHandle(handle))
}

/// 扫描所有 `SERVICE_AUTO_START` 的服务。
pub fn scan() -> Vec<AutostartItem> {
    let records = match enum_auto_start_services() {
        Ok(records) => records,
        Err(err) => {
            eprintln!("[platform] 枚举系统服务失败，已跳过服务类自启动项：{err:#}");
            return Vec::new();
        }
    };
    records.into_iter().map(record_to_item).collect()
}

fn enum_auto_start_services() -> Result<Vec<ServiceRecord>> {
    let scm = open_scm()?;

    // 先给一个足够大的缓冲区；返回 ERROR_MORE_DATA 时按需要的字节数扩容重试。
    let mut buffer: Vec<u8> = vec![0u8; 256 * 1024];
    let mut bytes_needed: u32 = 0;
    let mut services_returned: u32 = 0;
    loop {
        let result = unsafe {
            EnumServicesStatusExW(
                scm.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                Some(&mut buffer),
                &mut bytes_needed,
                &mut services_returned,
                None,
                PCWSTR::null(),
            )
        };
        match result {
            Ok(()) => break,
            Err(err) if win32_code(&err) == ERROR_MORE_DATA.0 => {
                let grow = (buffer.len() as u32).max(bytes_needed);
                if grow as usize <= buffer.len() {
                    // 理论上不会发生；避免死循环。
                    return Err(err).context("枚举服务失败：缓冲区尺寸异常");
                }
                buffer.resize(grow as usize, 0);
                continue;
            }
            Err(err) => {
                return Err(err).context("调用 EnumServicesStatusExW 失败");
            }
        }
    }

    let mut records = Vec::new();
    let entries = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
            services_returned as usize,
        )
    };

    for entry in entries {
        let status = entry.ServiceStatusProcess;
        // 只关心「自动启动」的服务：手动 / 禁用的服务不参与登录自启动。
        let (name, display_name) = unsafe {
            (
                entry.lpServiceName.to_string().unwrap_or_default(),
                entry.lpDisplayName.to_string().unwrap_or_default(),
            )
        };
        if name.is_empty() {
            continue;
        }

        let service = match unsafe {
            OpenServiceW(
                scm.0,
                PCWSTR(to_wide(&name).as_ptr()),
                SERVICE_QUERY_CONFIG,
            )
        } {
            Ok(service) => ServiceHandle(service),
            Err(err) => {
                // 受保护的服务（如 PPL 进程）会拒绝查询配置，跳过即可。
                eprintln!(
                    "[platform] 跳过服务「{name}」：无法打开服务配置（Win32 错误 {}）",
                    win32_code(&err)
                );
                continue;
            }
        };

        let mut needed: u32 = 0;
        // 第一次调用只取所需缓冲区大小（返回 ERROR_INSUFFICIENT_BUFFER 是预期的）。
        unsafe {
            let _ = QueryServiceConfigW(service.0, None, 0, &mut needed);
        }
        let size = needed.max(std::mem::size_of::<QUERY_SERVICE_CONFIGW>() as u32 + 2048);
        let mut config_buffer: Vec<u8> = vec![0u8; size as usize];
        if let Err(err) = unsafe {
            QueryServiceConfigW(
                service.0,
                Some(config_buffer.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW),
                config_buffer.len() as u32,
                &mut needed,
            )
        } {
            eprintln!(
                "[platform] 跳过服务「{name}」：读取服务配置失败（Win32 错误 {}）",
                win32_code(&err)
            );
            continue;
        }
        let config = unsafe {
            std::ptr::read_unaligned(config_buffer.as_ptr() as *const QUERY_SERVICE_CONFIGW)
        };
        if config.dwStartType != SERVICE_AUTO_START {
            continue;
        }

        let binary_path = unsafe { config.lpBinaryPathName.to_string().unwrap_or_default() };
        records.push(ServiceRecord {
            name,
            display_name,
            binary_path,
            running: status.dwCurrentState == SERVICE_RUNNING,
        });
    }

    Ok(records)
}

fn record_to_item(record: ServiceRecord) -> AutostartItem {
    let command = expand_env_vars(record.binary_path.trim());
    let (exe, args) = parse_command_line(&command);
    let executable = if exe.is_empty() {
        None
    } else {
        super::registry::resolve_executable(&exe)
    };
    let name = if record.display_name.trim().is_empty() {
        record.name.clone()
    } else {
        record.display_name.clone()
    };

    let mut details = vec![
        format!("服务名：{}", record.name),
        if record.running {
            "自动启动（当前运行中）".to_string()
        } else {
            "自动启动（当前未运行）".to_string()
        },
    ];
    if record.binary_path.trim().is_empty() {
        details.push("无法读取服务二进制路径".to_string());
    }

    AutostartItem {
        id: format!(
            "{}::machine::{}",
            super::source_key(AutostartSource::Service),
            record.name
        ),
        name,
        source: AutostartSource::Service,
        source_label: AutostartSource::Service.label().to_string(),
        command,
        executable,
        args,
        enabled: true,
        toggle_supported: true,
        // 服务不提供删除（属于系统基础设施，只允许改启动类型）。
        can_delete: false,
        // 改服务启动类型必须提权。
        requires_admin: true,
        is_self: false,
        taken_over: false,
        scope: "machine".to_string(),
        locator: record.name,
        detail: Some(details.join("；")),
    }
}

/// 修改服务启动类型（auto / demand）。
pub fn set_enabled(item: &AutostartItem, enabled: bool) -> Result<()> {
    let scm = open_scm()?;
    let wide_name = to_wide(&item.locator);
    let service = match unsafe {
        OpenServiceW(
            scm.0,
            PCWSTR(wide_name.as_ptr()),
            SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG,
        )
    } {
        Ok(service) => ServiceHandle(service),
        Err(err) => {
            if win32_code(&err) == ERROR_ACCESS_DENIED.0 {
                anyhow::bail!(
                    "修改服务「{}」的启动类型需要管理员权限（以管理员身份运行 DaemonMan 后重试）",
                    item.name
                );
            }
            return Err(err).with_context(|| format!("打开服务「{}」失败", item.name));
        }
    };

    let start_type = if enabled {
        SERVICE_AUTO_START
    } else {
        SERVICE_DEMAND_START
    };

    // 只改启动类型，其它参数全部 SERVICE_NO_CHANGE，避免破坏服务配置。
    unsafe {
        ChangeServiceConfigW(
            service.0,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            start_type,
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
    }
    .map_err(|err| {
        if win32_code(&err) == ERROR_ACCESS_DENIED.0 {
            anyhow::anyhow!(
                "把服务「{}」改为{}启动需要管理员权限（以管理员身份运行 DaemonMan 后重试）",
                item.name,
                if enabled { "自动" } else { "手动" }
            )
        } else {
            anyhow::anyhow!(
                "修改服务「{}」启动类型失败：{err}",
                item.name
            )
        }
    })
}
