# DaemonMan 架构说明

## 1. 分层

```
Vue 视图 / Pinia store / api 封装
            │  invoke("command", args)          ▲ events: workflow:run / log:entry / navigate
            ▼                                    │
commands/*  ── 参数校验、权限与配置持久化 ───────────┤
            │                                    │
   ┌────────┴─────────┐                          │
   ▼                  ▼                          │
engine/*          platform/*                     │
DAG 校验与调度     Windows 集成（注册表/任务/服务/进程/窗口）
   │                  │
   └──────► state.rs / config.rs / model.rs ◄────┘
```

- `model.rs` 同时是配置文件结构、IPC 契约与运行状态模型，字段全部 `camelCase` 序列化；前端 `src/api/types.ts` 与之逐字段对应。
- `state.rs` 持有 `RwLock<AppConfig>`、运行记录（含历史上限）、日志环形缓冲、每个运行的取消标记，以及 5 秒 TTL 的自启动扫描缓存。
- 命令层（`commands/`）只做参数校验、配置修改与日志，不包含平台细节；`autostart` 命令负责「接管」这种跨层业务（关闭系统自启 + 写入工作流 + 记录 override）。

## 2. 配置模型

```jsonc
{
  "version": 1,
  "settings": { /* 语言、主题、登录自启、并发、超时、历史上限… */ },
  "workflows": [
    {
      "id": "uuid",
      "name": "登录启动",
      "enabled": true,                    // 登录时自动运行
      "nodes": [
        {
          "id": "uuid",
          "name": "Clash Verge",
          "kind": "application",          // application | delay | wait | script
          "enabled": true,
          "position": { "x": 80, "y": 80 },
          "app": {
            "path": "C:\\Program Files\\Clash Verge\\clash-verge.exe",
            "args": [],
            "workingDir": null,
            "env": [],
            "skipIfRunning": true,
            "processName": null,
            "waitForWindow": false,
            "windowTimeoutMs": 15000,
            "closeMainWindow": true,      // 启动后关闭启动画面
            "focusWindow": false,
            "runAsAdmin": false,
            "settleMs": 0
          },
          "delayMs": 0,
          "wait": { "processName": "", "windowVisible": false, "timeoutMs": 30000, "failOnTimeout": true },
          "script": { "command": "", "workingDir": null, "timeoutMs": 60000, "failOnNonzero": true },
          "note": ""
        }
      ],
      "edges": [
        { "id": "uuid", "source": "node-a", "target": "node-b", "condition": "success" }
      ]
    }
  ],
  "overrides": [ { "id": "registry-run::user::Foo", "name": "Foo", "source": "registry-run", "disabledAt": "…" } ]
}
```

配置写入采用「临时文件 + rename」原子替换，旧文件保留为 `config.json.bak`；解析失败会把坏文件备份为 `config.broken-<时间>.json` 并回退默认配置。

**工作流单独存储**：`config.json` 只包含 `version / settings / overrides / updatedAt`，每个工作流独立保存为 `workflows/<id>.json`（原子写 + `.bak`）。`AppConfig.workflows` 只是运行时视图，`#[serde(skip_serializing)]` 不写回 config.json；启动时若发现旧配置内嵌 `workflows`，会自动拆分到独立文件并清空字段。工作流保存/删除/复制/接管都会同步写/删对应文件；导出配置时用 `ExportBundle` 把 settings、overrides 与全部工作流打成一个 JSON，导入时先整体替换 `workflows/` 再写 config。

**生产与开发实例共存**：生产数据目录 `~/.daemonMan`，开发构建（未启用 `custom-protocol` feature）使用 `~/.dev-daemonMan`。开发实例首次启动且自身配置不存在时，只读复制生产 `config.json` 与 `workflows/*.json`；单实例插件只在生产构建启用，DaemonMan 自身的开机自启值名在开发实例下为 `DaemonManDev`，避免覆盖生产自启；开发实例还使用独立的 WebView2 用户数据目录（`%LOCALAPPDATA%\com.daemonman.app.dev\EBWebView`），避免与生产实例的 WebView2 profile 互斥。`DAEMONMAN_DATA_DIR` 环境变量可覆盖目录（自动化测试用）。

旧版本目录 `~/.daemon-man` 在首次启动时通过 `config::migrate_legacy_data_dir()` 自动迁移（优先整体 rename，失败则逐项复制配置与日志）。

## 3. 工作流执行语义

1. **校验**（`engine::graph::validate`）：检测重复 id、悬空边、自环、环依赖（Kahn 算法），Application 节点缺 path 视为错误；空图与禁用节点为警告；输出拓扑序。
2. **调度**（`engine::executor`）：
   - 节点入边全部满足后进入 `waiting`，获得并发许可后进入 `running`；
   - 边条件：`success` 要求上游终态为成功，`failure` 要求上游失败/取消，`always` 无条件放行；
   - 无入边节点立即并行启动；并发受 `settings.maxConcurrency` 限制；
   - 节点执行受 `tokio::time::timeout` 包裹（`defaultNodeTimeoutMs`，0 为不限）。
