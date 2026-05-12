# Verdaccio Cache Manager - 设计文档

## 概述

一个 Tauri 桌面应用，用于将 npm 包缓存到私有 Verdaccio 实例。支持从公网搜索拉取、从 package.json/lockfile 批量导入、从本地 node_modules 上传，覆盖在线批量预热和离线手动补版本两种场景。

## 技术栈

| 层 | 选型 |
|----|------|
| 桌面框架 | Tauri v2 |
| 前端 | React + TypeScript + Vite |
| UI 组件 | shadcn/ui + Tailwind CSS |
| 状态管理 | zustand |
| 后端 | Rust (tokio 异步运行时) |

## 架构

混合模式：轻量操作同步 Command，重操作异步任务引擎 + Event 推送。

```
轻量操作: React → invoke Command → Rust 同步处理 → 返回结果
重操作:   React → invoke Command(启动任务) → Rust task_engine 异步执行
                ← emit Event ← 实时进度推送
```

### Rust 模块划分

| 模块 | 职责 |
|------|------|
| `registry_client` | 封装 npm registry + Verdaccio HTTP API（搜索、下载 tarball、PUT publish） |
| `parser` | 解析 package.json / pnpm-lock.yaml / package-lock.json，提取包名+版本 |
| `task_engine` | 异步任务队列，管理并发、重试、进度上报（通过 Tauri Event） |
| `local_scanner` | 扫描 node_modules 目录结构，解析本地 .tgz 文件元信息 |
| `config` | 配置持久化（Verdaccio 地址、并发数、超时等） |

### Rust 依赖

| 用途 | crate |
|------|-------|
| HTTP 客户端 | `reqwest` |
| 异步运行时 | `tokio` |
| YAML 解析 | `serde_yaml` |
| JSON 解析 | `serde_json` |
| Tarball 操作 | `flate2` + `tar` |
| 序列化 | `serde` |
| 并发控制 | `tokio::sync::Semaphore` |

## 功能详细设计

### 1. 搜索与缓存

- 搜索源可切换：npmjs.com（拉新包）/ Verdaccio（查看已缓存）
- 搜索结果展示包名、描述、版本列表
- 搜索 Verdaccio 时标注已缓存版本，方便发现缺失
- 用户勾选包+版本后加入任务队列
- task_engine 执行：从 npmjs 下载 tarball → PUT 到 Verdaccio

### 2. package.json 导入

- 支持拖入或选择 package.json 文件
- 解析 dependencies + devDependencies
- 可选开启"包含传递依赖"：自动查找同目录下的 lockfile 并解析
- 展示包列表，支持勾选/全选后批量缓存

### 3. Lockfile 解析

支持两种格式：

- **pnpm-lock.yaml**：解析 `packages` 字段，提取 name@version + tarball URL
- **package-lock.json**：解析 `packages` 字段（v2/v3 格式），提取 `resolved` URL

解析后展示扁平依赖列表，与 Verdaccio 已有版本对比，只缓存缺失部分。

### 4. 本地上传

**扫描 node_modules：**

1. 用户选择项目目录
2. 递归扫描 node_modules，读取每个包的 package.json 获取 name + version
3. 与 Verdaccio 已有版本对比，展示列表
4. 用户勾选后，Rust 自行打 tarball（不依赖 npm CLI）：
   - 根据 `files` 字段或默认规则收集文件
   - 用 `tar` + `flate2` 打包（顶层目录 `package/`）
   - PUT 到 Verdaccio

**拖入 .tgz：**

1. 用户拖入一个或多个 .tgz 文件
2. 解析 tarball 内 package.json 获取元信息
3. 确认后直接 PUT 到 Verdaccio

### 5. 任务引擎

**配置项：**

| 配置 | 默认值 | 说明 |
|------|--------|------|
| 并发数 | 5 | 同时执行的下载+上传任务数 |
| 重试次数 | 3 | 单个包失败后自动重试 |
| 超时 | 60s | 单个包的下载/上传超时 |

**包状态机：** `等待 → 下载中 → 上传中 → 成功 / 失败`

**进度上报：** 通过 Tauri Event 实时推送每个包的状态变化到前端。

**失败处理：** 失败的包可在 UI 上单独重试或批量重试。

### 6. Verdaccio Publish 协议

使用 npm registry 标准 publish API（Verdaccio 完全兼容）：

```
PUT /{package_name}
Content-Type: application/json

{
  "name": "包名",
  "versions": { "x.y.z": { ... package.json 内容 } },
  "_attachments": {
    "包名-x.y.z.tgz": {
      "content_type": "application/octet-stream",
      "data": "<base64 encoded tarball>"
    }
  }
}
```

## 前端设计

### 页面结构

侧边栏导航 + 右侧内容区 + 底部常驻任务状态栏。

```
┌──────┬──────────────────────────────────┐
│      │                                  │
│ 搜索 │         内容区                    │
│ 导入 │                                  │
│ 上传 │                                  │
│ 设置 │                                  │
│      │                                  │
├──────┴──────────────────────────────────┤
│  任务栏: 3/12 完成  2 失败  [查看详情]    │
└─────────────────────────────────────────┘
```

### 页面职责

**搜索页：**
- 搜索框 + 源切换（npmjs / Verdaccio）
- 结果列表：包名、描述、最新版本
- 展开包 → 所有版本，标注已缓存状态
- 勾选后"缓存到私服"

**导入页：**
- 拖入/选择文件区域（package.json / lockfile）
- 解析后表格展示（包名、版本/范围、是否已缓存）
- toggle："包含传递依赖（从 lockfile）"
- 全选/反选 → "开始缓存"

**上传页：**
- Tab 1 "扫描 node_modules"：选择目录 → 扫描结果 → 勾选上传
- Tab 2 "上传 .tgz"：拖入区域，支持多文件

**设置页：**
- Verdaccio 地址 + 连接测试
- 并发数、超时、重试次数

**任务详情（底部栏展开）：**
- 表格：包名、版本、状态（颜色标识）、耗时
- 筛选：全部 / 进行中 / 成功 / 失败
- 操作：重试失败项、清空已完成、取消进行中

### 前端状态管理

- zustand 管理全局状态（任务列表、配置、搜索结果）
- Tauri Event 监听封装为 `useTauriEvent` hook
- 任务状态实时更新，无需轮询

## 配置

单 Verdaccio 实例，无认证。配置持久化为 JSON 文件。

**存储位置（Tauri app_data_dir）：**
- macOS: `~/Library/Application Support/com.verdaccio-cache-manager/config.json`
- Windows: `%APPDATA%/com.verdaccio-cache-manager/config.json`
- Linux: `~/.config/com.verdaccio-cache-manager/config.json`

**配置结构：**

```json
{
  "registry_url": "http://localhost:4873",
  "concurrency": 5,
  "retry_count": 3,
  "timeout_secs": 60
}
```

## 约束与边界

- 不支持认证（内网私服无需登录）
- 单 Verdaccio 实例（不做多实例切换）
- Lockfile 只支持 pnpm-lock.yaml 和 package-lock.json（不支持 yarn.lock）
- 从 node_modules 打包时遵循 npm 的 files 字段规则；无 files 字段时打包整个包目录（排除 node_modules、.git、测试目录），不处理 .npmignore（简化实现）
- 不做版本冲突解决——如果 Verdaccio 已有该版本，跳过并标记为"已存在"
