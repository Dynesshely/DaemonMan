/**
 * 与 Rust `src/model.rs` 一一对应的 IPC 数据类型。
 * 修改任一侧都必须同步另一侧（serde 使用 camelCase）。
 */

export type NodeKind = "application" | "delay" | "wait" | "script";
export type EdgeCondition = "success" | "failure" | "always";
export type RunStatus = "running" | "success" | "failed" | "cancelled";
export type NodeStatus =
  | "pending"
  | "waiting"
  | "running"
  | "success"
  | "failed"
  | "skipped"
  | "cancelled";
export type RunTrigger = "manual" | "startup" | "tray";
export type AutostartSource =
  | "registry-run"
  | "registry-runonce"
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
  requiresAdmin: boolean;
  isSelf: boolean;
  takenOver: boolean;
  scope: string;
  locator: string;
  detail: string | null;
}

export interface AutostartOverride {
  id: string;
  name: string;
  source: AutostartSource;
  disabledAt: string;
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

export interface LogEntry {
  id: number;
  ts: string;
  level: LogLevel;
  scope: string;
  message: string;
  detail: string | null;
}
