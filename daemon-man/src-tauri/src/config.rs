//! 配置文件读写。
//!
//! 位置：生产实例 `%USERPROFILE%\.daemonMan\`，开发实例 `%USERPROFILE%\.dev-daemonMan\`。
//! `config.json` 只存设置/接管记录；每个工作流单独存为 `workflows/<id>.json`。
//! 写入使用「临时文件 + 原子替换」，并为旧文件保留 `.bak`。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{now_iso, AppConfig, AppSettings, AutostartOverride, CONFIG_VERSION, Workflow};

/// 是否为开发实例（未启用 `custom-protocol` feature）。
///
/// 生产构建（`pnpm tauri build` / 带 `--features custom-protocol`）会内嵌前端资源，
/// 开发构建（`pnpm tauri dev` 等）需要 dev server，两者用不同数据目录共存。
pub fn is_dev() -> bool {
    !cfg!(feature = "custom-protocol")
}

/// 当前数据目录：生产 `~/.daemonMan`，开发 `~/.dev-daemonMan`。
///
/// 设置 `DAEMONMAN_DATA_DIR` 环境变量可覆盖（用于开发/自动化测试隔离真实配置）。
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DAEMONMAN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(if is_dev() { ".dev-daemonMan" } else { ".daemonMan" })
}

/// 生产数据目录（开发实例启动时用来复制一份配置）。
pub fn production_data_dir() -> PathBuf {
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

pub fn workflows_dir() -> PathBuf {
    data_dir().join("workflows")
}

/// 把工作流 id 变成安全的文件名（UUID 通常无需处理）。
fn safe_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub fn workflow_path(id: &str) -> PathBuf {
    workflows_dir().join(format!("{}.json", safe_id(id)))
}

pub fn ensure_dirs() -> Result<()> {
    seed_dev_config();
    migrate_legacy_data_dir();
    fs::create_dir_all(data_dir())?;
    fs::create_dir_all(log_dir())?;
    fs::create_dir_all(backup_dir())?;
    fs::create_dir_all(workflows_dir())?;
    Ok(())
}

/// 开发实例首次启动时，从生产目录复制一份配置（config.json + workflows/*.json）。
///
/// - 仅开发实例、未设置 `DAEMONMAN_DATA_DIR`、开发配置尚不存在时执行；
/// - 只读生产目录，不修改生产数据。
pub fn seed_dev_config() {
    if !is_dev() || std::env::var_os("DAEMONMAN_DATA_DIR").is_some() {
        return;
    }
    if config_path().exists() {
        return;
    }
    let prod = production_data_dir();
    let prod_config = prod.join("config.json");
    if !prod_config.exists() {
        return;
    }
    let _ = fs::create_dir_all(data_dir());
    let _ = fs::copy(&prod_config, config_path());
    let prod_workflows = prod.join("workflows");
    if prod_workflows.is_dir() && fs::create_dir_all(workflows_dir()).is_ok() {
        if let Ok(entries) = fs::read_dir(&prod_workflows) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                    let _ = fs::copy(&path, workflows_dir().join(entry.file_name()));
                }
            }
        }
    }
}

/// 读取 `workflows/` 下的所有工作流文件（无法解析的文件跳过并提示）。
pub fn load_workflows() -> Vec<Workflow> {
    let mut workflows = Vec::new();
    let Ok(entries) = fs::read_dir(workflows_dir()) else {
        return workflows;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        match serde_json::from_str::<Workflow>(&text) {
            Ok(mut workflow) => {
                if workflow.updated_at.is_empty() {
                    workflow.updated_at = now_iso();
                }
                workflows.push(workflow);
            }
            Err(err) => {
                eprintln!(
                    "[config] 工作流文件解析失败：{}：{err}",
                    path.display()
                );
            }
        }
    }
    workflows.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    workflows
}

pub fn save_workflow_file(workflow: &Workflow) -> Result<()> {
    fs::create_dir_all(workflows_dir())?;
    let text = serde_json::to_string_pretty(workflow).context("序列化工作流失败")?;
    write_atomic(&workflow_path(&workflow.id), text.as_bytes())
}