3. **节点实现**：
   - `application`：`skipIfRunning` 检测 → 启动 → `settleMs` → 可选等待窗口 → 可选关闭主窗口/置前；
   - `delay`：休眠 `delayMs`；
   - `wait`：等待进程（可选要求主窗口可见），超时可配置是否视为失败；
   - `script`：`cmd /C <command>`，返回码非 0 可按配置视为失败。
4. **状态与取消**：任何状态变化都 `state.upsert_run` 并 emit `workflow:run`（payload 为完整 `WorkflowRun`）；取消通过 `state.run_flag(run_id)`（`Arc<AtomicBool>`）协作式生效：未执行的节点标记为 cancelled，已启动的进程不强制结束。
5. **失败传播**：失败/取消会让 `success` 边的下游节点变为 `skipped`，但 `failure`/`always` 边继续执行，从而支持降级分支。
6. **单点执行**：`engine::spawn_single_node_run` 校验节点存在且非分组、强制 `enabled=true`，用只含该节点的最小工作流（edges 清空、沿用原 workflow id/name/skip_existing）复用同一执行器，便于调试；不校验整张图，未保存的草稿也能单点运行。节点 id 保持不变，前端按 `run.workflowId + nodeId` 显示状态。
7. **进程父进程**：普通启动走 `CreateProcessW + STARTUPINFOEXW`，用 `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` 把父进程指向当前 shell（`GetShellWindow` 所属 explorer），使工作流启动的进程不挂在 DaemonMan 进程树下；属性列表/句柄失败或 `CREATE_BREAKAWAY_FROM_JOB` 不被允许时按「去 breakaway → 普通 CreateProcess」回退。管理员启动仍由 ShellExecuteEx(runas) 处理。

## 4. 自启动接管

| 来源 | 扫描方式 | 启用/禁用方式 | 权限 |
| --- | --- | --- | --- |
| 注册表 Run / RunOnce | HKCU、HKLM、HKLM WOW6432Node | `Explorer\StartupApproved\Run` 12 字节标记 | HKCU 无需管理员 |
| 启动文件夹 | `%APPDATA%` 与 `%ProgramData%` 的 Startup | `StartupApproved\StartupFolder` | 所有用户项需要管理员 |
| 计划任务 | Task Scheduler COM，登录触发器，排除 `\Microsoft\` | `put_Enabled` / `schtasks /Change` | 系统任务需要管理员 |
| 服务 | SCM 枚举 `SERVICE_AUTO_START` | `ChangeServiceConfigW`（auto/demand） | 需要管理员 |

`AutostartItem.id` 由 `source + scope + locator` 组成，重新扫描保持稳定，命令层据此定位；`overrides` 记录被 DaemonMan 关闭的项，用于「恢复系统自启」和界面上的「已接管」标记。

### 4.1 临时提权

GUI 始终以普通权限运行。需要管理员权限的单个操作（启用/禁用 HKLM 项、服务、系统任务，恢复或删除系统级项）失败时，前端弹确认框，命令层 `elevate_autostart_op` 通过 `ShellExecuteExW(runas)` 启动同一个 exe 的 `--elevated-autostart <request.json> <result.json>` 一次性子进程（无窗口/托盘），子进程执行操作后把结果 JSON 写回并退出，父进程读取结果、失效自启动缓存并刷新界面，自身权限不变。请求只支持 `enable/disable/restore/delete` 四种动作；用户取消 UAC 时错误文案为「已取消管理员授权」。

「接管」流程：批量关闭系统自启（失败项汇总到 `failures`）→ 为目标工作流追加节点（可执行文件存在则用 Application 节点，否则退化为 Script 节点）→ 按顺序串联 success 边 → 标记 `overrides`。整个过程只使用系统官方开关，不删除注册表值或快捷方式。

## 5. 事件与 IPC

- 命令清单见 `src-tauri/src/lib.rs` 的 `invoke_handler`，前端封装在 `src/api/index.ts`。
- 事件：
  - `workflow:run`：`WorkflowRun`，每次节点/运行状态变化推送；
  - `log:entry`：`LogEntry`，日志实时推送；
  - `navigate`：托盘「设置」菜单通知前端跳转；
  - `app:ready`：后端 setup 完成。
- 前端在 `App.vue` 统一订阅事件并写入 Pinia store，页面只消费 store。

## 6. 测试

- Rust：`cargo test --lib` 覆盖工作流图算法、配置容错、平台层纯函数；涉及真实注册表/服务的测试标记 `#[ignore]`。
- 前端：`pnpm typecheck`（vue-tsc strict）与 `pnpm build`；编辑器交互依赖人工验证。

