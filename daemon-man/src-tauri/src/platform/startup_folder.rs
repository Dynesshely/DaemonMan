//! 启动文件夹自启动项（当前用户 + 所有用户）。
//!
//! 目录：
//! - `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`
//! - `%ProgramData%\Microsoft\Windows\Start Menu\Programs\Startup`
//!
//! .lnk 用 COM `IShellLinkW` 解析出目标与参数；开关使用
//! `StartupApproved\StartupFolder`，值名是**完整的 .lnk / .exe 路径**，
//! 因此同样是「非破坏性」的：不会删除或修改启动文件夹里的文件。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{AutostartItem, AutostartSource};

use super::startup_approved::{self, Hive, APPROVED_STARTUP_FOLDER};

/// 一个启动文件夹位置。
#[derive(Debug, Clone, Copy)]
struct FolderLocation {
    scope: &'static str,
    hive: Hive,
}

const USER: FolderLocation = FolderLocation {
    scope: "user",
    hive: Hive::CurrentUser,
};
const MACHINE: FolderLocation = FolderLocation {
    scope: "machine",
    hive: Hive::LocalMachine,
};

/// 启动文件夹里会被系统执行的文件类型（其余文件如 desktop.ini 直接忽略）。
const SUPPORTED_EXTENSIONS: [&str; 5] = ["lnk", "exe", "bat", "cmd", "url"];

fn locations() -> [FolderLocation; 2] {
    [USER, MACHINE]
}

fn location_for(scope: &str) -> Option<FolderLocation> {
    locations()
        .into_iter()
        .find(|loc| loc.scope == scope)
}

fn startup_dir(loc: FolderLocation) -> Option<PathBuf> {
    let base = match loc.hive {
        Hive::CurrentUser => std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(dirs::data_dir),
        // ProgramData 没有跨平台等价物，只认环境变量。
        Hive::LocalMachine | Hive::LocalMachine32 => {
            std::env::var_os("ProgramData").map(PathBuf::from)
        }
    }?;
    Some(
        base.join(r"Microsoft\Windows\Start Menu\Programs\Startup"),
    )
}

/// 扫描两个启动文件夹。
pub fn scan() -> Vec<AutostartItem> {
    let mut items = Vec::new();
    for loc in locations() {
        let Some(dir) = startup_dir(loc) else {
            eprintln!(
                "[platform] 无法确定启动文件夹路径（scope={}），已跳过",
                loc.scope
            );
            continue;
        };
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
                // 目录不存在 = 没有自启动项，属于正常情况。
                if err.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "[platform] 读取启动文件夹 {} 失败，已跳过：{err}",
                        dir.display()
                    );
                }
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    eprintln!("[platform] 读取启动文件夹条目失败，已跳过：{err}");
                    continue;
                }
            };
            if let Some(item) = scan_entry(loc, &entry.path()) {
                items.push(item);
            }
        }
    }
    items
}

fn scan_entry(loc: FolderLocation, path: &Path) -> Option<AutostartItem> {
    let file_name = path.file_name()?.to_string_lossy().to_string();
    // desktop.ini 是文件夹外观配置，不是自启动项。
    if file_name.eq_ignore_ascii_case("desktop.ini") {
        return None;
    }
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) => {
            eprintln!("[platform] 读取 {} 的元数据失败，已跳过：{err}", path.display());
            return None;
        }
    };
    if !metadata.is_file() {
        return None;
    }

    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }

    let full_path = path.display().to_string();
    let mut details: Vec<String> = Vec::new();

    let (command, executable, args) = if extension == "lnk" {
        match super::proc::resolve_lnk(&full_path) {
            Some(link) => {
                let mut command = format!("\"{}\"", link.target);
                if let Some(args) = link.args.as_deref() {
                    command.push(' ');
                    command.push_str(args);
                }
                (command, Some(link.target), link.args)
            }
            None => {
                // 解析不了也照样列出来：至少能开关，用户能看见。
                details.push("无法解析快捷方式目标，接管时可能无法生成启动节点".to_string());
                (format!("\"{full_path}\""), None, None)
            }
        }
    } else {
        (format!("\"{full_path}\""), Some(full_path.clone()), None)
    };

    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or_else(|| file_name.clone());

    let enabled = startup_approved::read_enabled(
        loc.hive,
        APPROVED_STARTUP_FOLDER,
        &full_path,
    )
    .unwrap_or(true);

    let is_self = executable
        .as_deref()
        .is_some_and(super::proc::is_current_exe);
    if is_self {
        details.push("DaemonMan 自身".to_string());
    }
    if loc.hive.is_machine() {
        details.push("所有用户启动文件夹，开关需要管理员权限".to_string());
    }

    Some(AutostartItem {
        id: format!(
            "{}::{}::{}",
            super::source_key(AutostartSource::StartupFolder),
            loc.scope,
            file_name
        ),
        name,
        source: AutostartSource::StartupFolder,
        source_label: AutostartSource::StartupFolder.label().to_string(),
        command,
        executable,
        args,
        enabled,
        toggle_supported: true,
        requires_admin: loc.hive.is_machine(),
        is_self,
        taken_over: false,
        scope: loc.scope.to_string(),
        locator: file_name,
        detail: if details.is_empty() {
            None
        } else {
            Some(details.join("；"))
        },
    })
}

