//! 自启动项的禁用 / 恢复 / 删除（含备份）。
//!
//! 设计：
//! - `disable`：支持系统开关的项直接禁用（`native`）；不支持的项（例如 RunOnce，
//!   Windows 没有它的 StartupApproved 机制）先把原始数据备份到
//!   `~/.daemonMan/autostart-backup/`，再从原位置移除（`removed`）。
//! - `restore`：`native` 项重新启用系统开关；`removed` / `deleted` 项按备份还原
//!   注册表值、文件或任务 XML。
//! - `delete`：先备份（`deleted`）再从原位置永久删除；系统服务拒绝删除。
//!
//! 备份索引 `index.json` 保存 `Vec<BackupRecord>`：注册表项存原始值类型与字节，
//! 文件项存副本路径，计划任务存导出的 XML 路径，全部可完整还原。

use std::path::{Path, PathBuf};

use base64::Engine as _;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{now_iso, AutostartItem, AutostartSource};

use super::startup_approved::{self, Hive};

/// 索引文件名（位于 `crate::config::backup_dir()`）。
const INDEX_FILE: &str = "index.json";

/// 进程内串行化索引写入，避免并发命令互相覆盖 `index.json`。
static INDEX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---------------------------------------------------------------------------
// 备份记录
// ---------------------------------------------------------------------------

/// 一个注册表自启动项的原始值。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RegistryBackup {
    /// `HKCU` / `HKLM` / `HKLM32`
    pub hive: String,
    pub key_path: String,
    pub value_name: String,
    /// `REG_*` 数值
    pub value_type: u32,
    /// 便于人看的文本（字符串类型是命令，数字类型是十进制数值）
    pub value: String,
    /// 原始字节（base64），还原时优先用它，保证无损
    pub value_bytes_base64: Option<String>,
}

/// 一个启动文件夹条目的原文件与备份副本。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileBackup {
    pub original_path: String,
    pub backup_path: String,
}

/// 一个计划任务的完整路径与导出的 XML。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskBackup {
    pub task_path: String,
    pub xml_path: String,
}

/// 一条备份记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupRecord {
    pub id: String,
    pub name: String,
    pub source: AutostartSource,
    pub locator: String,
    pub scope: String,
    pub command: String,
    pub executable: Option<String>,
    pub args: Option<String>,
    pub created_at: String,
    /// `native`（系统开关） / `removed`（备份后移除） / `deleted`（备份后永久删除）
    pub mode: String,
    pub enabled_before: bool,
    pub requires_admin: bool,
    pub registry: Option<RegistryBackup>,
    pub file: Option<FileBackup>,
    pub task: Option<TaskBackup>,
}

impl Default for BackupRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            source: AutostartSource::RegistryRun,
            locator: String::new(),
            scope: String::new(),
            command: String::new(),
            executable: None,
            args: None,
            created_at: now_iso(),
            mode: "native".to_string(),
            enabled_before: true,
            requires_admin: false,
            registry: None,
            file: None,
            task: None,
        }
    }
}

impl BackupRecord {
    fn from_item(item: &AutostartItem, mode: &str) -> Self {
        Self {
            id: item.id.clone(),
            name: item.name.clone(),
            source: item.source,
            locator: item.locator.clone(),
            scope: item.scope.clone(),
            command: item.command.clone(),
            executable: item.executable.clone(),
            args: item.args.clone(),
            created_at: now_iso(),
            mode: mode.to_string(),
            enabled_before: item.enabled,
            requires_admin: item.requires_admin,
            registry: None,
            file: None,
            task: None,
        }
    }

    fn to_item(&self, enabled: bool) -> AutostartItem {
        AutostartItem {
            id: self.id.clone(),
            name: self.name.clone(),
            source: self.source,
            source_label: self.source.label().to_string(),
            command: self.command.clone(),
            executable: self.executable.clone(),
            args: self.args.clone(),
            enabled,
            toggle_supported: self.source != AutostartSource::RegistryRunOnce,
            can_delete: self.source != AutostartSource::Service,
            requires_admin: self.requires_admin,
            is_self: false,
            taken_over: false,
            scope: self.scope.clone(),
            locator: self.locator.clone(),
            detail: Some(format!("从备份还原（{}）", self.created_at)),
        }
    }
}

