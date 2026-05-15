<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" alt="Verdaccio Cache Manager" width="128" />
</p>

<h1 align="center">Verdaccio Cache Manager</h1>

<p align="center">
  <img alt="version" src="https://img.shields.io/badge/version-0.1.22-blue" />
  <img alt="platform" src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey" />
  <img alt="license" src="https://img.shields.io/badge/license-MIT-green" />
</p>

一个用于把 npm 包预热到私有 [Verdaccio](https://verdaccio.org/) 的桌面应用。覆盖三种场景：

- **在线搜索** — 从公网 npmjs 搜包、勾选版本批量推送到私服。
- **依赖文件导入** — 拖入 `package.json` / `pnpm-lock.yaml` / `package-lock.json`，自动解析依赖并递归缓存。
- **本地上传** — 扫描某项目的 `node_modules`，或直接拖入 `.tgz` tarball 推送到私服，支持导出 tarball 到目录，适合离线/内网补包。

技术栈：Tauri v2 · React 19 · TypeScript · Rust（tokio）· shadcn/ui · Tailwind CSS · zustand。

---

## 截图

### 搜索与缓存

在 **npmjs** 标签下搜公网包，按勾选的版本批量推送；切到 **Verdaccio** 标签可查看私服已缓存的包并继续操作。

| npmjs 多版本勾选 | Verdaccio 已缓存包 |
|---|---|
| ![](docs/screenshots/search-npmjs-versions.png) | ![](docs/screenshots/search-verdaccio-cached.png) |

### 依赖文件导入

拖入 `package.json` / `pnpm-lock.yaml` / `package-lock.json`，自动解析依赖并标记哪些版本尚未缓存。

| 空状态 | 解析后的依赖列表 |
|---|---|
| ![](docs/screenshots/import-empty.png) | ![](docs/screenshots/import-package-json.png) |

### 本地上传

选择本地项目目录，扫描 `node_modules` 并对比已缓存索引；也支持单独上传 `.tgz`。

| 空状态 | 扫描结果（已缓存 / 未缓存） |
|---|---|
| ![](docs/screenshots/local-upload-empty.png) | ![](docs/screenshots/local-upload-scan.png) |

### 设置

配置 Verdaccio 地址、可选 storage 路径、并发数、重试与超时；同步本地缓存索引。

![](docs/screenshots/settings.png)

## 安装

从 [Releases](../../releases) 下载对应平台的安装包：

| 平台 | 文件 |
|------|------|
| macOS (Apple Silicon) | `*_aarch64.dmg` |
| macOS (Intel) | `*_x64.dmg` |
| Windows | `*_x64-setup.exe` / `*_x64_en-US.msi` |
| Windows（内网 / 旧 WebView2） | `*_offline-webview2-setup.exe` / `*_offline-webview2.msi` |
| Linux | `*_amd64.deb` / `*_amd64.AppImage` |

> 应用未签名，首次打开 macOS 可能需要在「系统设置 → 隐私与安全性」放行。
> `offline-webview2` Windows 包会内置 WebView2 离线安装器，安装包更大，适合不能联网或系统 WebView2 版本过旧的机器；普通 Windows 包保持原来的较小体积。

## 近期更新

**v0.1.12 / v0.1.11**
- 全新应用图标：macOS squircle 风格，深森林绿配等距 3D 包裹盒，呼应 Verdaccio 品牌

**v0.1.10**
- 并发上传 npm 包时不再每包重复登录 Verdaccio，整个扫描任务只触发 1 次登录请求，消除大量 409 日志和密码验证开销

**v0.1.8**
- 扫描 node_modules 后自动比对已缓存包，默认仅勾选未缓存项
- 扫描页上传时显示按钮 loading 与逐包状态徽章
- Publish 413 错误给出明确修复提示（调大 Verdaccio `max_body_size`）

**v0.1.5**
- 新增导出 tarball 到目录功能
- 支持 proxy 关闭时从本地 node_modules 上传
- 本地 SQLite 缓存索引，增量同步已缓存包列表
- 依赖解析支持递归缓存
- 设置页 UI 重构

完整变更见 [CHANGELOG.md](CHANGELOG.md)。

## 快速开始

1. 启动应用。
2. 打开「设置」，填写 Verdaccio 地址（例：`http://localhost:4873`），点「测试连接」。
3. 进入「搜索 / 导入 / 本地上传」任一页面，选包后点「上传/缓存到私服」。
4. 任务进度在底部任务栏实时显示，失败可重试。

## 配套插件：`verdaccio-cached-list`（可选）

Verdaccio 内置 `/-/verdaccio/data/packages` 只能列出本地 **publish** 的包，**proxy 缓存**的包不在结果里。

仓库内 [`verdaccio-cached-list/`](verdaccio-cached-list) 是一个 Verdaccio 中间件插件，扫描存储目录暴露 `GET /-/cached-packages`，让"搜索 Verdaccio 已缓存"功能可以看到 proxy-cached 的包。

```bash
# 在 Verdaccio 容器内安装
npm install -g verdaccio-cached-list
```

```yaml
# config.yaml
middlewares:
  cached-list: {}
```

详见插件 README：[`verdaccio-cached-list/README.md`](verdaccio-cached-list/README.md)。

## 开发

需要 Rust toolchain（stable）、Node 22+、pnpm 10+。Linux 还需 webkit2gtk 等依赖，参考 [Tauri 前置依赖](https://tauri.app/start/prerequisites/)。

```bash
pnpm install
pnpm tauri dev    # 启动开发模式
pnpm tauri build  # 打包当前平台
```

仅前端调试：

```bash
pnpm dev          # vite dev server (无 tauri shell)
pnpm tsc --noEmit # 类型检查
```

## 架构

```
轻量操作: React → invoke Command → Rust 同步处理 → 返回结果
重操作:   React → invoke Command(启动任务) → Rust task_engine 异步执行
                ← emit Event ← 实时进度推送
```

Rust 模块（`src-tauri/src/`）：

| 模块 | 职责 |
|------|------|
| `registry_client` | npm registry / Verdaccio HTTP API（搜索、下载、PUT publish），共享登录 token |
| `parser` | 解析 `package.json` / `pnpm-lock.yaml` / `package-lock.json` |
| `dependency_resolver` | 递归解析依赖树，根据 semver 选定具体版本 |
| `task_engine` | 异步任务队列，并发 + 重试 + 进度上报 |
| `local_scanner` | 扫描 `node_modules`、解析 `.tgz` |
| `storage_scanner` | 从 Verdaccio storage 读已缓存包 |
| `cache_db` | SQLite 本地索引，缓存包元数据与版本列表 |
| `sync_engine` | 与 Verdaccio 的增量同步（首次全量，之后增量） |
| `config` | 配置持久化 |

完整设计：[`2026-05-12-verdaccio-cache-manager-design.md`](2026-05-12-verdaccio-cache-manager-design.md)。

## 发布流程

推送 `v*` tag 即触发 GitHub Actions 多平台构建并起草 Release：

```bash
# 1. 同步升级四处版本号
#    - package.json
#    - src-tauri/tauri.conf.json
#    - src-tauri/Cargo.toml
#    - README.md  (version badge)
cargo update -p verdaccio-cache-manager  # 刷新 Cargo.lock

# 2. 提交版本变更
git commit -m "chore: bump version to <version>"

# 3. 基于 git log <prev-tag>..HEAD 生成 CHANGELOG.md 条目并提交
git commit -m "docs: update changelog for v<version>"

# 4. 先推 main，确认与远程同步后再打 tag
git push origin main
git tag v<version>
git push origin v<version>
```

> ⚠️ **tag 必须打在 `origin/main` HEAD 上**。如果打 tag 后再向 main 推 commit，重跑 release workflow 会因 GitHub API 限制返回 403（`GITHUB_TOKEN` 无法对非默认分支 HEAD 创建 release）；如需追加变更，请走下一个 patch 版本。

完整发布约束详见 [CLAUDE.md](CLAUDE.md)。工作流定义：[`.github/workflows/release.yml`](.github/workflows/release.yml)。

## License

MIT
