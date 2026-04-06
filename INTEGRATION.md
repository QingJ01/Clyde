# Clyde 桌面宠物 — 二次开发交接文档

## 项目概述

Clyde 是一个基于 Tauri 2（Rust + Svelte 5）的桌面宠物应用，通过 SVG 动画展示不同状态。本文档说明如何将其接入外部应用（如语音聊天）作为视觉效果展示。

---

## 架构总览

```
外部应用（语音聊天等）
    │
    │  POST http://127.0.0.1:23333/state
    │  { "state": "thinking", "session_id": "voice-chat-001" }
    │
    ▼
┌─────────────────────────────────────────────┐
│  Clyde HTTP Server (Axum, 127.0.0.1:23333)  │
│  └─ state_machine.rs: 多会话状态优先级管理    │
│     └─ 最高优先级状态 → 发射前端事件           │
├─────────────────────────────────────────────┤
│  Pet Window (Svelte + SVG)                  │
│  └─ 收到 state-change 事件 → 切换 SVG 动画   │
└─────────────────────────────────────────────┘
```

**核心接入方式：向本地 HTTP server POST 状态即可驱动宠物动画，无需修改 Clyde 源码。**

---

## HTTP API 接口

### 健康检查

```
GET http://127.0.0.1:23333/state
→ {"ok": true, "app": "clyde-on-desk"}
```

### 设置状态（核心接口）

```
POST http://127.0.0.1:23333/state
Content-Type: application/json

{
  "state": "thinking",              // 必填 — 动画状态名
  "session_id": "voice-chat-001",   // 可选 — 默认 "default"，建议填自定义 ID
  "event": "UserSpeaking",          // 可选 — 事件名，用于日志
  "agent_id": "my-voice-app",       // 可选 — 默认 "claude-code"
  "source_pid": 12345,              // 可选 — 用于 attention 状态自动聚焦终端
  "cwd": "/path/to/project"         // 可选 — 工作目录
}
```

**最简调用：**
```bash
curl -X POST http://127.0.0.1:23333/state \
  -H "Content-Type: application/json" \
  -d '{"state": "thinking", "session_id": "voice-chat"}'
```

### 结束会话

```json
{
  "state": "idle",
  "session_id": "voice-chat-001",
  "event": "SessionEnd"
}
```

发送 `event: "SessionEnd"` 会完全移除该会话。

---

## 可用状态列表

| 状态名 | 优先级 | 视觉效果 | SVG 文件 | 类型 |
|--------|--------|---------|----------|------|
| `error` | 8 | 感叹号/错误表情 | clyde-error.svg | 一次性 |
| `notification` | 7 | 通知表情 | clyde-notification.svg | 一次性 |
| `sweeping` | 6 | 扫地动画 | clyde-working-sweeping.svg | 一次性 |
| `attention` | 5 | 开心跳跃 | clyde-happy.svg | 一次性 |
| `carrying` | 4 | 搬运动画 | clyde-working-carrying.svg | 一次性 |
| `juggling` | 4 | 杂耍/指挥 | clyde-working-juggling.svg | 持续 |
| `working` | 3 | 打字/建造 | clyde-working-typing.svg | 持续 |
| `thinking` | 2 | 托腮思考 | clyde-working-thinking.svg | 持续 |
| `idle` | 1 | 待机跟随眼球 | clyde-idle-follow.svg | 持续 |
| `sleeping` | 0 | 睡觉 | clyde-sleeping.svg | 持续 |

### 状态类型说明

- **一次性（Oneshot）**：播放动画后自动恢复到之前的状态（如 `attention` 跳一下就回 `idle`）
- **持续（Persistent）**：保持该状态直到收到新的状态更新
- **优先级**：多个会话同时存在时，最高优先级的状态显示

### 语音聊天建议映射

| 语音聊天事件 | 建议状态 |
|-------------|---------|
| 用户开始说话 | `thinking` |
| AI 正在处理/生成回复 | `working` |
| AI 正在说话 | `working` 或 `juggling` |
| AI 回复完成 | `attention` |
| 出错 | `error` |
| 空闲等待 | `idle` |
| 收到新消息 | `notification` |

---

## 会话管理

