//! COM 初始化 / 创建的薄封装。
//!
//! 计划任务（ITaskService）与快捷方式解析（IShellLinkW）都要求调用线程先初始化 COM 套间，
//! 并且必须在**同一线程、同一作用域**内完成 `CoUninitialize`。这里用 RAII guard 保证：
//! COM 对象全部析构后 guard 才 drop，随后再 `CoUninitialize`。

use anyhow::{bail, Result};
use windows::core::{GUID, Interface};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};

/// 当前线程已经以另一种套间模式（多为 MTA）初始化过 COM。
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;

/// COM 套间 guard。
///
/// - `CoInitializeEx` 成功（含 S_FALSE：本线程已初始化，引用计数 +1）时必须成对 `CoUninitialize`；
/// - 若返回 `RPC_E_CHANGED_MODE`，线程已由别处以 MTA 初始化，仍可直接使用 COM，
///   但**不能**再调用 `CoUninitialize`（否则会破坏调用方的 COM 生命周期）。
pub(crate) struct Apartment {
    uninitialize: bool,
}

/// 在当前线程初始化 STA 套间并返回 guard。
pub(crate) fn init_apartment() -> Result<Apartment> {
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if hr.is_ok() {
        Ok(Apartment { uninitialize: true })
    } else if hr.0 == RPC_E_CHANGED_MODE {
        Ok(Apartment { uninitialize: false })
    } else {
        bail!("初始化 COM 失败（HRESULT 0x{:08X}）", hr.0 as u32)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

/// `CoCreateInstance` 的简写：从 CLSID 创建接口 `T`。
pub(crate) fn create_instance<T: Interface>(clsid: &GUID) -> Result<T> {
    let instance = unsafe { CoCreateInstance(clsid, None, CLSCTX_ALL) }?;
    Ok(instance)
}
