# DaemonMan

用**工作流**编排软件启动顺序的 Windows 启动管线管理器，同时能接管系统自启动项。

DaemonMan 是旧 PowerShell 启动器（`ProgressManager`）的完全重写版本：后端 Rust（Tauri 2），前端 Vue 3 + TypeScript + Tailwind CSS 4。它把「开机要启动的一堆软件」抽象成一张有向无环图（DAG）：节点是启动应用 / 延时 / 等待进程 / 执行命令，连线表达依赖与条件；登录时按图执行，并且可以把 Windows 里散落的自启动项关掉、收编进同一条管线。

## 功能

- **可视化工作流编辑**：Vue Flow 画布，拖拽节点、连线定义依赖；节点支持启动参数、工作目录、环境变量、跳过已运行、等待主窗口、关闭启动画面、窗口置前、管理员启动等选项。
- **双视图**：同一份工作流可在「节点视图」（图编辑）与「文本视图」（高信息密度清单：执行分层、上游依赖、目标命令、运行状态）之间切换。
- **登录启动管线**：标记「登录时自动运行」的工作流会在 DaemonMan 随系统启动后依次执行；支持启动缓冲时间、并发上限、节点超时。
- **自启动项扫描与接管**：扫描注册表 `Run/RunOnce`（HKCU/HKLM/WOW6432Node）、启动文件夹、登录触发的计划任务、自动启动的 Windows 服务；列表显示程序图标，支持按类别/位置 Tabs 筛选；可单选/切换，也可批量「接管」——关闭系统自启并把它们加入工作流，需要时还能一键恢复系统自启。
- **右键禁用/删除**：支持系统开关的项直接禁用；不支持的项（如 RunOnce）会先把原始数据备份到 DaemonMan 配置目录再从原位置移除，可随时恢复；删除同样先备份再永久删除（服务不支持删除）。
- **按图调度**：无入边节点并行启动，依赖边支持「成功后 / 失败后 / 总是」三种条件，失败可沿 failure 边做降级分支；运行状态实时推送到界面。
- **进程与日志**：查看由 DaemonMan 启动的进程（PID/内存/CPU/运行时/所属工作流）并可结束；结构化日志支持级别筛选、实时滚动、导出。
- **托盘常驻**：关闭窗口可隐藏到托盘；托盘菜单可打开主界面、运行全部启用工作流、退出。
- **明暗主题 + 中英双语**，配置与日志持久化到 `~/.daemonMan/`。

## 技术栈

| 层 | 选型 |
| --- | --- |
| 桌面框架 | Tauri 2（Rust） |
| 前端 | Vue 3 + TypeScript + Vite 6 + Pinia + Vue Router |
| 样式 | Tailwind CSS 4（纯 class，无独立样式文件） |
| 图编辑 | @vue-flow/core |
| Windows 集成 | winreg、windows（Win32 API）、sysinfo |

## 开发

```powershell
# 安装依赖
pnpm install

# 开发模式（前端 HMR + Rust 后端）
pnpm tauri dev

# 仅前端类型检查 / 构建
pnpm typecheck
pnpm build

# 生产构建（生成 NSIS 安装包）
pnpm tauri build

# 后端检查
cd src-tauri
cargo check
cargo clippy --all-targets
cargo test --lib

# 手动打包（等价于 pnpm tauri build，必须启用 custom-protocol 才会内嵌前端资源）
cargo build --release --features custom-protocol --bin daemon-man
```

> 注意：Tauri 2 的 dev/生产模式由 `custom-protocol` feature 决定，而不是 Cargo profile。
> 不带该 feature 的构建（包括 `cargo build --release`）会加载 `devUrl`，需要同时运行
> `pnpm dev`；`pnpm tauri build` 会自动启用该 feature。

环境要求：Rust stable（MSVC toolchain）、Node.js 18+、pnpm、Windows 10/11、WebView2 Runtime。

## 验证与自检

