//! 文件图标提取：把 exe / lnk / bat 等的 shell 图标转成 PNG data URL。
//!
//! 流程：
//! 1. `SHCreateItemFromParsingName` → `IShellItemImageFactory::GetImage` 取 HBITMAP
//!    （.lnk / .exe / .bat / URL 都适用，尺寸由参数决定）；
//! 2. HBITMAP → 32bpp DIB → BGRA → RGBA（整张图 alpha 全 0 时按不透明处理）；
//! 3. 失败时回退到老 API `SHGetFileInfoW(SHGFI_ICON | SHGFI_LARGEICON)`；
//! 4. `image` 编码 PNG，`base64` 生成 `data:image/png;base64,...`，进程内缓存。
//!
//! 所有失败路径都返回 `None`，绝不 panic：图标只是 UI 装饰，不能影响自启动列表。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC,
    BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON,
    SHGFI_LARGEICON, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

/// 缓存上限。超过后整表清空（图标很小，重建成本远低于做 LRU）。
const MAX_CACHE_ENTRIES: usize = 512;

type IconCache = Mutex<HashMap<(String, u32), Option<String>>>;

fn cache() -> &'static IconCache {
    static CACHE: OnceLock<IconCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 清空图标缓存（例如文件被替换、主题切换后）。
///
/// 命令层暂时没有调用点，但这是平台层对外承诺的能力（`platform::icon::invalidate_icon_cache`），
/// 留给后续「刷新图标」命令使用。
#[allow(dead_code)]
pub fn invalidate_icon_cache() {
    if let Ok(mut cache) = cache().lock() {
        cache.clear();
    }
}

/// 提取文件图标，返回 `data:image/png;base64,...`；无法提取时返回 None。
pub fn file_icon_data_url(path: &str, size: u32) -> Option<String> {
    let path = path.trim().trim_matches('"');
    if path.is_empty() {
        return None;
    }
    let size = size.clamp(16, 256);
    let key = (path.to_ascii_lowercase(), size);

    if let Ok(cache) = cache().lock() {
        if let Some(cached) = cache.get(&key) {
            return cached.clone();
        }
    }

    // 文件不存在时直接返回 None，避免 shell 为不存在的路径生成「通用图标」。
    let result = if std::path::Path::new(path).exists() {
        shell_item_icon(path, size).or_else(|| shell_file_info_icon(path))
    } else {
        None
    };

    if let Ok(mut cache) = cache().lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, result.clone());
    }
    result
}

/// 首选方式：`IShellItemImageFactory::GetImage`。
fn shell_item_icon(path: &str, size: u32) -> Option<String> {
    let _apartment = super::com::init_apartment().ok()?;
    let wide = to_wide(path);
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
    let bitmap = unsafe {
        factory.GetImage(
            SIZE {
                cx: size as i32,
                cy: size as i32,
            },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        )
    }
    .ok()?;
    if bitmap.is_invalid() {
        return None;
    }
    // HBITMAP 由调用方（我们）负责释放。
    let rgba = bitmap_to_rgba(bitmap);
    unsafe {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    let (width, height, pixels) = rgba?;
    png_data_url(width, height, pixels)
}

/// 回退方式：`SHGetFileInfoW` 拿 HICON。
fn shell_file_info_icon(path: &str) -> Option<String> {
    let wide = to_wide(path);
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let rgba = icon_to_rgba(info.hIcon);
    unsafe {
        let _ = DestroyIcon(info.hIcon);
    }
    let (width, height, pixels) = rgba?;
    png_data_url(width, height, pixels)
}

/// HICON → RGBA。HICON 内部维护 hbmColor / hbmMask，需要我们自己释放。
fn icon_to_rgba(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut info = ICONINFO::default();
    if unsafe { GetIconInfo(icon, &mut info) }.is_err() {
        return None;
    }
    let color = info.hbmColor;
    let mask = info.hbmMask;
    let result = if color.is_invalid() {
        None
    } else {
        bitmap_to_rgba(color)
    };
    unsafe {
        if !color.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(color.0));
        }
        if !mask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(mask.0));
        }
    }
    result
}

/// HBITMAP → (宽, 高, RGBA 像素)。使用 32bpp 顶向下的 DIB。
fn bitmap_to_rgba(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(bitmap.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut core::ffi::c_void),
        );
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            return None;
        }
        let width = bm.bmWidth as u32;
        let height = bm.bmHeight as u32;

        let screen_dc: HDC = GetDC(HWND(std::ptr::null_mut()));
        if screen_dc.is_invalid() {
            return None;
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_invalid() {
            let _ = ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);
            return None;
        }

        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // 负高度 = 从上到下存储，省一次翻转
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let lines = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr() as *mut core::ffi::c_void),
            &mut info,
            DIB_RGB_COLORS,
        );

        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);

        if lines == 0 {
            return None;
        }
        let rgba = rgba_from_bgra(&pixels, width, height)?;
        Some((width, height, rgba))
    }
}

