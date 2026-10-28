//! 全局数据模型。
//!
//! 这里的类型同时是：
//! - 配置文件（`~/.daemon-man/config.json`）的序列化结构；
//! - Rust 后端与前端 TypeScript 之间的 IPC 契约（`serde(rename_all = "camelCase")`）。
//!
//! 修改本文件必须同步修改 `src/api/types.ts`。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 设置与配置
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// "zh-CN" | "en"
    pub language: String,
    /// "system" | "light" | "dark"
    pub theme: String,
    /// 开机自启 DaemonMan 自身（写入 HKCU Run）
    pub launch_at_login: bool,
    /// 以 `--autostart` 启动时隐藏主窗口
    pub start_minimized: bool,
    /// 关闭主窗口时隐藏到托盘而不是退出
    pub minimize_to_tray: bool,
    /// 登录启动后自动执行所有「启用」的工作流
    pub run_workflows_on_start: bool,
    /// 登录启动后等待多久再开始执行工作流（毫秒）
    pub startup_grace_ms: u64,
    /// 工作流内单个节点的默认超时（毫秒），0 表示不限制
    pub default_node_timeout_ms: u64,
    /// 同时执行的最大节点数
    pub max_concurrency: usize,
    /// 工作流编辑器的撤销/重做历史层数（1..=10000，默认 100）
    pub undo_limit: usize,
    /// 历史运行记录保留条数
    pub max_run_history: usize,
    /// 日志保留条数
    pub max_log_entries: usize,
    /// 接管自启动项时是否二次确认
    pub confirm_take_over: bool,
    /// 关闭窗口/托盘退出时是否询问
    pub confirm_quit: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: "zh-CN".into(),
            theme: "system".into(),
            launch_at_login: false,
            start_minimized: true,
            minimize_to_tray: true,
            run_workflows_on_start: true,
            startup_grace_ms: 3000,
            default_node_timeout_ms: 30_000,
            max_concurrency: 8,
            undo_limit: 100,
            max_run_history: 50,
            max_log_entries: 2000,
            confirm_take_over: true,
            confirm_quit: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub version: u32,
    pub settings: AppSettings,
    /// 工作流单独存放在 `<data_dir>/workflows/<id>.json`；
    /// 这里仅作为运行时视图 / IPC 返回值，不会写入 config.json（见 `config::ConfigFile`）。
    #[serde(default)]
    pub workflows: Vec<Workflow>,
    /// 被 DaemonMan 关闭并接管的自启动项记录，用于恢复
    pub overrides: Vec<AutostartOverride>,
    /// 上次写入配置的时间（ISO8601）
    pub updated_at: Option<String>,
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            version: CONFIG_VERSION,
            ..Default::default()
        }
    }

    pub fn workflow(&self, id: &str) -> Option<&Workflow> {
        self.workflows.iter().find(|w| w.id == id)
    }

    pub fn workflow_mut(&mut self, id: &str) -> Option<&mut Workflow> {
        self.workflows.iter_mut().find(|w| w.id == id)
    }
}

// ---------------------------------------------------------------------------
// 工作流
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 是否在登录启动时自动执行
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    /// 工作流级「跳过已存在进程」：开启时，已在运行的应用节点会被跳过（标记为已运行跳过）
    pub skip_existing_processes: bool,
}

impl Default for Workflow {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            enabled: false,
            created_at: String::new(),
            updated_at: String::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            skip_existing_processes: true,
        }
    }
}

impl Workflow {
    pub fn new(name: impl Into<String>) -> Self {
        let now = crate::model::now_iso();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            description: String::new(),
            enabled: false,
            created_at: now.clone(),
            updated_at: now,
            nodes: Vec::new(),
            edges: Vec::new(),
            skip_existing_processes: true,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now_iso();
    }

    pub fn node(&self, id: &str) -> Option<&WorkflowNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// 启动一个可执行文件 / 快捷方式
    #[default]
    Application,
    /// 等待一段时间
    Delay,
    /// 等待某个进程出现
    Wait,
    /// 执行一条 shell 命令
    Script,
    /// 分组节点：只用于展示结构，不参与执行
    Group,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

/// 启动一个程序的完整描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppLaunchSpec {
    /// exe / lnk / bat 的绝对路径，或者一个可以直接启动的 URL
    pub path: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub env: Vec<EnvVar>,
    /// 已经在运行则跳过启动
    pub skip_if_running: bool,
    /// 进程名覆盖（默认从 path 推导）
    pub process_name: Option<String>,
    /// 启动后等待主窗口出现，再放行后继节点
    pub wait_for_window: bool,
    /// 等待主窗口的超时时间（毫秒）
    pub window_timeout_ms: u64,
    /// 主窗口出现后发送 WM_CLOSE（启动器 / 托盘类程序常见需求）
    pub close_main_window: bool,
    /// 窗口出现后置前
    pub focus_window: bool,
    /// 以管理员身份启动
    pub run_as_admin: bool,
    /// 启动后额外等待的时间（毫秒）
    pub settle_ms: u64,
}