```powershell
# 无窗口后端自检：配置路径、权限、自启动项分布、进程数（退出码 0 = 通过）
.\src-tauri\target\debug\daemon-man.exe --selftest

# 后端单元测试（含平台层真实环境的 #[ignore] 冒烟测试）
cd src-tauri
cargo test --lib
cargo test --lib scan_smoke -- --ignored --nocapture
```

GUI 冒烟脚本通过 WebView2 远程调试协议驱动真实窗口（截图 + 前端错误采集）：

```powershell
pnpm dev                                  # 终端 A
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=19222"
.\src-tauri\target\debug\daemon-man.exe    # 终端 B

node scripts/ui-smoke.mjs       # 逐页截图到 .develop/verify/
node scripts/e2e-workflow.mjs   # 新建工作流→加节点→保存→运行→校验
```

## 目录结构

```
daemon-man/
├── src/                       # Vue 前端
│   ├── api/                   # IPC 类型与 invoke 封装（与 src-tauri/src/model.rs 对齐）
│   ├── modules/
│   │   ├── components/        # layout / ui / workflow / dashboard / startup / settings
│   │   ├── stores/            # Pinia：app / workflows / autostart / runtime / logs / theme / ui
│   │   ├── views/             # 概览、工作流、自启动、进程、日志、设置
│   │   ├── locales/           # 轻量 i18n（zh-CN / en）
│   │   ├── router/
│   │   └── utils/
│   └── assets/styles/main.css # Tailwind 入口 + 第三方样式适配
└── src-tauri/                 # Rust 后端
    └── src/
        ├── model.rs           # 配置 / 工作流 / 运行状态 / 自启动项数据模型
        ├── config.rs          # ~/.daemonMan/config.json 原子读写
        ├── state.rs           # 共享状态（配置、运行记录、日志、取消标记、扫描缓存）
        ├── platform/          # Windows 平台层：注册表、启动文件夹、计划任务、服务、进程/窗口
        ├── engine/            # DAG 校验、调度执行、应用启动器
        ├── commands/          # Tauri IPC 命令（config/workflow/autostart/process/system）
        ├── tray.rs            # 系统托盘
        └── lib.rs             # 应用装配、登录启动、关闭到托盘
```

## 数据与配置

