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

配置文件位置为 `%USERPROFILE%\.daemonMan\config.json`。旧版本目录 `~/.daemon-man` 在首次启动时通过 `config::migrate_legacy_data_dir()` 自动迁移（优先整体 rename，失败则逐项复制配置与日志）。

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

## 4. 自启动接管

| 来源 | 扫描方式 | 启用/禁用方式 | 权限 |
| --- | --- | --- | --- |
| 注册表 Run / RunOnce | HKCU、HKLM、HKLM WOW6432Node | `Explorer\StartupApproved\Run` 12 字节标记 | HKCU 无需管理员 |
| 启动文件夹 | `%APPDATA%` 与 `%ProgramData%` 的 Startup | `StartupApproved\StartupFolder` | 所有用户项需要管理员 |
| 计划任务 | Task Scheduler COM，登录触发器，排除 `\Microsoft\` | `put_Enabled` / `schtasks /Change` | 系统任务需要管理员 |
| 服务 | SCM 枚举 `SERVICE_AUTO_START` | `ChangeServiceConfigW`（auto/demand） | 需要管理员 |

`AutostartItem.id` 由 `source + scope + locator` 组成，重新扫描保持稳定，命令层据此定位；`overrides` 记录被 DaemonMan 关闭的项，用于「恢复系统自启」和界面上的「已接管」标记。

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

## 7. 旧版配置迁移（legacy.rs）

`legacy::convert_file()` 解析 ProgressManager 的 `config.json`（容忍 UTF-8 BOM），递归构建工作流：

- 程序项 → Application 节点；带子项的程序同样先启动自身，再按 `launchOrder` 处理子项；
- ordered：子项之间串联 success 边，`latency` 以 Delay 节点插入；
- parallel：子项之间不建边；
- `waiting` 在第二遍解析：`type | name` 通过注册表（HashMap）找到被依赖项的出口节点集合后连边；
- 入口/出口语义：分组本身无节点，入口是第一个（或全部并行）子项，出口是最后一个（或全部并行）子项；
- 结果用 `engine::validate` 做环检测，失败时写入 warning 而不产出坏配置。

导入入口：`import_legacy_config` IPC（设置页「数据迁移」）与 `--import-legacy <path>` CLI；`import_into_config()` 对同名工作流做替换，保证重复导入幂等。

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

- 节点视图：Vue Flow 画布（拖拽、连线、自动排列、属性检查器、运行监控）。
- 文本视图：`WorkflowTextView.vue` + `graph.ts` 的 Kahn 分层算法。表格展示序号、执行层、类型、名称、目标/参数、上游依赖（含条件）、下游数、运行状态；另有「执行分层」区块。行点击联动右侧属性面板；切换视图保留选中节点。


