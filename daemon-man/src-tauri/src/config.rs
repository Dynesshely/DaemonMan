//! 配置文件读写。
//!
//! 位置：`%USERPROFILE%\.daemonMan\config.json`。
//! 写入使用「临时文件 + 原子替换」，并为旧文件保留 `.bak`。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{now_iso, AppConfig, CONFIG_VERSION};

/// 当前数据目录：`~/.daemonMan`。
///
/// 设置 `DAEMONMAN_DATA_DIR` 环境变量可覆盖（用于开发/自动化测试隔离真实配置）。
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DAEMONMAN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".daemonMan")
}

/// 旧版本使用过的数据目录：`~/.daemon-man`（仅用于首次启动时自动迁移）。
pub fn legacy_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".daemon-man")
}

/// 若新目录不存在而旧目录存在，则把旧数据整体迁移过来（配置 + 日志）。
pub fn migrate_legacy_data_dir() {
    // 显式指定数据目录时不自动迁移，避免测试污染真实配置。
    if std::env::var_os("DAEMONMAN_DATA_DIR").is_some() {
        return;
    }
    let new_dir = data_dir();
    let old_dir = legacy_data_dir();
    if new_dir.exists() || !old_dir.exists() {
        return;
    }

    if fs::rename(&old_dir, &new_dir).is_ok() {
        return;
    }

    // 跨卷或被占用时退化为逐项复制，避免用户配置丢失。
    let _ = fs::create_dir_all(&new_dir);
    let _ = fs::copy(old_dir.join("config.json"), new_dir.join("config.json"));
    let old_logs = old_dir.join("logs");
    if old_logs.is_dir() {
        let new_logs = new_dir.join("logs");
        let _ = fs::create_dir_all(&new_logs);
        if let Ok(entries) = fs::read_dir(&old_logs) {
            for entry in entries.flatten() {
                let _ = fs::copy(entry.path(), new_logs.join(entry.file_name()));
            }
        }
    }
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn log_path() -> PathBuf {
    log_dir().join("daemon-man.log")
}

pub fn backup_dir() -> PathBuf {
    data_dir().join("autostart-backup")
}

pub fn ensure_dirs() -> Result<()> {
    migrate_legacy_data_dir();
    fs::create_dir_all(data_dir())?;
    fs::create_dir_all(log_dir())?;
    fs::create_dir_all(backup_dir())?;
    Ok(())
}

/// 读取配置；文件缺失或损坏时回退到默认配置（损坏文件会被备份）。
pub fn load_or_init() -> AppConfig {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<AppConfig>(&text) {
            Ok(mut config) => {
                if config.version == 0 {
                    config.version = CONFIG_VERSION;
                }
                config
            }
            Err(err) => {
                let backup = path.with_extension(format!(
                    "broken-{}.json",
                    chrono::Local::now().format("%Y%m%d%H%M%S")
                ));
                let _ = fs::copy(&path, &backup);
                eprintln!(
                    "[config] 解析配置失败：{err}；已备份到 {}，使用默认配置",
                    backup.display()
                );
                AppConfig::new()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => AppConfig::new(),
        Err(err) => {
            eprintln!("[config] 读取配置失败：{err}；使用默认配置");
            AppConfig::new()
        }
    }
}

/// 原子写入配置。
pub fn save(config: &AppConfig) -> Result<()> {
    ensure_dirs()?;
    let path = config_path();
    let mut config = config.clone();
    config.version = CONFIG_VERSION;
    config.updated_at = Some(now_iso());

    let text = serde_json::to_string_pretty(&config).context("序列化配置失败")?;
    write_atomic(&path, text.as_bytes())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let bak = path.with_extension("json.bak");
        let _ = fs::copy(path, bak);
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    // Windows 上 rename 到已存在的目标会失败，先移除目标。
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// 导出/导入用的配置快照。
pub fn export_to(path: &Path, config: &AppConfig) -> Result<()> {
    let text = serde_json::to_string_pretty(config)?;
    write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default_config() {
        let config = AppConfig::new();
        let text = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.version, CONFIG_VERSION);
        assert_eq!(parsed.settings.max_concurrency, 8);
    }

    #[test]
    fn tolerant_of_missing_fields() {
        let parsed: AppConfig = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(parsed.workflows.is_empty());
        assert_eq!(parsed.settings.language, "zh-CN");
    }
}