- 每个 `session_id` 是独立的会话，互不干扰
- 会话 **10 分钟** 无更新自动清除
- `working`/`thinking` 状态 **5 分钟** 无更新自动降级为 `idle`
- 多会话并存时，优先级最高的状态显示
- DND（勿扰）模式下状态更新会被跳过（除 `SessionEnd`）

---

## 端口发现

Clyde 启动后写入运行时配置文件：

```
~/.clyde/runtime.json
→ {"app": "clyde-on-desk", "port": 23333}
```

默认端口 `23333`，如果被占用会尝试 `23334-23339`。外部应用应该：
1. 先读 `~/.clyde/runtime.json` 获取端口
2. 读取失败则尝试 23333-23339 范围
3. 用 `GET /state` 验证是否为 Clyde server

---

## 关键源码位置

| 文件 | 用途 |
|------|------|
| `src-tauri/src/http_server.rs` | HTTP server，所有 API 端点 |
| `src-tauri/src/state_machine.rs` | 多会话状态管理、优先级、SVG 映射 |
| `src-tauri/src/macos_spaces.rs` | macOS 全屏覆盖 + NSPanel 提升 |
| `src-tauri/src/windows.rs` | 多显示器坐标、拖拽范围 |
| `src-tauri/src/lib.rs` | 拖拽逻辑、窗口初始化 |
| `src/windows/pet/App.svelte` | 前端 SVG 渲染、眼球跟随 |
| `src/windows/hit/App.svelte` | 不可见交互层（拖拽/点击） |
| `assets/svg/` | 所有 SVG 动画文件（35 个） |
| `hooks/` | Claude Code hook 脚本（参考实现） |

---

## 二次开发改动记录

### 本次改动（feat/fullscreen-overlay 分支）

1. **全屏覆盖** — 通过 `object_setClass` 将 NSWindow 提升为 NSPanel 子类，启用 `NonactivatingPanel` style mask，配合 `CanJoinAllSpaces + FullScreenAuxiliary + Stationary` collectionBehavior 和 `NSScreenSaverWindowLevel(1000)`

2. **跨显示器拖拽** — 拖拽 clamp 改为所有显示器联合包围盒；Rust 端用 CoreGraphics `CGEvent` 获取全局鼠标坐标，绕过 Tauri 的 DPI 缩放 bug

3. **隐藏 Dock 图标** — `set_activation_policy(Accessory)`

---

## 快速接入示例（Python）

```python
import requests
import json

CLYDE_URL = "http://127.0.0.1:23333/state"
SESSION = "voice-chat-session"

def set_pet_state(state: str):
    """设置宠物状态"""
    requests.post(CLYDE_URL, json={
        "state": state,
        "session_id": SESSION,
        "agent_id": "voice-chat"
    }, timeout=0.5)

# 使用示例
set_pet_state("thinking")   # 用户在说话
set_pet_state("working")    # AI 在处理
set_pet_state("attention")  # 处理完成（跳一下）
set_pet_state("idle")       # 回到待机

# 结束会话
requests.post(CLYDE_URL, json={
    "state": "idle",
    "session_id": SESSION,
    "event": "SessionEnd"
}, timeout=0.5)
```

---

## 快速接入示例（Node.js）

```javascript
const CLYDE_URL = 'http://127.0.0.1:23333/state';
const SESSION = 'voice-chat-session';

async function setPetState(state) {
  await fetch(CLYDE_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      state,
      session_id: SESSION,
      agent_id: 'voice-chat'
    })
  });
}

// 使用
await setPetState('thinking');
await setPetState('working');
await setPetState('attention');
await setPetState('idle');
```

---

## 注意事项

1. **Clyde 必须先启动** — 外部应用调用前确认 `GET /state` 返回 200
2. **超时设短** — POST 请求建议 500ms 超时，避免阻塞主流程
3. **状态不要刷太快** — 建议至少间隔 100ms，否则动画来不及播放
4. **一次性状态自动恢复** — `attention`/`error` 等播完会自动回到之前状态，不需要手动切回
5. **session_id 要唯一** — 避免和 Claude Code 的会话冲突，建议用自己的前缀如 `voice-chat-xxx`
