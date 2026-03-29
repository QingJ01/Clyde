<p align="center">
  <img src="assets/tray-icon.png" width="128" alt="Clyde">
</p>
<h1 align="center">Clyde 桌宠</h1>
<p align="center">
  实时映射 AI 编程助手工作状态的桌面宠物
  <br>
  <a href="README.md">English</a>
</p>
<p align="center">
  <img src="https://img.shields.io/badge/v0.1.0-blue" alt="version">
  <img src="https://img.shields.io/badge/Tauri_v2-orange" alt="Tauri v2">
  <img src="https://img.shields.io/badge/Svelte_5-red" alt="Svelte 5">
  <img src="https://img.shields.io/badge/Rust-black" alt="Rust">
  <img src="https://img.shields.io/badge/Windows%20%7C%20macOS%20%7C%20Linux-grey" alt="platforms">
</p>

Clyde 是一只住在桌面上的宠物，能实时感知 AI 编程助手在做什么：提问时思考，跑工具时打字，子代理工作时杂耍，弹卡片审批权限，任务完成时庆祝，你离开时睡觉。

支持 **Claude Code**、**Codex CLI** 和 **Copilot CLI**，三者可同时运行。

## 快速开始

```bash
git clone https://github.com/QingJ01/Clyde.git
cd Clyde
npm install
npm start        # Tauri 开发模式，前端热更新
```