// ---------------------------------------------------------------------------
// 索引读写
// ---------------------------------------------------------------------------

fn backup_dir() -> PathBuf {
    crate::config::backup_dir()
}

fn index_path() -> PathBuf {
    backup_dir().join(INDEX_FILE)
}

/// 读取索引；文件不存在或损坏时按空索引处理（损坏只警告，不阻断功能）。
fn load_index() -> Vec<BackupRecord> {
    match std::fs::read_to_string(index_path()) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(records) => records,
            Err(err) => {
                eprintln!("[platform] 自启动备份索引格式损坏，按空索引处理：{err}");
                Vec::new()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => {
            eprintln!("[platform] 读取自启动备份索引失败，按空索引处理：{err}");
            Vec::new()
        }
    }
}

fn save_index(records: &[BackupRecord]) -> Result<()> {
    let text = serde_json::to_string_pretty(records).context("序列化自启动备份索引失败")?;
    crate::config::write_atomic(&index_path(), text.as_bytes())
        .context("写入自启动备份索引失败")
}

/// 在锁内读改写索引。
fn with_index_mut<T>(f: impl FnOnce(&mut Vec<BackupRecord>) -> T) -> Result<T> {
    let _guard = INDEX_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut records = load_index();
    let result = f(&mut records);
    save_index(&records)?;
    Ok(result)
}

fn upsert_record(record: BackupRecord) -> Result<()> {
    with_index_mut(|records| {
        records.retain(|existing| existing.id != record.id);
        records.push(record);
    })
}

fn find_record(item_id: &str) -> Option<BackupRecord> {
    load_index().into_iter().find(|record| record.id == item_id)
}

fn remove_record(item_id: &str) -> Result<()> {
    with_index_mut(|records| {
        records.retain(|record| record.id != item_id);
    })
}

// ---------------------------------------------------------------------------
// 对外接口
// ---------------------------------------------------------------------------

/// 禁用自启动项（native 或备份后移除）。
pub fn disable(item: &AutostartItem) -> Result<()> {
    if item.is_self {
        bail!("不能禁用 DaemonMan 自身");
    }
    if item.toggle_supported {
        native_set_enabled(item, false)
            .with_context(|| format!("禁用自启动项「{}」失败", item.name))?;
        let record = BackupRecord {
            enabled_before: item.enabled,
            ..BackupRecord::from_item(item, "native")
        };
        upsert_record(record)?;
        Ok(())
    } else {
        backup_and_remove(item, "removed")
    }
}

/// 恢复被禁用的自启动项。
pub fn restore(item_id: &str) -> Result<AutostartItem> {
    let record = find_record(item_id);

    match record.as_ref().map(|record| record.mode.as_str()) {
        Some("native") => {
            let item = scan_item(item_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "「{item_id}」已被禁用，但系统里找不到该项（可能被其他程序删除），无法通过系统开关恢复"
                )
            })?;
            native_set_enabled(&item, true)
                .with_context(|| format!("恢复自启动项「{}」失败", item.name))?;
        }
        Some("removed") | Some("deleted") => {
            let record = record.as_ref().expect("record exists");
            restore_backed_up(record)?;
        }
        Some(other) => bail!("未知的备份模式：{other}"),
        None => {
            // 没有备份记录：兼容用户通过系统开关（任务管理器等）关掉的项。
            let item = scan_item(item_id)
                .ok_or_else(|| anyhow::anyhow!("未找到自启动项：{item_id}（也没有对应的备份记录）"))?;
            if !item.toggle_supported {
                bail!(
                    "自启动项「{}」没有备份记录，且 Windows 没有它的系统开关，无法恢复",
                    item.name
                );
            }
            native_set_enabled(&item, true)
                .with_context(|| format!("恢复自启动项「{}」失败", item.name))?;
        }
    }

    if record.is_some() {
        // 还原成功后备份记录已无用（文件副本 / XML 已消费或已回到原位）。
        remove_record(item_id)?;
    }

    if let Some(mut item) = scan_item(item_id) {
        item.enabled = true;
        return Ok(item);
    }
    if let Some(record) = record {
        return Ok(record.to_item(true));
    }
    bail!("恢复成功，但重新扫描找不到该项：{item_id}")
}