impl Default for AppLaunchSpec {
    fn default() -> Self {
        Self {
            path: String::new(),
            args: Vec::new(),
            working_dir: None,
            env: Vec::new(),
            skip_if_running: true,
            process_name: None,
            wait_for_window: false,
            window_timeout_ms: 15_000,
            close_main_window: false,
            focus_window: false,
            run_as_admin: false,
            settle_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WaitSpec {
    /// 等待的进程名（不含 .exe）
    pub process_name: String,
    /// 是否必须等到主窗口可见
    pub window_visible: bool,
    pub timeout_ms: u64,
    /// 超时后是否视作失败（false = 仅告警继续）
    pub fail_on_timeout: bool,
}

impl Default for WaitSpec {
    fn default() -> Self {
        Self {
            process_name: String::new(),
            window_visible: false,
            timeout_ms: 30_000,
            fail_on_timeout: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptSpec {
    /// 通过 cmd.exe /C 执行
    pub command: String,
    pub working_dir: Option<String>,
    /// 0 表示不限制
    pub timeout_ms: u64,
    /// 命令返回值非 0 是否视作失败
    pub fail_on_nonzero: bool,
}

impl Default for ScriptSpec {
    fn default() -> Self {
        Self {
            command: String::new(),
            working_dir: None,
            timeout_ms: 60_000,
            fail_on_nonzero: true,
        }
    }
}

/// 分组启动模式：
/// - Sequence：按组内顺序依次启动，忽略组内连线（保留连线以便切回其它模式）；
/// - Parallel：组内节点同时启动，忽略组内连线；
/// - Graph：按组内连线依赖执行（旧行为）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupMode {
    Sequence,
    Parallel,
    Graph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkflowNode {
    pub id: String,
    /// 展示名称，默认取程序名
    pub name: String,
    pub kind: NodeKind,
    pub enabled: bool,
    pub position: NodePosition,
    pub app: AppLaunchSpec,
    pub delay_ms: u64,
    pub wait: WaitSpec,
    pub script: ScriptSpec,
    pub note: String,
    /// 所属分组节点 id（仅用于展示与排版；执行顺序仍由 edges 决定）
    pub parent_id: Option<String>,
    /// 分组节点：子项是否并行（用于紧凑排版与文本视图标记；兼容字段，新代码用 group_mode）
    pub group_parallel: bool,
    /// 分组节点的启动模式（旧配置缺省时由 group_parallel 推导）
    pub group_mode: Option<GroupMode>,
    /// 分组级延迟：顺序模式下应用于组内相邻节点之间，覆盖组内连线上的延时
    pub group_delay_ms: u64,
    /// 并行分组每批最多同时启动的直接子项数；0 表示不限制。
    /// 例如 limit=3、interval=3000：每批 3 个，上一批开始后至少隔 3 秒再启动下一批
    /// （上一批还没全部完成时会等它完成，因此「最大并发」永远不被突破）。
    pub group_parallel_limit: u32,
    /// 并行分组批次之间的间隔毫秒（仅 limit > 0 且确有多个批次时生效）。
    pub group_parallel_interval_ms: u64,
    /// 顺序分组是否使用简洁展示（紧凑换行、隐藏组内连线、拖拽排序）
    pub group_compact: bool,
}

impl Default for WorkflowNode {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            kind: NodeKind::Application,
            enabled: true,
            position: NodePosition::default(),
            app: AppLaunchSpec::default(),
            delay_ms: 0,
            wait: WaitSpec::default(),
            script: ScriptSpec::default(),
            note: String::new(),
            parent_id: None,
            group_parallel: false,
            group_mode: None,
            group_delay_ms: 0,
            group_parallel_limit: 0,
            group_parallel_interval_ms: 0,
            group_compact: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeCondition {
    /// 上游成功才执行
    #[default]
    Success,
    /// 上游失败才执行
    Failure,
    /// 无论上下游都会执行
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub condition: EdgeCondition,
}

impl Default for WorkflowEdge {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source: String::new(),
            target: String::new(),
            condition: EdgeCondition::Success,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkflowValidation {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// 拓扑顺序中的节点 id
    pub order: Vec<String>,
}

// ---------------------------------------------------------------------------
// 运行状态
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Pending,
    Waiting,
    Running,
    Success,
    Failed,
    Skipped,
    Cancelled,
    /// 因进程已在运行而跳过（视为成功，但界面用独特颜色标记）
    #[serde(rename = "skipped-existing")]
    SkippedExisting,
}

impl NodeStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            NodeStatus::Success
                | NodeStatus::Failed
                | NodeStatus::Skipped
                | NodeStatus::Cancelled
                | NodeStatus::SkippedExisting
        )
    }

    /// 对依赖条件而言是否算「成功」。
    pub fn is_success(self) -> bool {
        matches!(self, NodeStatus::Success | NodeStatus::SkippedExisting)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunTrigger {
    Manual,
    Startup,
    Tray,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeRunState {
    pub node_id: String,
    pub node_name: String,
    pub status: NodeStatus,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub pid: Option<u32>,
    pub attempt: u32,
}

impl Default for NodeRunState {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            node_name: String::new(),
            status: NodeStatus::Pending,
            message: None,
            started_at: None,
            finished_at: None,
            pid: None,
            attempt: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkflowRun {
    pub run_id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub trigger: RunTrigger,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub message: Option<String>,
    pub nodes: HashMap<String, NodeRunState>,
}

impl Default for WorkflowRun {
    fn default() -> Self {
        Self {
            run_id: String::new(),
            workflow_id: String::new(),
            workflow_name: String::new(),
            trigger: RunTrigger::Manual,
            status: RunStatus::Running,
            started_at: now_iso(),
            finished_at: None,
            message: None,
            nodes: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// 自启动项
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AutostartSource {
    /// HKCU / HKLM ...\Run
    RegistryRun,
    RegistryRunOnce,
    /// 当前用户 / 所有用户的启动文件夹
    StartupFolder,
    /// Windows 计划任务（登录触发器）
    ScheduledTask,
    /// 自动启动的 Windows 服务
    Service,
}

impl AutostartSource {
    pub fn label(self) -> &'static str {
        match self {
            AutostartSource::RegistryRun => "注册表 Run",
            AutostartSource::RegistryRunOnce => "注册表 RunOnce",
            AutostartSource::StartupFolder => "启动文件夹",
            AutostartSource::ScheduledTask => "计划任务",
            AutostartSource::Service => "系统服务",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutostartItem {
    /// 稳定 id：`source::scope::locator`，可以直接用于再次定位
    pub id: String,
    pub name: String,
    pub source: AutostartSource,
    pub source_label: String,
    /// 原始命令行
    pub command: String,
    /// 解析后的可执行文件路径（尽力而为）
    pub executable: Option<String>,
    pub args: Option<String>,
    /// 当前是否实际生效（已考虑 StartupApproved 等系统开关）
    pub enabled: bool,
    /// 是否支持由 DaemonMan 开关
    pub toggle_supported: bool,
    /// 是否支持从原位置永久删除（系统服务不支持）
    pub can_delete: bool,
    /// 开关是否需要管理员权限
    pub requires_admin: bool,
    /// 是否是 DaemonMan 自己
    pub is_self: bool,
    /// 是否已被 DaemonMan 接管（已关闭并加入工作流）
    pub taken_over: bool,
    /// 归属范围：`user` / `machine` / 任务路径 / 服务名
    pub scope: String,
    /// 定位信息（注册表值名、文件路径、任务名、服务名）
    pub locator: String,
    /// 附加信息（发布者、描述等）
    pub detail: Option<String>,
}

impl Default for AutostartItem {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            source: AutostartSource::RegistryRun,
            source_label: String::new(),
            command: String::new(),
            executable: None,
            args: None,
            enabled: true,
            toggle_supported: true,
            can_delete: false,
            requires_admin: false,
            is_self: false,
            taken_over: false,
            scope: String::new(),
            locator: String::new(),
            detail: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutostartOverride {
    pub id: String,
    pub name: String,
    pub source: AutostartSource,
    pub disabled_at: String,
    /// 禁用方式：`native`（系统开关）或 `removed`（备份后从原位置移除）
    pub mode: String,
    /// 原始作用域（user / machine…），供前端展示「已移除」项的位置
    pub scope: String,
    /// 原始启动命令，供前端展示与搜索
    pub command: String,
}

impl Default for AutostartOverride {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            source: AutostartSource::RegistryRun,
            disabled_at: now_iso(),
            mode: "native".into(),
            scope: String::new(),
            command: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// 进程 / 系统信息 / 日志
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub executable: Option<String>,
    pub memory_bytes: u64,
    pub cpu_usage: f32,
    pub running_secs: u64,
    /// 是否由 DaemonMan 启动
    pub managed: bool,
    /// 关联的工作流（managed 时）
    pub workflow_name: Option<String>,
    pub main_window_title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub hostname: String,
    pub app_version: String,
    pub config_path: String,
    pub log_path: String,
    pub elevated: bool,
    pub autostart_total: usize,
    pub autostart_disabled: usize,
    pub workflow_count: usize,
    pub running_workflows: usize,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
    Success,
}

/// 开始菜单里的可启动应用（用于「新建启动应用」快速选择）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMenuApp {
    /// 显示名（快捷方式文件名）
    pub name: String,
    /// `.lnk` 完整路径（可直接作为节点 app.path）
    pub path: String,
    /// 解析出的目标可执行文件（用于图标/预览）
    pub target: Option<String>,
    pub args: Vec<String>,
    /// "user" | "machine"
    pub location: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackedProcess {
    pub workflow_id: String,
    pub workflow_name: String,
    pub node_id: String,
    pub node_name: String,
    pub pid: u32,
    pub name: String,
    pub executable: Option<String>,
    pub memory_bytes: u64,
    pub cpu_usage: f32,
    pub running_secs: u64,
    /// 该进程是否由 DaemonMan 的某次运行托管
    pub managed: bool,
    pub main_window_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub ts: String,
    pub level: LogLevel,
    pub scope: String,
    pub message: String,
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

pub fn now_iso() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