**前置条件** — [Node.js](https://nodejs.org/) v18+、[Rust](https://rustup.rs/) stable、平台相关 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)。

**Agent 配置** — 全部零配置：
- **Claude Code** — 启动时自动注册 command hook + HTTP 权限 hook
- **Codex CLI** — 自动轮询 `~/.codex/sessions/` 日志
- **Copilot CLI** — 检测到 `~/.copilot` 时自动配置

## 功能

### 动画

12 种动画状态，由实时 Agent 事件驱动：

| Agent 事件 | Clyde 的反应 | SVG |
|---|---|---|
| 空闲 | 眼球跟随鼠标，身体微倾 | `clyde-idle-follow` |
| 提交提示词 | 思考 | `clyde-working-thinking` |
| 工具执行中 | 打字 | `clyde-working-typing` |
| 3+ 个会话活跃 | 建造 | `clyde-working-building` |
| 1 个子代理 | 杂耍 | `clyde-working-juggling` |
| 2+ 个子代理 | 指挥 | `clyde-working-conducting` |
| 工具执行失败 | 报错闪烁 | `clyde-error` |
| 任务完成 | 开心弹跳 | `clyde-happy` |
| 通知 | 惊跳 | `clyde-notification` |
| 上下文压缩 | 扫地 | `clyde-working-sweeping` |
| 创建 Worktree | 搬箱子 | `clyde-working-carrying` |
| 60 秒无活动 | 打哈欠 → 打盹 → 倒下 → 睡觉 | `clyde-sleeping` |

### 交互

- **拖拽** — 任何状态下都能拖动，Pointer Capture 防止快甩丢失
- **双击** 戳一下，**连点 4 下** 东张西望
- **右键菜单** — 会话列表、免打扰、极简模式、大小、语言
- **系统托盘** — 调大小 (S/M/L)、免打扰、极简模式、语言、自动启动、退出

### 极简模式

拖到左/右屏幕边缘（或右键"极简模式"），Clyde 藏到边缘只露半身，悬停时探头，收起状态下仍能显示迷你通知和庆祝动画。

### 权限审批气泡

Claude Code 请求工具权限时，Clyde 弹出浮动卡片 — 允许、拒绝或选择建议规则（如"始终允许 Read"）。多个请求向上堆叠。如果你先在终端回答了，气泡自动消失。

### 会话智能

- **多会话优先级** — 所有会话中最高优先级的状态胜出
- **子代理感知** — 1 个子代理杂耍，2 个以上指挥
- **终端聚焦** — 右键某个会话可直接跳转到对应终端
- **自动清理** — 10 分钟无更新删除会话，5 分钟无更新降级工作状态
- **免打扰** — 静默所有事件，右键或托盘切换

## 项目结构

```
src-tauri/src/           Rust 后端
├── lib.rs               应用入口 + Tauri 命令
├── state_machine.rs     多会话状态追踪 + 优先级
├── http_server.rs       Axum HTTP (POST /state, /permission)
├── hooks.rs             Hook 部署 + settings.json 注册
├── permission.rs        权限气泡窗口
├── mini.rs              边缘吸附、探头、抛物线跳跃
├── tick.rs              50ms 光标轮询（眼球、睡眠、探头）
├── tray.rs              系统托盘菜单
├── windows.rs           窗口边界 + 命中测试
├── focus.rs             按 PID 聚焦终端 (Win/Mac/Linux)
├── codex_monitor.rs     Codex JSONL 日志轮询
├── prefs.rs             偏好持久化
└── i18n.rs              中英文字符串

src/windows/             Svelte 5 前端（3 个窗口）
├── pet/                 SVG 渲染器
├── hit/                 不可见点击层
└── bubble/              权限审批卡片

hooks/                   JS hook 脚本（编译时嵌入）
├── clyde-hook.js        Claude Code 命令 hook
├── server-config.js     端口发现
├── auto-start.js        SessionStart 自动拉起
├── copilot-hook.js      Copilot CLI hook
└── install.js           手动 hook 注册 CLI

assets/svg/              35 个动画帧
```

## 技术栈

| 层 | 技术 | 为什么选它 |
|---|---|---|
| **桌面框架** | [Tauri v2](https://v2.tauri.app/) | 打包体积 ~5 MB（Electron 动辄 150 MB+）；原生系统 API（透明窗口、托盘、全局快捷键）；Rust 后端直接调用，零 IPC 序列化开销 |
| **后端语言** | [Rust](https://www.rust-lang.org/) | 无 GC、零成本抽象；50 ms 定时器 + 多会话状态机跑在单进程里，CPU 占用趋近于零；`Mutex` + `Arc` 天然线程安全 |
| **前端框架** | [Svelte 5](https://svelte.dev/) | 编译时生成极小运行时（无虚拟 DOM），三个窗口 JS 合计 < 30 KB；`$state` / `$props` 响应式模型让 SVG 渲染逻辑极简 |
| **HTTP 服务** | [Axum](https://github.com/tokio-rs/axum) | 构建在 Tokio 上的异步 Web 框架；类型安全路由 + 提取器；与 Tauri 共享同一 Tokio 运行时，无额外线程池 |
| **构建工具** | [Vite](https://vitejs.dev/) | 开发时毫秒级热更新；生产构建 Tree-shaking 极致精简 |

**组合优势：** Rust 处理所有状态逻辑和系统交互，Svelte 只做最薄的渲染层，Tauri 把两者粘合成一个 < 10 MB 的跨平台桌面应用。整个架构没有任何运行时解释器（Node.js、Python 等），冷启动 < 1 秒，常驻内存 < 30 MB。

## 已知限制

| 限制 | 说明 |
|---|---|
| Codex: 无终端聚焦 | JSONL 轮询不携带终端 PID |
| Copilot: 无权限气泡 | Copilot hook 协议仅支持拒绝 |
| HTTP 服务无认证 | 仅绑定 `127.0.0.1`；计划添加 token 认证 |
| 无自动更新 | 请从 GitHub Releases 下载新版本 |

## 贡献

欢迎 Issue、建议和 PR — [提交 Issue](https://github.com/QingJ01/Clyde/issues) 或直接提 PR。

```bash
npm test             # cargo test（19 个单元测试）
```

### 贡献者

<table>
  <tr>
    <td align="center"><a href="https://github.com/QingJ01"><img src="https://github.com/QingJ01.png" width="50" style="border-radius:50%" /><br /><sub>QingJ01</sub></a></td>
    <td align="center"><a href="https://github.com/rullerzhou-afk"><img src="https://github.com/rullerzhou-afk.png" width="50" style="border-radius:50%" /><br /><sub>rullerzhou-afk</sub></a></td>
  </tr>
</table>


## 致谢

- 由 [Clawd on Desk](https://github.com/rullerzhou-afk/clawd-on-desk) ([@rullerzhou-afk](https://github.com/rullerzhou-afk)) 演化而来 — 最初的 Clawd 桌宠项目
- Clyde 像素风格参考自 [clyde-tank](https://github.com/marciogranzotto/clyde-tank) by [@marciogranzotto](https://github.com/marciogranzotto)
- Clyde 角色（"ClawdWizard"）为社区创作。本项目非 [Anthropic](https://www.anthropic.com) 官方产品。

## 许可证

[AGPL-3.0](LICENSE)