/// 删除自启动项（先备份再删除）。
pub fn delete(item: &AutostartItem) -> Result<()> {
    if item.is_self {
        bail!("不能删除 DaemonMan 自身");
    }
    if !item.can_delete {
        bail!("该类型的自启动项不支持删除（系统服务请使用禁用）");
    }
    backup_and_remove(item, "deleted")
}

// ---------------------------------------------------------------------------
// 实现
// ---------------------------------------------------------------------------

fn native_set_enabled(item: &AutostartItem, enabled: bool) -> Result<()> {
    match item.source {
        AutostartSource::RegistryRun | AutostartSource::RegistryRunOnce => {
            super::registry::set_enabled(item, enabled)
        }
        AutostartSource::StartupFolder => super::startup_folder::set_enabled(item, enabled),
        AutostartSource::ScheduledTask => super::tasks::set_enabled(item, enabled),
        AutostartSource::Service => super::services::set_enabled(item, enabled),
    }
}

fn scan_item(item_id: &str) -> Option<AutostartItem> {
    super::scan_autostart()
        .into_iter()
        .find(|item| item.id == item_id)
}

/// 备份后从原位置移除（`mode` = `removed` / `deleted`）。
fn backup_and_remove(item: &AutostartItem, mode: &str) -> Result<()> {
    match item.source {
        AutostartSource::RegistryRun | AutostartSource::RegistryRunOnce => {
            backup_registry_and_remove(item, mode)
        }
        AutostartSource::StartupFolder => backup_file_and_remove(item, mode),
        AutostartSource::ScheduledTask => backup_task_and_remove(item, mode),
        AutostartSource::Service => bail!("系统服务不支持移除（请使用禁用）"),
    }
}

fn backup_registry_and_remove(item: &AutostartItem, mode: &str) -> Result<()> {
    let raw = super::registry::read_raw_value(item.source, &item.scope, &item.locator)
        .with_context(|| format!("读取注册表原始值失败：{}", item.name))?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "在注册表中找不到「{}」的原始值（可能已被其他程序修改），已中止，未做任何修改",
                item.name
            )
        })?;

    let mut record = BackupRecord::from_item(item, mode);
    record.registry = Some(RegistryBackup {
        hive: raw.hive.as_record_str().to_string(),
        key_path: raw.key_path.to_string(),
        value_name: raw.value_name.clone(),
        value_type: raw.value_type,
        value: decode_registry_text(raw.value_type, &raw.bytes),
        value_bytes_base64: Some(base64::engine::general_purpose::STANDARD.encode(&raw.bytes)),
    });

    // 先落盘备份，再删除；删除失败则回滚记录并保留原值。
    upsert_record(record)?;
    if let Err(err) = super::registry::delete_value(item.source, &item.scope, &item.locator) {
        let _ = remove_record(&item.id);
        return Err(err).with_context(|| {
            format!("删除注册表值失败，已保留原值并回滚备份记录：{}", item.name)
        });
    }
    Ok(())
}

fn backup_file_and_remove(item: &AutostartItem, mode: &str) -> Result<()> {
    let original = super::startup_folder::item_path(&item.scope, &item.locator)
        .with_context(|| format!("定位启动文件夹条目失败：{}", item.name))?;
    if !original.is_file() {
        bail!(
            "启动文件夹条目文件不存在（可能已被删除）：{}",
            original.display()
        );
    }

    let backup_path = unique_backup_path(&item.locator);
    std::fs::copy(&original, &backup_path).with_context(|| {
        format!(
            "备份启动文件夹条目失败：{} → {}",
            original.display(),
            backup_path.display()
        )
    })?;

    let mut record = BackupRecord::from_item(item, mode);
    record.file = Some(FileBackup {
        original_path: original.display().to_string(),
        backup_path: backup_path.display().to_string(),
    });
    upsert_record(record)?;

    if let Err(err) = std::fs::remove_file(&original) {
        let _ = remove_record(&item.id);
        let _ = std::fs::remove_file(&backup_path);
        return Err(err).with_context(|| {
            format!("删除启动文件夹条目失败，已保留原文件：{}", original.display())
        });
    }

    // 顺手清掉 StartupApproved 里的旧开关值（失败可忽略）。
    if let Some(hive) = folder_hive(&item.scope) {
        let full_path = original.display().to_string();
        if let Err(err) = startup_approved::delete_value(
            hive,
            startup_approved::APPROVED_STARTUP_FOLDER,
            &full_path,
        ) {
            eprintln!("[platform] 清理启动文件夹 StartupApproved 值失败（可忽略）：{err:#}");
        }
    }
    Ok(())
}