- 配置：`%USERPROFILE%\.daemonMan\config.json`（原子写入，保留 `.bak`）
- 日志：`%USERPROFILE%\.daemonMan\logs\daemon-man.log`
- 自启动项备份：`%USERPROFILE%\.daemonMan\autostart-backup\`（`index.json` + 文件副本/计划任务 XML；右键禁用或删除时写入，恢复后自动清理）
- 开机自启：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `DaemonMan` 值（`--autostart` 参数启动，默认隐藏窗口并延迟执行工作流）
- 旧版本使用的 `~/.daemon-man` 目录会在首次启动时自动迁移到 `~/.daemonMan`（配置与日志）。
- 自启动项开关不删除用户数据：注册表 Run 项通过 `Explorer\StartupApproved\Run` 的启用/禁用标记控制，启动文件夹项通过 `StartupApproved\StartupFolder` 控制，与任务管理器行为一致，可无损恢复。

## 迁移旧版 ProgressManager 配置

旧的 `ProgressManager\config.json`（嵌套分组 + `waiting` 依赖 + `launchOrder`/`latency` + `afterStartup.sendCloseWindow`）可以直接转换为 DaemonMan 工作流：

```powershell
# 方式一：设置 → 数据迁移 → 从 ProgressManager 导入（选择 config.json）
# 方式二：命令行（无窗口，适合脚本化）
.\src-tauri\target\release\daemon-man.exe --import-legacy "C:\path\to\ProgressManager\config.json"
```

转换规则：

| 旧配置 | DaemonMan 工作流 |
| --- | --- |
| `type: "program"` | Application 节点（`skipIfRunning=true`） |
| 分组（含带子项的 program） | 不生成节点，只保留结构 |
| `options.launchOrder: "ordered"` | 子节点之间用 success 边串联 |
| `options.launchOrder: "parallel"` | 子节点之间不建边（并行启动） |
| `options.latency` | 在顺序子项之间插入 Delay 节点 |
| `waiting: ["group | X"]` / `["program | Y"]` | 从被依赖项的「出口节点」连边到目标节点 |
| `options.afterStartup.sendCloseWindow` | Application 节点的「启动后关闭主窗口」 |

迁移生成的工作流名为 `ProgressManager 迁移`，默认标记为「登录时自动运行」；重复导入同名工作流会替换而不是重复创建。转换过程会保留原始路径（含 `.lnk`），遗留的相对路径/裸命令会在节点备注中提示补全。

## 自启动管理

列表支持两组 Tabs 组合筛选：

- **按类别**：全部 / 注册表 / 启动文件夹 / 计划任务 / 系统服务（注册表 Run 与 RunOnce 归并）
- **按位置**：全部位置 / 当前用户 / 系统级（`HKCU`、用户启动文件夹、按当前用户身份运行的计划任务 vs `HKLM`、公共启动文件夹、系统服务）

与搜索、状态筛选（启用中 / 已停用 / 已接管）全部 AND；全选与批量接管只作用于当前可见行。列表会异步加载程序图标（shell 图标 → PNG，失败时回退为按来源的默认图标）。

**右键菜单**：

| 操作 | 行为 |
| --- | --- |
| 禁用 | 支持系统开关的类型（Run / 启动文件夹 / 计划任务 / 服务）直接使用系统禁用；不支持的类型（如 RunOnce）先把原始注册表值/文件备份到 `autostart-backup/`，再从原位置移除 |
| 恢复 | 系统开关禁用的项重新启用；被移除的项会以「已移除」行保留在列表中（不在系统扫描结果里，但有备份记录），可从右键菜单或行内按钮恢复，索引记录自动清理 |
| 删除 | 先备份（注册表值、文件副本或计划任务 XML），再从原位置永久删除；Windows 服务不支持删除（请使用禁用） |

禁用/删除前均需确认；服务与系统级项在普通权限下会返回「需要管理员权限」的明确错误。

## 权限说明

普通权限即可管理当前用户的 `Run`、启动文件夹与用户级计划任务；以下操作需要以管理员身份运行 DaemonMan：

- 开关 `HKLM`（所有用户）自启动项
- 开关系统服务（把启动类型在自动/手动之间切换）
- 开关部分系统级计划任务

界面会标注「需要管理员」并在侧边栏/设置页提示当前权限。

## 与旧版 ProgressManager 的差异

| 能力 | 旧版（PowerShell） | DaemonMan |
| --- | --- | --- |
| 编排方式 | 嵌套 JSON 分组 + `waiting` 依赖 | 任意 DAG，可视化编辑 |
| 依赖条件 | 仅「等某个分组启动完成」 | 成功后 / 失败后 / 总是 |
| 启动选项 | 关闭窗口、并行/顺序、间隔 | 参数/工作目录/环境变量/等待窗口/跳过已运行/置前/管理员等 |
| 自启动项 | 无 | 扫描 + 切换 + 接管 + 恢复 |
| 状态可见性 | 终端 ANSI 刷新 | 桌面 GUI 实时状态 + 运行历史 + 日志 |
| 运行方式 | 登录脚本 | 托盘常驻 + 登录启动管线 |

## 已知限制

- 目前仅实现 Windows 平台（`platform` 层为 Windows 专用；跨平台需补 macOS/Linux 实现）。
- UWP / Store 应用的自启动任务（StartupTask）暂未纳入扫描。
- 服务与系统级任务受权限限制，普通权限下会提示需要管理员。
- 打包产物默认 NSIS；如需 MSI 可在 `tauri.conf.json` 的 `bundle.targets` 中调整。