## 7. 工作流编辑器：布局、边样式与历史

`components/workflow/layout.ts` 负责自动排列，`graph.ts` 提供 Kahn 拓扑分层：

- 每个可执行节点计算 `layer`（1 + max(上游层)）与 `拓扑序`（Kahn 出队顺序）。顶层单元按层分列，同层按拓扑序排列；顺序分组内子项按拓扑序排列（延时节点位于其真实前后节点之间），并行分组用三列紧凑网格。
- 节点卡片显示执行步骤号（拓扑序）；分组容器边界由子节点位置递归推导。
- 边分类：`target.layer === source.layer + 1 && condition === "success"` 为执行流（实线、品牌色、粗箭头）；其余（跨层/同层跳线、失败/总是）为依赖线（虚线、灰色、细箭头）。图例见画布工具栏。
- 历史：`stores/workflows.ts` 对 `draft` 做深度 watch + 约 400ms 防抖，把 JSON 快照压入 past/future 栈；上限取 `AppSettings.undo_limit`（默认 100，范围 1..=10000），加载/保存/切换工作流时重置。撤销/重做恢复快照并保持 dirty/选中态一致。
- 新建启动应用：`api.list_start_menu_apps` 扫描用户与系统开始菜单中的 `.lnk`（解析目标与参数、过滤卸载/帮助/文档等噪声），前端 `StartMenuPicker` 搜索/图标/键盘选择后填入节点。
- 应用节点图标复用 `get_file_icons`：画布层收集所有 application 节点的 path 批量调用 `useIconStore().ensure`，卡片渲染 PNG data URL，失败回退 lucide 图标。

## 8. 自启动管理的筛选模型

前端 `components/startup/categories.ts` 负责把 `AutostartItem` 归入「类别」与「位置」两个维度：

- 类别：registry（Run + RunOnce）/ startup-folder / scheduled-task / service；
- 位置：由 source + scope + requiresAdmin 推导（`machine*`、公共启动文件夹、服务、非当前用户身份的计划任务 → 系统级，其余为当前用户）。

两组 Tabs 与搜索、状态筛选是 AND 关系；计数徽标基于其它筛选条件的当前结果动态计算；全选与批量接管只作用于可见行。

### 8.1 图标与右键禁用/删除

- 图标：`platform::icon::file_icon_data_url` 用 `IShellItemImageFactory` 取 shell 图标（`.exe`/`.lnk`/`.bat` 一致），HBITMAP → 32bpp DIB → RGBA → PNG → base64 data URL；`SHGetFileInfoW` 兜底；进程内按（小写路径, 尺寸）缓存。命令层 `get_file_icons` 批量返回，前端 `useIconStore` 分批懒加载。
- 备份：`platform::backup` 维护 `~/.daemonMan/autostart-backup/index.json` 与文件副本/任务 XML，写入走 `config::write_atomic` + 进程内锁。
  - `disable`：`toggle_supported` 的项走各来源的系统开关（mode=native）；其余（如 RunOnce）先备份再移除（mode=removed）。
  - `restore`：无备份记录时回退为扫描后 native 启用；native 重新启用；removed 还原注册表原值（含类型与原始字节）/文件/任务 XML，成功后清理记录。前端会用 `overrides` 里 mode=removed 的记录补一行「已移除」，因此被移除项仍可恢复。
  - `delete`：拒绝 `can_delete=false`（服务），先备份（mode=deleted）再永久删除；备份失败即中止。
- `AutostartItem.can_delete` 由各扫描器填写；`AutostartOverride.mode` 记录 native/removed，用于恢复语义。

## 9. 工作流双视图

`WorkflowsView` 提供两种视图，共享同一 `draft`、保存/校验/运行与实时状态：

- 节点视图：Vue Flow 画布（拖拽、连线、自动排列、属性检查器、运行监控）。分组节点（`kind=group`，`parentId` 指向它）作为容器背景层渲染；顺序分组子项纵向排列，并行分组（`groupParallel=true`）用紧凑三列网格。
- 文本视图：`WorkflowTextView.vue` + `graph.ts` 的 Kahn 分层算法与 parentId 树。分组行显示树线/并行徽标/子项数，子节点缩进；每行保留状态、类型、目标/参数、上游依赖（含条件）。行点击联动右侧属性面板；切换视图保留选中节点。

## 10. 跳过已存在进程

`Workflow.skip_existing_processes`（默认 true）是工作流级开关。执行时对 Application 节点计算
`effective = workflow.skip_existing_processes && node.app.skip_if_running`；若进程已在运行，节点直接置为
`NodeStatus::SkippedExisting`（serde `skipped-existing`，界面 teal 色标记），并在依赖语义上视为成功
（success 边继续放行）。该节点不执行启动，也不写入 pid。

分组节点（`kind=group`）只用于展示：不进入 `run.nodes`、不参与调度与拓扑层级，校验时也不要求 path。