fn backup_task_and_remove(item: &AutostartItem, mode: &str) -> Result<()> {
    let xml = super::tasks::export_xml(item)
        .with_context(|| format!("导出计划任务 XML 失败：{}", item.name))?;
    let xml_path = unique_backup_path(&format!("{}-task.xml", item.locator));
    std::fs::write(&xml_path, xml.as_bytes())
        .with_context(|| format!("写入计划任务 XML 备份失败：{}", xml_path.display()))?;

    let mut record = BackupRecord::from_item(item, mode);
    record.task = Some(TaskBackup {
        task_path: super::tasks::task_full_path(&item.scope, &item.locator),
        xml_path: xml_path.display().to_string(),
    });
    upsert_record(record)?;

    if let Err(err) = super::tasks::delete_task(item) {
        let _ = remove_record(&item.id);
        let _ = std::fs::remove_file(&xml_path);
        return Err(err).with_context(|| {
            format!("删除计划任务失败，已保留原任务：{}", item.name)
        });
    }
    Ok(())
}

/// 从备份记录还原原始数据（注册表值 / 文件 / 任务 XML）。
fn restore_backed_up(record: &BackupRecord) -> Result<()> {
    if let Some(registry) = record.registry.as_ref() {
        let hive = Hive::from_record_str(&registry.hive).ok_or_else(|| {
            anyhow::anyhow!("备份记录里的注册表根无效：{}", registry.hive)
        })?;
        let bytes = match registry.value_bytes_base64.as_deref() {
            Some(encoded) => base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .context("备份记录里的注册表原始字节损坏（base64 解码失败）")?,
            None => encode_registry_text(registry.value_type, &registry.value)?,
        };
        super::registry::write_raw_value(
            hive,
            &registry.key_path,
            &registry.value_name,
            registry.value_type,
            &bytes,
        )
        .with_context(|| format!("还原注册表值失败：{}", record.name))?;
        // 还原后确保不被 StartupApproved 残留状态挡住。
        super::registry::clear_disabled_state(
            record.source,
            &record.scope,
            &record.locator,
        )
        .with_context(|| format!("清除还原项的禁用状态失败：{}", record.name))?;
        return Ok(());
    }

    if let Some(file) = record.file.as_ref() {
        let original = Path::new(&file.original_path);
        let backup = Path::new(&file.backup_path);
        if !backup.is_file() {
            bail!(
                "备份文件已丢失，无法还原：{}",
                file.backup_path
            );
        }
        if let Some(parent) = original.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建启动文件夹失败：{}", parent.display()))?;
        }
        std::fs::copy(backup, original).with_context(|| {
            format!(
                "还原启动文件夹条目失败：{} → {}",
                file.backup_path,
                file.original_path
            )
        })?;
        // 还原后清掉可能残留的禁用状态。
        if let Some(hive) = folder_hive(&record.scope) {
            let full_path = original.display().to_string();
            if let Err(err) = startup_approved::delete_value(
                hive,
                startup_approved::APPROVED_STARTUP_FOLDER,
                &full_path,
            ) {
                eprintln!("[platform] 清除启动文件夹 StartupApproved 值失败（可忽略）：{err:#}");
            }
        }
        let _ = std::fs::remove_file(backup);
        return Ok(());
    }

    if let Some(task) = record.task.as_ref() {
        super::tasks::register_from_xml(&task.task_path, Path::new(&task.xml_path))
            .with_context(|| format!("还原计划任务失败：{}", record.name))?;
        // XML 备份保留在备份目录，便于出问题后手工导入，不主动删除。
        return Ok(());
    }

    bail!(
        "备份记录「{}」没有可还原的数据（native 模式请走系统开关恢复）",
        record.name
    )
}

