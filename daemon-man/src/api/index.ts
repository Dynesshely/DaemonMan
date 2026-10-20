import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AppConfig,
  AppSettings,
  AutostartItem,
  LegacyImportResult,
  LogEntry,
  ProcessInfo,
  SystemInfo,
  TakeOverResult,
  Workflow,
  WorkflowRun,
  WorkflowValidation,
} from "./types";

export * from "./types";

/** 与 Rust `commands::*` 一一对应的调用封装。 */
export const api = {
  // 配置 / 系统
  getConfig: () => invoke<AppConfig>("get_app_config"),
  saveSettings: (settings: AppSettings) =>
    invoke<AppConfig>("save_settings", { settings }),
  getSystemInfo: () => invoke<SystemInfo>("get_system_info"),
  setSelfAutostart: (enabled: boolean) =>
    invoke<boolean>("set_self_autostart", { enabled }),
  getSelfAutostart: () => invoke<boolean>("get_self_autostart"),

  // 工作流
  listWorkflows: () => invoke<Workflow[]>("list_workflows"),
  saveWorkflow: (workflow: Workflow) =>
    invoke<Workflow>("save_workflow", { workflow }),
  deleteWorkflow: (id: string) => invoke<void>("delete_workflow", { id }),
  duplicateWorkflow: (id: string) =>
    invoke<Workflow>("duplicate_workflow", { id }),
  validateWorkflow: (workflow: Workflow) =>
    invoke<WorkflowValidation>("validate_workflow", { workflow }),
  runWorkflow: (workflowId: string) =>
    invoke<WorkflowRun>("run_workflow", { workflowId }),
  runAllEnabled: () => invoke<WorkflowRun[]>("run_all_enabled"),
  cancelRun: (runId: string) => invoke<void>("cancel_run", { runId }),
  getRecentRuns: () => invoke<WorkflowRun[]>("get_recent_runs"),
  testLaunch: (path: string, args?: string[], workingDir?: string) =>
    invoke<number>("test_launch", {
      path,
      args: args ?? null,
      workingDir: workingDir ?? null,
    }),

  // 自启动项
  scanAutostart: (force = false) =>
    invoke<AutostartItem[]>("scan_autostart", { force }),
  setAutostartEnabled: (itemId: string, enabled: boolean) =>
    invoke<AutostartItem>("set_autostart_enabled", { itemId, enabled }),
  takeOverAutostart: (
    itemIds: string[],
    workflowId?: string,
    workflowName?: string,
  ) =>
    invoke<TakeOverResult>("take_over_autostart", {
      itemIds,
      workflowId: workflowId ?? null,
      workflowName: workflowName ?? null,
    }),
  releaseAutostart: (itemId: string) =>
    invoke<AutostartItem>("release_autostart", { itemId }),

  // 进程
  listProcesses: () => invoke<ProcessInfo[]>("list_processes"),
  killProcess: (pid: number) => invoke<void>("kill_process", { pid }),

  // 日志 / 配置导入导出
  getLogs: (limit = 500) => invoke<LogEntry[]>("get_logs", { limit }),
  clearLogs: () => invoke<void>("clear_logs"),
  exportLogs: (path: string) => invoke<void>("export_logs", { path }),
  exportConfig: (path: string) => invoke<void>("export_config", { path }),
  importConfig: (path: string) => invoke<AppConfig>("import_config", { path }),

  /** 读取旧版 ProgressManager 的 config.json 并转换为工作流。 */
  importLegacyProgressManager: (path: string) =>
    invoke<LegacyImportResult>("import_legacy_config", { path }),

  openConfigDir: () => invoke<void>("open_config_dir"),
  openLogDir: () => invoke<void>("open_log_dir"),
  quitApp: () => invoke<void>("quit_app"),
};

/** Rust 侧 `app.emit` 的事件订阅。 */
export const events = {
  onRun: (handler: (run: WorkflowRun) => void): Promise<UnlistenFn> =>
    listen<WorkflowRun>("workflow:run", (event) => handler(event.payload)),
  onLog: (handler: (entry: LogEntry) => void): Promise<UnlistenFn> =>
    listen<LogEntry>("log:entry", (event) => handler(event.payload)),
  onNavigate: (handler: (path: string) => void): Promise<UnlistenFn> =>
    listen<string>("navigate", (event) => handler(event.payload)),
  onAppReady: (handler: () => void): Promise<UnlistenFn> =>
    listen<boolean>("app:ready", () => handler()),
};
