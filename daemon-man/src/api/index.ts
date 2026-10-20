import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AppConfig,
  AppSettings,
  AutostartItem,
  FileIcon,
  LogEntry,
  ProcessInfo,
  ScriptContent,
  StartMenuApp,
  SystemInfo,
  TakeOverResult,
  TrackedProcess,
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

  /** 右键「禁用」：native 开关或备份后移除。 */
  disableAutostartItem: (itemId: string) =>
    invoke<AutostartItem>("disable_autostart_item", { itemId }),

  /** 右键「恢复」：启用系统开关或从备份还原。 */
  restoreAutostartItem: (itemId: string) =>
    invoke<AutostartItem>("restore_autostart_item", { itemId }),

  /** 右键「删除」：先备份到配置目录再从原位置删除。 */
  deleteAutostartItem: (itemId: string) =>
    invoke<void>("delete_autostart_item", { itemId }),

  /** 批量提取文件图标（PNG data URL）。 */
  getFileIcons: (paths: string[], size = 32) =>
    invoke<FileIcon[]>("get_file_icons", { paths, size }),

  /** 列出开始菜单里的可见应用（新建启动应用时快速选择）。 */
  listStartMenuApps: () => invoke<StartMenuApp[]>("list_start_menu_apps"),

  /** 右键「在资源管理器中显示」。 */
  revealAutostartItem: (itemId: string) =>
    invoke<string>("reveal_autostart_item", { itemId }),

  /** 右键「编辑启动内容」：读取脚本文件。 */
  readAutostartScript: (itemId: string) =>
    invoke<ScriptContent>("read_autostart_script", { itemId }),

  /** 右键「编辑启动内容」：保存脚本（自动备份，保持读取时的编码）。 */
  writeAutostartScript: (itemId: string, content: string, encoding?: string) =>
    invoke<ScriptContent>("write_autostart_script", {
      itemId,
      content,
      encoding: encoding ?? null,
    }),

  // 进程
  listProcesses: () => invoke<ProcessInfo[]>("list_processes"),
  killProcess: (pid: number) => invoke<void>("kill_process", { pid }),

  /** 单点执行：用当前草稿中的某个节点发起一次调试运行。 */
  runSingleNode: (workflow: Workflow, nodeId: string) =>
    invoke<WorkflowRun>("run_single_node", { workflow, nodeId }),

  /** 单点启动分组：运行分组内所有可执行节点。 */
  runSingleGroup: (workflow: Workflow, groupId: string) =>
    invoke<WorkflowRun>("run_single_group", { workflow, groupId }),

  /** 按工作流节点追踪当前运行中的进程（含非 DaemonMan 启动的进程）。 */
  trackWorkflowProcesses: (workflowId?: string | null) =>
    invoke<TrackedProcess[]>("track_workflow_processes", {
      workflowId: workflowId ?? null,
    }),

  /** 临时提权执行自启动操作（GUI 仍以普通权限运行）。 */
  runElevatedAutostart: (
    action: "enable" | "disable" | "restore" | "delete",
    itemId: string,
    enabled?: boolean,
  ) =>
    invoke<AutostartItem>("elevate_autostart_op", {
      action,
      itemId,
      enabled: enabled ?? null,
    }),

  // 日志 / 配置导入导出
  getLogs: (limit = 500) => invoke<LogEntry[]>("get_logs", { limit }),
  clearLogs: () => invoke<void>("clear_logs"),
  exportLogs: (path: string) => invoke<void>("export_logs", { path }),
  exportConfig: (path: string) => invoke<void>("export_config", { path }),
  importConfig: (path: string) => invoke<AppConfig>("import_config", { path }),

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
