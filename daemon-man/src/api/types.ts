/**
 * 与 Rust `src/model.rs` 一一对应的 IPC 数据类型。
 * 修改任一侧都必须同步另一侧（serde 使用 camelCase）。
 */

export type NodeKind = "application" | "delay" | "wait" | "script" | "group";
export type EdgeCondition = "success" | "failure" | "always";
export type RunStatus = "running" | "success" | "failed" | "cancelled";
export type NodeStatus =
  | "pending"
  | "waiting"
  | "running"
  | "success"
  | "failed"
  | "skipped"
  | "cancelled"
  | "skipped-existing";
export type RunTrigger = "manual" | "startup" | "tray";
export type AutostartSource =
  | "registry-run"
  | "registry-run-once"
  | "startup-folder"
  | "scheduled-task"
  | "service";
export type LogLevel = "debug" | "info" | "warn" | "error" | "success";

export interface AppSettings {
  language: string;
  theme: string;
  launchAtLogin: boolean;
  startMinimized: boolean;
  minimizeToTray: boolean;
  runWorkflowsOnStart: boolean;
  startupGraceMs: number;
  defaultNodeTimeoutMs: number;
  maxConcurrency: number;
  maxRunHistory: number;
  maxLogEntries: number;
  /** 工作流编辑器撤销/重做历史层数（1..=10000） */
  undoLimit: number;
  confirmTakeOver: boolean;
  confirmQuit: boolean;
}

export interface AppConfig {
  version: number;
  settings: AppSettings;
  workflows: Workflow[];
  overrides: AutostartOverride[];
  updatedAt: string | null;
}

export interface NodePosition {
  x: number;
  y: number;
}

export interface EnvVar {
  key: string;
  value: string;
}

export interface AppLaunchSpec {
  path: string;
  args: string[];
  workingDir: string | null;
  env: EnvVar[];
  skipIfRunning: boolean;
  processName: string | null;
  waitForWindow: boolean;
  windowTimeoutMs: number;
  closeMainWindow: boolean;
  focusWindow: boolean;
  runAsAdmin: boolean;
  settleMs: number;
}

export interface WaitSpec {
  processName: string;
  windowVisible: boolean;
  timeoutMs: number;
  failOnTimeout: boolean;
}

export interface ScriptSpec {
  command: string;
  workingDir: string | null;
  timeoutMs: number;
  failOnNonzero: boolean;
}

export interface WorkflowNode {
  id: string;
  name: string;
  kind: NodeKind;
  enabled: boolean;
  position: NodePosition;
  app: AppLaunchSpec;
  delayMs: number;
  wait: WaitSpec;
  script: ScriptSpec;
  note: string;
  /** 所属分组节点 id（仅用于展示与排版） */
  parentId: string | null;
  /** 分组节点：子项是否并行（紧凑排版） */
  groupParallel: boolean;
}

export interface WorkflowEdge {
  id: string;
  source: string;
  target: string;
  condition: EdgeCondition;
}

export interface Workflow {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
  /** 工作流级「跳过已存在进程」 */
  skipExistingProcesses: boolean;
}

export interface WorkflowValidation {
  ok: boolean;
  errors: string[];
  warnings: string[];
  order: string[];
}

export interface NodeRunState {
  nodeId: string;
  nodeName: string;
  status: NodeStatus;
  message: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  pid: number | null;
  attempt: number;
}

export interface WorkflowRun {
  runId: string;
  workflowId: string;
  workflowName: string;
  trigger: RunTrigger;
  status: RunStatus;
  startedAt: string;
  finishedAt: string | null;
  message: string | null;
  nodes: Record<string, NodeRunState>;
}

export interface AutostartItem {
  id: string;
  name: string;
  source: AutostartSource;
  sourceLabel: string;
  command: string;
  executable: string | null;
  args: string | null;
  enabled: boolean;
  toggleSupported: boolean;
  canDelete: boolean;
  requiresAdmin: boolean;
  isSelf: boolean;
  takenOver: boolean;
  scope: string;
  locator: string;
  detail: string | null;
  /** 仅前端使用：该行来自「备份后移除」的 override 记录，不在系统扫描结果中。 */
  removed?: boolean;
}

export interface AutostartOverride {
  id: string;
  name: string;
  source: AutostartSource;
  disabledAt: string;
  /** native = 系统开关禁用；removed = 备份后从原位置移除 */
  mode: string;
  /** 原始作用域（user / machine…），用于「备份后移除」行继续参与位置筛选。 */
  scope: string;
  /** 原始启动命令，用于「已移除」行的展示与搜索。 */
  command: string;
}

/** 批量图标提取结果。 */
export interface FileIcon {
  path: string;
  dataUrl: string | null;
}

/** 启动脚本内容（右键编辑启动项）。 */
export interface ScriptContent {
  path: string;
  content: string;
  size: number;
  /** utf-8 | utf-8-bom | gb18030 */
  encoding: string;
}

export interface TakeOverResult {
  workflow: Workflow;
  items: AutostartItem[];
  skipped: string[];
  failures: string[];
}

export interface ProcessInfo {
  pid: number;
  name: string;
  executable: string | null;
  memoryBytes: number;
  cpuUsage: number;
  runningSecs: number;
  managed: boolean;
  workflowName: string | null;
  mainWindowTitle: string | null;
}

/** 按工作流节点追踪到的运行中进程（含非 DaemonMan 启动的进程）。 */
export interface TrackedProcess {
  workflowId: string;
  workflowName: string;
  nodeId: string;
  nodeName: string;
  pid: number;
  name: string;
  executable: string | null;
  memoryBytes: number;
  cpuUsage: number;
  runningSecs: number;
  managed: boolean;
  mainWindowTitle: string | null;
}

export interface SystemInfo {
  os: string;
  arch: string;
  hostname: string;
  appVersion: string;
  configPath: string;
  logPath: string;
  elevated: boolean;
  autostartTotal: number;
  autostartDisabled: number;
  workflowCount: number;
  runningWorkflows: number;
  uptimeSecs: number;
}

/** 开始菜单里的可启动应用（新建启动应用时快速选择）。 */
export interface StartMenuApp {
  name: string;
  /** `.lnk` 完整路径，可直接作为节点 app.path */
  path: string;
  /** 解析出的目标可执行文件（用于图标/预览） */
  target: string | null;
  args: string[];
  location: "user" | "machine";
}

export interface LogEntry {
  id: number;
  ts: string;
  level: LogLevel;
  scope: string;
  message: string;
  detail: string | null;
}
