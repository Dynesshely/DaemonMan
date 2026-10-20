//! 开始菜单可见应用扫描。
//!
//! 遍历用户（`%APPDATA%\Microsoft\Windows\Start Menu\Programs`）与系统
//! （`%ProgramData%\...\Programs`）开始菜单里的 `.lnk`，解析目标与参数，
//! 过滤隐藏文件、卸载/帮助/文档等噪声项，用于工作流「新建启动应用」时的快速选择。

use std::path::{Path, PathBuf};

use crate::model::StartMenuApp;

use super::proc;

/// 开始菜单目录嵌套深度上限（防止异常目录结构）。
const MAX_DEPTH: usize = 6;

/// 名称里出现这些词的快捷方式不是可启动应用（卸载/帮助/文档/官网等）。
const NOISE_KEYWORDS: &[&str] = &[
    "uninstall",
    "uninst",
    "卸载",
    "readme",
    "read me",
    "help",
    "帮助",
    "documentation",
    "manual",
    "手册",
    "说明",
    "website",
    "官网",
    "release note",
    "版本说明",
    "license",
    "许可",
    "repair",
    "修复",
    "command prompt",
    "命令提示符",
    "windows powershell",
    "windows terminal",
    "system information",
    "系统信息",
];

fn locations() -> [(&'static str, Option<PathBuf>); 2] {
    let user = std::env::var_os("APPDATA").map(|root| {
        PathBuf::from(root).join(r"Microsoft\Windows\Start Menu\Programs")
    });
    let machine = std::env::var_os("ProgramData").map(|root| {
        PathBuf::from(root).join(r"Microsoft\Windows\Start Menu\Programs")
    });
    [("user", user), ("machine", machine)]
}

fn is_noise(name: &str) -> bool {
    let lower = name.to_lowercase();
    NOISE_KEYWORDS.iter().any(|keyword| lower.contains(keyword))
}

/// 简单的命令行参数切分，支持双引号包裹。
fn split_args(raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in raw.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn is_hidden(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn walk(dir: &Path, location: &str, depth: usize, out: &mut Vec<StartMenuApp>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if is_hidden(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            walk(&path, location, depth + 1, out);
            continue;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_lowercase());
        if extension.as_deref() != Some("lnk") {
            continue;
        }
        let Some(name) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        if is_noise(name) {
            continue;
        }

        let Some(link) = proc::resolve_lnk(&path.to_string_lossy()) else {
            continue;
        };
        let target = link.target.trim().to_string();
        if target.is_empty() || !Path::new(&target).is_file() {
            continue;
        }
        // 目标本身是卸载器/日志之类的也过滤掉。
        let target_name = Path::new(&target)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if target_name.starts_with("unins") || target_name.contains("uninstall") {
            continue;
        }

        out.push(StartMenuApp {
            name: name.to_string(),
            path: path.display().to_string(),
            target: Some(target),
            args: link
                .args
                .as_deref()
                .map(split_args)
                .unwrap_or_default(),
            location: location.to_string(),
        });
    }
}

/// 扫描开始菜单里的可见应用（用户 + 系统，按名称排序）。
pub fn scan() -> Vec<StartMenuApp> {
    let mut apps = Vec::new();
    for (location, dir) in locations() {
        if let Some(dir) = dir {
            walk(&dir, location, 0, &mut apps);
        }
    }
    apps.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.location.cmp(&right.location))
    });
    apps.dedup_by(|left, right| left.path.eq_ignore_ascii_case(&right.path));
    apps
}
