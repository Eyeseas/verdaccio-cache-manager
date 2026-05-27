# Changelog

## v0.1.29 (2026-05-27)

### Bug Fixes
- 搜索页"更新于"显示改用 syncStore.lastSyncAt（后端持久化的真实同步时点），切换 npmjs/Verdaccio tab 不再把时间重置为 0 秒前 (624b015, 05fbdb2)

## v0.1.28 (2026-05-27)

### Features
- 新增 package.json 降级页面：分析当前依赖与目标版本差异，按主/次/补丁分类降级建议 (c3fb428, de49eb3, 29659a2, 28cd366)
- 在线搜索结果按包名匹配度排序，名称越接近越靠前 (a5f23e3)
- 已缓存包 chip 增加一键复制 name@version 按钮 (af39332)

### Bug Fixes
- 修复同主版本号降级被错误归类为跨主版本的问题 (dbec10c)

## v0.1.27 (2026-05-20)

### Bug Fixes
- publish 时携带完整 metadata（dependencies 等）并计算 shasum/integrity，修复安装时 EINTEGRITY 及依赖丢失 (440f77d)
- 本地来源遇到 409 时自动 unpublish 后重新 publish，解决 Verdaccio 有 uplink metadata 但无本地 tarball 的离线场景 (440f77d)

## v0.1.26 (2026-05-20)

### Features
- 依赖解析支持 optionalDependencies 与失败容错 (37800b9)
- 搜索页包名增加一键复制按钮 (90170a3)
- 导出 tarball 时在 sonner toast 中显示实时下载进度 (e625acc)

### Bug Fixes
- 导出 tarball 单包失败不再中止整批，结束后始终弹出汇总提醒 (b99a8a8)

### Refactor
- extract resolve_single and tarball helpers, deduplicate logic (b5f81a8)

## v0.1.25 (2026-05-17)

### Features
- 设置页新增「Verdaccio 插件」卡片，可导出内置 verdaccio-cached-list 离线插件包，导出文件名由 tarball 元数据自动派生 (9bd4406)

## v0.1.24 (2026-05-17)

### Features
- 底部任务栏新增实时分段进度条，按颜色区分成功/进行中/失败/等待状态占比，全部完成后自动渐隐 (a2e2e64)
- 修复任务引擎 Mutex 在执行期间阻塞 get_tasks 导致前端无法实时更新的问题，将 TaskEngine 包装为 Arc 解除锁竞争 (a2e2e64)
- 任务列表改用虚拟滚动优化大量任务时的渲染性能 (a2e2e64)

### Refactor
- sync 初始化从 TaskBar 移至 AppLayout，职责分离 (a2e2e64)

### Tests
- 搭建前后端测试框架与首批用例 (4b544f9)

## v0.1.23 (2026-05-16)

### Features
- 新增 Verdaccio 包 unpublish/deprecate 管理能力：支持右键单项与操作栏批量操作，单版本 unpublish 对齐 npm CLI 行为（PUT 后重取 _rev 再删 tarball、剥离 _attachments/_revisions、latest 被删时按 semver 重指、删末版本走整包删除），并同步清理本地缓存索引；切换数据源清空选择并按当前缓存过滤目标 (e53e903)

## v0.1.22 (2026-05-16)

### Bug Fixes
- 将 draft release 阶段生成的 updater 下载地址重写为正式 tag 地址，修复 latest.json 指向 untagged 资产导致检查更新下载 404 的问题 (4efb822)

## v0.1.21 (2026-05-16)

### Bug Fixes
- 并行化 release 平台打包，并改为最终单独生成 latest.json，避免共享 updater metadata 并发写入冲突 (23a3202)

## v0.1.20 (2026-05-16)

### Bug Fixes
- 串行化 release asset 发布，避免多个平台同时更新 latest.json 导致 release workflow 失败和 updater metadata 不完整 (07fae40)

## v0.1.19 (2026-05-16)

### Bug Fixes
- 生成 Tauri updater 发布产物，修复 GitHub Release 缺少 latest.json 导致检查更新失败的问题 (34ecf91)

## v0.1.18 (2026-05-15)

### Bug Fixes
- 升级 tauri-action 到 v0.6 修复签名文件与 asset 文件名不匹配的问题 (4b3ef05)

## v0.1.17 (2026-05-15)

### Bug Fixes
- 升级 tauri-action 到 v0.5 修复 updater 签名文件检测失败的问题 (6f333f1)

## v0.1.16 (2026-05-15)

### Bug Fixes
- 修复 release workflow 未传递签名密钥导致 updater JSON 未生成的问题 (5975e26)

## v0.1.15 (2026-05-15)

### Features
- 导入页行级右键菜单，支持单包缓存/缓存含依赖/导出 tarball (5d01444)
- 设置页底部显示版本号、爱心图标和作者名称 (d80e0d0)
- 接入 GitHub Release 自动更新 (377102d)
- 侧边栏同步按钮，镜像设置页同步操作 (b39fa8e)

## v0.1.14 (2026-05-15)

### Features
- 导出选中包时按需解析未确定版本，并在导出期间保持操作栏可见、禁用缓存按钮 (0b6630b)

### Refactor
- 抽离导出版本解析逻辑到 importPageLogic，并对过期的 resolve 响应做守卫 (fa2150f)

## v0.1.13 (2026-05-15)

### Features
- 导入 package.json 时按行解析 semver 区间，并实时反馈每个依赖的解析与缓存状态 (cc42fef)

### Bug Fixes
- 缓存过程中保留未解析成功的勾选行，方便后续重试 (5426041)
- 缓存进行中保持导入操作栏始终可见，便于追加操作 (56a55fd)

### Refactor
- 抽离 ImportPage 状态逻辑，并对过期的 resolve 响应做保护，避免回填错乱 (5ff9933)

## v0.1.12 (2026-05-14)

### UI
- 再次更新应用图标，换用更高质量的 PNG 源图重新生成全套桌面与 Store 尺寸 (bd04b14)

## v0.1.11 (2026-05-14)

### UI
- 全新应用图标：macOS squircle 风格，深森林绿背景配等距 3D 米白色快递盒与 "V" 标记，呼应 Verdaccio 品牌；覆盖桌面与 Store 所需全部尺寸 (870a343)

## v0.1.10 (2026-05-14)

### Bug Fixes
- 并发上传 npm 包时不再每包重复登录 Verdaccio，整个扫描任务只触发 1 次 `PUT /-/user/org.couchdb.user:cache-manager`，消除大量 409 日志和密码验证开销 (2454b90)

## v0.1.9 (2026-05-14)

### CI
- 修复 release workflow 的 CHANGELOG 提取逻辑，并把 release 标题改为仅版本号 (9242d15)

## v0.1.8 (2026-05-14)

### Bug Fixes
- Publish 失败 413 返回明确修复提示（建议调大 Verdaccio `max_body_size`） (aeb7126)

### UI
- 扫描 node_modules 后自动比对已缓存包，默认仅勾选未缓存项；上传时按钮显示 loading、逐包展示状态徽章 (d59d3e5)

## v0.1.7 (2026-05-14)

### Bug Fixes
- 修复同步 toast 在状态更新时残留旧错误描述的问题 (09120cd)

### UI
- 优化已选包的控件展示 (5881cbf)

### CI
- Release workflow 从 CHANGELOG.md 自动提取版本说明作为 release body (f2eb868)