// ---------------------------------------------------------------------------
// 工具函数
// ---------------------------------------------------------------------------

fn folder_hive(scope: &str) -> Option<Hive> {
    match scope {
        "user" => Some(Hive::CurrentUser),
        "machine" => Some(Hive::LocalMachine),
        _ => None,
    }
}

/// 备份目录里的唯一文件名（保留原扩展名，便于人工识别）。
fn unique_backup_path(locator: &str) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let unique = uuid::Uuid::new_v4().to_string();
    let unique = &unique[..8];
    let stem = sanitize_file_stem(locator);
    backup_dir().join(format!("{stamp}-{unique}-{stem}"))
}

/// 把任意条目名清洗成安全的文件名片段（非 ASCII 一律换成 `_`）。
pub(crate) fn sanitize_file_stem(text: &str) -> String {
    let mut out: String = text
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if out.len() > 80 {
        out.truncate(80);
    }
    if out.trim_matches('_').is_empty() {
        out.push_str("item");
    }
    out
}

/// 把注册表原始字节解码成可读文本（用于备份记录展示）。
pub(crate) fn decode_registry_text(value_type: u32, bytes: &[u8]) -> String {
    match value_type {
        // REG_SZ / REG_EXPAND_SZ / REG_MULTI_SZ：UTF-16LE
        1 | 2 | 7 => {
            let (pairs, _) = bytes.as_chunks::<2>();
            let units: Vec<u16> = pairs
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            String::from_utf16_lossy(&units)
                .trim_end_matches('\0')
                .replace('\0', ";")
        }
        // REG_DWORD
        4 if bytes.len() >= 4 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).to_string(),
        // REG_QWORD
        11 if bytes.len() >= 8 => u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
        .to_string(),
        _ => String::new(),
    }
}

