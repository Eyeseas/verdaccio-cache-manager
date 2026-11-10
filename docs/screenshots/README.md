# 页面截图索引

截图由用户在本机已安装的 Verdaccio Cache Manager Windows 客户端 v0.1.44 中手动采集，用于 README 和功能指南。截图展示当前客户端的实际中文文案和布局，不代表连接到某个固定的生产 Registry。

| 文件 | 页面 | 状态 |
|------|------|------|
| `search-npmjs-versions.png` | 搜索 | 搜索 `react`、展开版本、导出和缓存操作 |
| `import-empty.png` | 导入 | 依赖文件/安装命令入口 |
| `downgrade-empty.png` | 降级 | 选择 package.json 入口 |
| `local-upload-empty.png` | 上传 | 扫描 node_modules、上传 tgz、导入离线包目录入口 |
| `health-empty.png` | 体检 | 本地目录/远程插件和开始体检入口 |
| `preflight-empty.png` | 预检 | Registry、依赖文件和安装命令入口 |
| `settings.png` | 设置 | Registry、插件、自动体检和任务参数 |

手动截图覆盖了 README 中使用的全部页面。体检和预检保留空结果区域，是因为截图时未连接到真实 Registry 或 storage；配置目标并点击开始后会在相同区域显示报告和修复操作。

## 重新采集

在本地运行 `pnpm tauri dev`，按侧边栏进入对应页面即可重新采集。推荐先在「设置」保存 Registry 和 Storage，再分别执行同步、体检或预检，以获得真实结果态。截图文件名保持不变即可自动更新 README 引用。