/// 通过 `StartupApproved\StartupFolder` 启用 / 禁用启动文件夹中的项。
pub fn set_enabled(item: &AutostartItem, enabled: bool) -> Result<()> {
    let loc = location_for(&item.scope).ok_or_else(|| {
        anyhow::anyhow!("未知的启动文件夹范围：{}", item.scope)
    })?;
    let dir = startup_dir(loc).context("无法确定启动文件夹路径")?;
    let full_path = dir.join(&item.locator).display().to_string();
    startup_approved::write_enabled(loc.hive, APPROVED_STARTUP_FOLDER, &full_path, enabled)
        .with_context(|| format!("切换启动文件夹自启动项「{}」失败", item.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_lookup_by_scope() {
        assert_eq!(location_for("user").unwrap().hive, Hive::CurrentUser);
        assert_eq!(location_for("machine").unwrap().hive, Hive::LocalMachine);
        assert!(location_for("machine32").is_none());
    }

    #[test]
    fn startup_dir_is_under_start_menu() {
        // 只验证拼接规则，不依赖当前机器的实际目录是否存在。
        if let Some(dir) = startup_dir(USER) {
            assert!(dir
                .to_string_lossy()
                .ends_with(r"Microsoft\Windows\Start Menu\Programs\Startup"));
        }
    }

    /// 端到端冒烟测试：拿当前用户启动文件夹里的第一个条目，关掉再恢复，
    /// 验证「文件不动、id 稳定、enabled 跟随 StartupApproved」。
    ///
    /// 手动运行：`cargo test --lib startup_folder_toggle_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn startup_folder_toggle_smoke() {
        let items = scan();
        println!("启动文件夹条目总数 = {}", items.len());
        let Some(item) = items.iter().find(|item| item.scope == "user") else {
            println!("当前用户启动文件夹没有条目，跳过");
            return;
        };
        let dir = startup_dir(USER).expect("无法确定用户启动文件夹");
        let file = dir.join(&item.locator);
        assert!(file.is_file(), "启动文件夹条目文件不存在：{}", file.display());

        println!(
            "选中条目：{} | id={} enabled={}",
            item.name, item.id, item.enabled
        );
        let original = item.enabled;

        set_enabled(item, !original).expect("切换启动文件夹条目失败");
        let toggled = scan()
            .into_iter()
            .find(|scan| scan.id == item.id)
            .expect("切换后条目消失（说明 id 不稳定）");
        assert_eq!(toggled.enabled, !original, "切换后 enabled 应取反");
        assert!(file.is_file(), "开关启动文件夹条目不应删除/移动文件");

        set_enabled(&toggled, original).expect("恢复启动文件夹条目失败");
        let restored = scan()
            .into_iter()
            .find(|scan| scan.id == item.id)
            .expect("恢复后条目消失");
        assert_eq!(restored.enabled, original, "恢复后 enabled 应与初始一致");
        println!("已恢复为 enabled={original}");
    }
}