pub fn delete_workflow_file(id: &str) -> Result<()> {
    let path = workflow_path(id);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// 用给定的工作流集合替换磁盘上的 `workflows/` 目录（导入配置时使用）。
pub fn write_all_workflows(workflows: &[Workflow]) -> Result<()> {
    fs::create_dir_all(workflows_dir())?;
    if let Ok(entries) = fs::read_dir(workflows_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                let _ = fs::remove_file(path);
            }
        }
    }
    for workflow in workflows {
        save_workflow_file(workflow)?;
    }
    Ok(())
}

/// 旧配置里内嵌的 workflows 拆分到独立文件，并把 config.json 中的字段清空。
pub fn split_embedded_workflows(config: &mut AppConfig) -> Result<()> {
    if config.workflows.is_empty() {
        return Ok(());
    }
    let embedded = std::mem::take(&mut config.workflows);
    fs::create_dir_all(workflows_dir())?;
    for workflow in &embedded {
        save_workflow_file(workflow)?;
    }
    save(config)?;
    Ok(())
}

/// 读取配置；文件缺失或损坏时回退到默认配置（损坏文件会被备份）。
pub fn load_or_init() -> AppConfig {
    let path = config_path();
    let mut config = match fs::read_to_string(&path) {
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
    };

    // 兼容旧版本：内嵌在 config.json 里的工作流自动拆分为独立文件。
    if let Err(err) = split_embedded_workflows(&mut config) {
        eprintln!("[config] 拆分内嵌工作流失败：{err:#}");
    }
    config.workflows = load_workflows();
    config
}

/// 原子写入配置（工作流已拆分到独立文件，不写入 config.json）。
pub fn save(config: &AppConfig) -> Result<()> {
    ensure_dirs()?;
    let path = config_path();
    let mut config = config.clone();
    config.version = CONFIG_VERSION;
    config.updated_at = Some(now_iso());

    let file = ConfigFile::from_config(&config);
    let text = serde_json::to_string_pretty(&file).context("序列化配置失败")?;
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

/// config.json 的磁盘结构：不含 workflows（工作流各自独立文件）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ConfigFile {
    pub version: u32,
    pub settings: AppSettings,
    pub overrides: Vec<AutostartOverride>,
    pub updated_at: Option<String>,
}

impl ConfigFile {
    fn from_config(config: &AppConfig) -> Self {
        Self {
            version: config.version,
            settings: config.settings.clone(),
            overrides: config.overrides.clone(),
            updated_at: config.updated_at.clone(),
        }
    }
}

/// 配置导出包：设置 + 接管记录 + 所有工作流（便于整机迁移）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportBundle {
    pub version: u32,
    pub settings: AppSettings,
    pub overrides: Vec<AutostartOverride>,
    pub workflows: Vec<Workflow>,
    pub updated_at: Option<String>,
}

pub fn export_bundle(config: &AppConfig) -> ExportBundle {
    ExportBundle {
        version: config.version,
        settings: config.settings.clone(),
        overrides: config.overrides.clone(),
        workflows: config.workflows.clone(),
        updated_at: config.updated_at.clone(),
    }
}

/// 导出/导入用的配置快照（包含工作流，便于跨设备迁移）。
pub fn export_to(path: &Path, config: &AppConfig) -> Result<()> {
    let bundle = export_bundle(config);
    let text = serde_json::to_string_pretty(&bundle)?;
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

    #[test]
    fn dev_and_production_data_dirs_differ() {
        // 有环境变量覆盖时跳过（自动化测试隔离路径）。
        if std::env::var_os("DAEMONMAN_DATA_DIR").is_some() {
            return;
        }
        assert_ne!(data_dir(), production_data_dir());
        if is_dev() {
            assert!(data_dir().ends_with(".dev-daemonMan"));
        } else {
            assert!(data_dir().ends_with(".daemonMan"));
        }
    }

    #[test]
    fn config_json_does_not_serialize_workflows() {
        let mut config = AppConfig::new();
        config.workflows.push(Workflow::new("测试工作流"));
        let text = serde_json::to_string(&ConfigFile::from_config(&config)).unwrap();
        assert!(!text.contains("\"workflows\""));
        // 旧配置（内嵌 workflows）仍可反序列化，供迁移拆分。
        let legacy = format!(
            r#"{{"version":1,"workflows":[{}]}}"#,
            serde_json::to_string(&config.workflows[0]).unwrap()
        );
        let parsed: AppConfig = serde_json::from_str(&legacy).unwrap();
        assert_eq!(parsed.workflows.len(), 1);
    }
}