/// 只有文本类型能在缺失原始字节时重建；其它类型必须依赖 base64 字节。
fn encode_registry_text(value_type: u32, text: &str) -> Result<Vec<u8>> {
    match value_type {
        1 | 2 => {
            let mut bytes: Vec<u8> = text.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
            bytes.extend_from_slice(&[0, 0]);
            Ok(bytes)
        }
        _ => bail!("备份记录缺少原始字节，且类型 {value_type} 不是字符串，无法重建注册表值"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";

    fn hkcu_run() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_READ | winreg::enums::KEY_WRITE)
            .expect("打开 HKCU Run 失败")
    }

    fn hkcu_run_once() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_ONCE_KEY, KEY_READ | winreg::enums::KEY_WRITE)
            .expect("打开 HKCU RunOnce 失败")
    }

    fn scan_item_by_id(id: &str) -> Option<AutostartItem> {
        crate::platform::scan_autostart()
            .into_iter()
            .find(|item| item.id == id)
    }

    fn cleanup_test_records(ids: &[&str]) {
        let mut records = load_index();
        records.retain(|record| !ids.contains(&record.id.as_str()));
        let _ = save_index(&records);
    }

    #[test]
    fn decode_registry_text_handles_string_and_dword() {
        // "Hi" 的 UTF-16LE + NUL
        let bytes = [b'H', 0, b'i', 0, 0, 0];
        assert_eq!(decode_registry_text(1, &bytes), "Hi");
        assert_eq!(decode_registry_text(4, &42u32.to_le_bytes()), "42");
        assert_eq!(decode_registry_text(3, &[1, 2, 3]), "");
    }

    #[test]
    fn encode_registry_text_roundtrip() {
        let bytes = encode_registry_text(1, "C:\\a b\\c.exe").unwrap();
        assert_eq!(decode_registry_text(1, &bytes), "C:\\a b\\c.exe");
        assert!(encode_registry_text(3, "x").is_err());
    }

    #[test]
    fn sanitize_file_stem_is_safe() {
        assert_eq!(sanitize_file_stem("Launch-Workbase.bat"), "Launch-Workbase.bat");
        // 中文与非 ASCII 全换成下划线（"任务 名称" = 5 个字符）
        assert_eq!(sanitize_file_stem("任务 名称.lnk"), "_____.lnk");
        assert!(!sanitize_file_stem("").is_empty());
        assert!(sanitize_file_stem(&"a".repeat(200)).len() <= 80);
    }

    /// 冒烟测试：RunOnce（不支持系统开关）→ disable 备份并移除 → restore 还原。
    ///
    /// 手动运行：`cargo test --lib backup_removed_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn backup_removed_smoke() {
        const NAME: &str = "DaemonManBackupSmokeOnce";
        const COMMAND: &str = r#""C:\Windows\System32\notepad.exe" --backup-smoke"#;
        let id = format!("registry-run-once::user::{NAME}");

        let run_once = hkcu_run_once();
        let _ = run_once.delete_value(NAME);
        run_once.set_value(NAME, &COMMAND).expect("写入临时 RunOnce 值失败");
        cleanup_test_records(&[&id]);

        let item = scan_item_by_id(&id).expect("临时 RunOnce 值没有出现在扫描结果里");
        println!(
            "扫描到 RunOnce：id={} toggle_supported={}",
            item.id, item.toggle_supported
        );
        assert!(!item.toggle_supported, "RunOnce 应标记为不支持系统开关");

        disable(&item).expect("备份并移除 RunOnce 失败");
        assert!(
            run_once.get_value::<String, _>(NAME).is_err(),
            "disable 后 RunOnce 值应已从注册表移除"
        );
        let record = find_record(&id).expect("备份索引里没有记录");
        println!(
            "备份记录：mode={} 注册表值类型={} 值={}",
            record.mode,
            record.registry.as_ref().map(|r| r.value_type).unwrap_or(0),
            record.registry.as_ref().map(|r| r.value.as_str()).unwrap_or("")
        );
        assert_eq!(record.mode, "removed");
        assert!(record.registry.is_some());
        assert!(scan_item_by_id(&id).is_none(), "移除后不应再扫描到该项");

        let restored = restore(&id).expect("从备份还原 RunOnce 失败");
        println!("还原后：id={} enabled={}", restored.id, restored.enabled);
        assert_eq!(
            run_once.get_value::<String, _>(NAME).expect("还原后 RunOnce 值不存在"),
            COMMAND
        );
        assert!(find_record(&id).is_none(), "还原后备份记录应被清理");

        let _ = run_once.delete_value(NAME);
        cleanup_test_records(&[&id]);
        println!("已清理");
    }

    /// 冒烟测试：native disable → restore（走 StartupApproved，不删原始值）。
    ///
    /// 手动运行：`cargo test --lib backup_native_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn backup_native_smoke() {
        const NAME: &str = "DaemonManBackupSmokeNative";
        const COMMAND: &str = r#""C:\Windows\System32\mstsc.exe" /backup-smoke"#;
        let id = format!("registry-run::user::{NAME}");

        let run = hkcu_run();
        let _ = run.delete_value(NAME);
        run.set_value(NAME, &COMMAND).expect("写入临时 Run 值失败");
        cleanup_test_records(&[&id]);
        let _ = startup_approved::delete_value(
            Hive::CurrentUser,
            startup_approved::APPROVED_RUN,
            NAME,
        );

        let item = scan_item_by_id(&id).expect("临时 Run 值没有出现在扫描结果里");
        assert!(item.toggle_supported);
        disable(&item).expect("native 禁用失败");
        let after_disable = scan_item_by_id(&id).expect("禁用后条目消失（id 不稳定）");
        println!("native 禁用后：enabled={}", after_disable.enabled);
        assert!(!after_disable.enabled);
        assert_eq!(
            run.get_value::<String, _>(NAME).expect("原始值被破坏"),
            COMMAND,
            "native 禁用不应修改原始 Run 值"
        );
        assert_eq!(find_record(&id).expect("缺少 native 备份记录").mode, "native");

        let restored = restore(&id).expect("native 恢复失败");
        println!("native 恢复后：enabled={}", restored.enabled);
        assert!(restored.enabled);
        assert!(find_record(&id).is_none(), "恢复后 native 记录应被清理");

        let _ = run.delete_value(NAME);
        let _ = startup_approved::delete_value(
            Hive::CurrentUser,
            startup_approved::APPROVED_RUN,
            NAME,
        );
        cleanup_test_records(&[&id]);
        println!("已清理");
    }

    /// 冒烟测试：delete 先备份再永久删除 → restore 从备份还原。
    ///
    /// 手动运行：`cargo test --lib backup_delete_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn backup_delete_smoke() {
        const NAME: &str = "DaemonManBackupSmokeDelete";
        const COMMAND: &str = r#""C:\Windows\System32\charmap.exe" /del-smoke"#;
        let id = format!("registry-run::user::{NAME}");

        let run = hkcu_run();
        let _ = run.delete_value(NAME);
        run.set_value(NAME, &COMMAND).expect("写入临时 Run 值失败");
        cleanup_test_records(&[&id]);

        let item = scan_item_by_id(&id).expect("临时 Run 值没有出现在扫描结果里");
        delete(&item).expect("删除自启动项失败");
        assert!(
            run.get_value::<String, _>(NAME).is_err(),
            "delete 后 Run 值应已删除"
        );
        let record = find_record(&id).expect("删除后应有 deleted 备份记录");
        assert_eq!(record.mode, "deleted");
        println!(
            "已删除并备份：mode={}，备份目录 = {}",
            record.mode,
            backup_dir().display()
        );

        let restored = restore(&id).expect("从 deleted 备份还原失败");
        println!("从备份还原：id={} enabled={}", restored.id, restored.enabled);
        assert_eq!(
            run.get_value::<String, _>(NAME).expect("还原后 Run 值不存在"),
            COMMAND
        );
        assert!(find_record(&id).is_none());

        let _ = run.delete_value(NAME);
        cleanup_test_records(&[&id]);
        println!("已清理");
    }

    /// 冒烟测试：启动文件夹条目 delete → 备份副本 → restore 还原回原路径。
    ///
    /// 手动运行：`cargo test --lib backup_folder_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn backup_folder_smoke() {
        const NAME: &str = "DaemonManBackupSmoke.bat";
        let id = format!("startup-folder::user::{NAME}");

        let path = super::super::startup_folder::item_path("user", NAME)
            .expect("无法定位用户启动文件夹");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("创建启动文件夹失败");
        }
        std::fs::write(&path, "@echo off\r\nrem daemon-man backup smoke\r\n")
            .expect("写入临时启动文件夹条目失败");
        cleanup_test_records(&[&id]);

        let item = scan_item_by_id(&id).expect("临时启动文件夹条目没有出现在扫描结果里");
        assert!(item.can_delete);
        delete(&item).expect("删除启动文件夹条目失败");
        assert!(!path.is_file(), "delete 后原文件应已移除");

        let record = find_record(&id).expect("删除后应有备份记录");
        assert_eq!(record.mode, "deleted");
        let file = record.file.as_ref().expect("备份记录缺少文件信息");
        println!(
            "已备份：{} → {}",
            file.original_path, file.backup_path
        );
        assert!(Path::new(&file.backup_path).is_file(), "备份副本不存在");

        let restored = restore(&id).expect("从备份还原启动文件夹条目失败");
        println!("还原后：id={} enabled={}", restored.id, restored.enabled);
        assert!(path.is_file(), "还原后文件应回到启动文件夹");
        assert!(find_record(&id).is_none());

        let _ = std::fs::remove_file(&path);
        cleanup_test_records(&[&id]);
        println!("已清理");
    }

    /// 冒烟测试：真实导出计划任务 XML（只读，验证任务备份所需能力）。
    ///
    /// 手动运行：`cargo test --lib task_export_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn task_export_smoke() {
        let task = crate::platform::scan_autostart()
            .into_iter()
            .find(|item| item.source == AutostartSource::ScheduledTask);
        let Some(task) = task else {
            println!("本机没有可用的登录计划任务，跳过");
            return;
        };
        let xml = crate::platform::tasks::export_xml(&task).expect("导出计划任务 XML 失败");
        println!(
            "任务「{}」XML 长度 = {} 字节，包含 Task 根节点 = {}",
            task.name,
            xml.len(),
            xml.contains("<Task")
        );
        assert!(xml.contains("<Task"), "导出的 XML 不像任务定义");
        assert!(xml.to_ascii_lowercase().contains("logon"));
    }
}