/// BGRA（Windows DIB 顺序）→ RGBA；整张图 alpha 全 0 时统一补成 255。
pub(crate) fn rgba_from_bgra(bgra: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let pixel_count = width as usize * height as usize;
    if pixel_count == 0 || bgra.len() < pixel_count * 4 {
        return None;
    }
    let source = &bgra[..pixel_count * 4];
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    let mut has_alpha = false;
    let (pixels, _) = source.as_chunks::<4>();
    for pixel in pixels {
        // DIB 内存布局是 B,G,R,A
        let alpha = pixel[3];
        if alpha != 0 {
            has_alpha = true;
        }
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], alpha]);
    }
    if !has_alpha {
        let (pixels, _) = rgba.as_chunks_mut::<4>();
        for pixel in pixels {
            pixel[3] = 255;
        }
    }
    Some(rgba)
}

/// RGBA → PNG → base64 data URL。
fn png_data_url(width: u32, height: u32, rgba: Vec<u8>) -> Option<String> {
    use base64::Engine as _;
    let image = image::RgbaImage::from_raw(width, height, rgba)?;
    let mut buffer = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut buffer, image::ImageFormat::Png)
        .ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(buffer.into_inner());
    Some(format!("data:image/png;base64,{encoded}"))
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_to_rgba_swaps_channels() {
        // 一个红色像素：B=0, G=0, R=255, A=255
        let bgra = [0u8, 0, 255, 255, 0, 255, 0, 128];
        let rgba = rgba_from_bgra(&bgra, 2, 1).unwrap();
        assert_eq!(rgba, vec![255, 0, 0, 255, 0, 255, 0, 128]);
    }

    #[test]
    fn bgra_to_rgba_alpha_zero_becomes_opaque() {
        let bgra = [10u8, 20, 30, 0, 40, 50, 60, 0];
        let rgba = rgba_from_bgra(&bgra, 2, 1).unwrap();
        assert_eq!(rgba, vec![30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn bgra_to_rgba_rejects_short_buffer() {
        assert!(rgba_from_bgra(&[0u8; 4], 2, 1).is_none());
        assert!(rgba_from_bgra(&[], 0, 0).is_none());
    }

    #[test]
    fn missing_file_returns_none() {
        assert!(file_icon_data_url(r"C:\definitely\not\exists\app.exe", 32).is_none());
        assert!(file_icon_data_url("", 32).is_none());
    }

    /// 冒烟测试：真实提取本机图标。
    ///
    /// 手动运行：`cargo test --lib icon_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn icon_smoke() {
        let mut samples = vec![
            r"C:\Windows\explorer.exe".to_string(),
            r"C:\Windows\System32\notepad.exe".to_string(),
            r"C:\Windows\System32\mstsc.exe".to_string(),
        ];
        // 真实 .lnk：从当前用户启动文件夹里挑一个（验证 IShellItemImageFactory 走快捷方式）。
        if let Some(startup) = dirs::data_dir().map(|dir| {
            dir.join(r"Microsoft\Windows\Start Menu\Programs\Startup")
        }) {
            if let Ok(entries) = std::fs::read_dir(&startup) {
                if let Some(lnk) = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .find(|path| {
                        path.extension()
                            .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("lnk"))
                    })
                {
                    samples.push(lnk.display().to_string());
                }
            }
        }

        for path in samples {
            let icon = file_icon_data_url(&path, 32);
            match icon.as_deref() {
                Some(data_url) => {
                    println!("{path} → 长度 {} 字节", data_url.len());
                    assert!(data_url.starts_with("data:image/png;base64,"));
                    assert!(data_url.len() > 300, "图标 data URL 过短：{data_url}");
                }
                None => println!("{path} → 无法提取图标（跳过）"),
            }
        }
        // 缓存命中：第二次调用应返回同样结果
        let first = file_icon_data_url(r"C:\Windows\explorer.exe", 32);
        let second = file_icon_data_url(r"C:\Windows\explorer.exe", 32);
        assert_eq!(first, second);
        invalidate_icon_cache();
        assert!(file_icon_data_url(r"C:\Windows\explorer.exe", 32).is_some());
        println!("缓存与 invalidate_icon_cache 正常");
    }

    /// 冒烟测试：图标尺寸参数生效（48px 的 PNG 应比 16px 大）。
    #[test]
    #[ignore]
    fn icon_sizes_smoke() {
        let small = file_icon_data_url(r"C:\Windows\explorer.exe", 16);
        let large = file_icon_data_url(r"C:\Windows\explorer.exe", 48);
        println!(
            "16px = {:?} 字节，48px = {:?} 字节",
            small.as_ref().map(|s| s.len()),
            large.as_ref().map(|s| s.len())
        );
        if let (Some(small), Some(large)) = (small, large) {
            assert!(small.len() > 300);
            assert!(large.len() > small.len());
        }
    }
}
