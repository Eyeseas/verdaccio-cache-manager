# 页面截图索引

截图由本机已安装的 Verdaccio Cache Manager Windows 客户端 v0.1.44 采集，用于 README 和功能指南。截图展示当前客户端的实际中文文案和布局，不代表连接到某个固定的生产 Registry。

| 文件 | 页面 | 状态 |
|------|------|------|
| `search-npmjs-versions.png` | 搜索 | npmjs 搜索 `typescript` 的版本结果 |
| `search-verdaccio-cached.png` | 搜索 | Verdaccio 为空状态 |
| `import-empty.png` | 导入 | 依赖文件/安装命令入口 |
| `import-package-json.png` | 导入 | 解析项目 `package.json` 的依赖列表 |
| `local-upload-empty.png` | 上传 | 扫描 node_modules 入口 |
| `local-upload-scan.png` | 上传 | 实际扫描仓库 node_modules 的 34 个包 |
| `downgrade-empty.png` | 降级 | 选择 package.json 入口 |
| `health-empty.png` | 体检 | 本地目录/远程插件入口 |
| `preflight-empty.png` | 预检 | Registry、依赖文件和安装命令入口 |
| `settings.png` | 设置 | Registry、插件、自动体检和任务参数 |

截图时本机没有运行中的 Verdaccio 服务，因此 Verdaccio 列表、体检报告和预检报告使用空状态截图。它们仍然展示了可操作的入口和前置配置；连接到真实 Registry 或 storage 后会出现结果列表和修复按钮。

## 重新采集

在本地运行 `pnpm tauri dev`，按侧边栏进入对应页面即可重新采集。推荐先在「设置」保存 Registry 和 Storage，再分别执行同步、体检或预检，以获得真实结果态。截图文件名保持不变即可自动更新 README 引用。
