# CLAUDE.md

## 项目概述

Verdaccio Cache Manager — 桌面应用，用于把 npm 包预热到私有 Verdaccio 仓库。

技术栈：Tauri v2 · React 19 · TypeScript · Rust · shadcn/ui · Tailwind CSS · zustand

## 开发命令

```bash
pnpm install          # 安装依赖
pnpm tauri dev        # 启动开发模式
pnpm tauri build      # 打包当前平台
pnpm dev              # 仅前端 vite dev server
pnpm tsc --noEmit     # 类型检查
```

## 发布规则

发布新版本时必须严格按以下步骤执行：

### 1. 升级版本号

同步修改以下四处版本号，保持一致：

- `package.json` → `"version"`
- `src-tauri/tauri.conf.json` → `"version"`
- `src-tauri/Cargo.toml` → `version`
- `README.md` → version badge

运行 `cargo update -p verdaccio-cache-manager` 或让 Cargo.lock 自动更新。

### 2. 提交版本变更

```bash
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock README.md
git commit -m "chore: bump version to <version>"
```

### 3. 生成更新日志

基于上一个 tag 到当前 HEAD 的 commit 生成 CHANGELOG 条目：

```bash
git log <prev-tag>..HEAD --oneline --no-merges
```

将变更按以下分类写入 `CHANGELOG.md` 顶部（仅保留有内容的分类）：

- **Features** — `feat:` 开头的提交
- **Bug Fixes** — `fix:` 开头的提交
- **Refactor** — `refactor:` 开头的提交
- **UI** — `ui:` 开头的提交

格式：

```markdown
## v<version> (<YYYY-MM-DD>)

### Features
- 描述 (commit-hash)

### Bug Fixes
- 描述 (commit-hash)
```

提交 CHANGELOG：

```bash
git add CHANGELOG.md
git commit -m "docs: update changelog for v<version>"
```

### 4. 打 Tag 并推送

```bash
git push origin main          # 先推 main，确认与远程一致
git tag v<version>            # 在当前 HEAD 打 tag
git push origin v<version>
```

Tag 格式必须为 `v` + 语义化版本号（如 `v0.1.6`），推送后 GitHub Actions 自动触发多平台构建和 Release 草稿。

⚠️ **关键约束**：tag 必须打在 `origin/main` HEAD 上。如果打 tag 后再向 main push 任何 commit，重跑 release workflow 会因 GitHub API 限制（`target_commitish` 非 default branch HEAD 时 `GITHUB_TOKEN` 无法创建 release，返回 403 "Resource not accessible by integration"）而失败。如需追加变更，请走下一个 patch 版本。

## 代码规范

- Commit message 遵循 Conventional Commits：`type: description`
- 常用 type：feat / fix / chore / refactor / ui / docs / test
- Rust 代码位于 `src-tauri/src/`，前端代码位于 `src/`
