# 自动体检

自动体检在「设置」页配置，后台只检查、不自动修复。它适合在客户端运行期间定期发现离线 Verdaccio 的元数据问题，再由操作者在「体检」页确认后修复。

## 配置

在「设置 → 自动体检」中：

- 勾选「启动时自动体检」，应用启动并完成配置加载后执行一次。
- 选择「本地目录」或「远程插件」模式。前者读取设置中的 Storage 路径，后者使用 Registry 地址和 `verdaccio-cached-list` 插件。
- 选择关闭、每 30 分钟、每 1 小时或每 6 小时。
- 点击「保存」后配置才会生效；默认全部关闭。

本地模式需要当前机器能读取 Verdaccio storage。远程模式需要插件支持 `include_empty=1` 和 `dist_latest`，插件过旧时体检页会提示从设置页重新导出 0.2.0 插件包。

## 检查内容

自动体检复用手动体检逻辑，检查以下情况：

- package metadata 中有版本，但 storage 中没有对应 tarball；
- storage 中有 tarball，但 package metadata 没有登记该版本（孤儿 tarball）；
- `dist-tags.latest` 缺失、指向无 tarball 版本，或落后于本地最高正式版本；
- 完全没有 tarball 的空壳包和无法解析的 metadata。

检查结果保存于当前应用会话的 zustand store，体检页会显示最近模式、目标、时间和问题列表。侧边栏「体检」图标会显示问题数量；插件过旧、配置缺失或检查失败时显示警告徽标，悬停可以查看原因。

## 调度规则

自动任务只在应用运行时执行。未配置目标时静默跳过；已有检查或手动修复正在运行时不会并发启动，当前周期直接跳过并等待下一个周期。切换 Registry、Storage 或体检模式后，下一次检查会使用新目标。

手动修复不会由自动体检触发。操作者在体检页选择包并确认修复后，页面会重新检查受影响的包并更新侧边栏提示。

## 推荐流程

1. 先在设置页测试 Registry，并确认 Storage 路径或插件可用。
2. 手动执行一次体检，确认结果符合预期。
3. 再开启启动或定时体检；自动任务发现问题后，打开体检页查看详情。
4. 修复前确认 Verdaccio 处于离线或 uplink 可控状态，避免修复后的 metadata 被上游重新合并。
5. 修复后导出补包清单，必要时在外网导出 tarball，再通过上传页导入并重新体检。

更多字段和离线目录校验见 [`offline-bundle-format.md`](offline-bundle-format.md)，真实服务器测试见 [`storage-health-testing.md`](storage-health-testing.md)。
