# Changelog

## v0.1.42 (2026-10-29)

### Bug Fixes
- 修复导入离线包目录时上传过程中看不到每个包上传状态的问题：列表逐项实时显示等待/读取中/上传中/成功/已存在/失败及错误信息，进度行显示已完成数量 (61e4075)

## v0.1.41 (2026-10-28)

### Bug Fixes
- 修复执行任务时切换到其他页面或上传页子标签后，上传/工作状态与进度丢失的问题：页面首次访问后常驻挂载，切换仅隐藏；隐藏页面不再响应文件拖放 (e6e6672)
- 修复安装命令导入后只能「重新选择」并弹出文件选择器的问题：改为「重新输入」，返回命令输入框并保留原命令 (fa2ef5c)

## v0.1.40 (2026-10-21)

### Features
- 识别并修复「孤儿 tarball」（storage 中有 tgz、元数据却未登记该版本，npm 安装报版本不存在）：搜索页 Verdaccio 页签标出未登记版本并可单包一键登记，仓库体检新增「未登记」问题类型并随修复一并处理（远程模式经 API 登记且保留原文件，本地模式直接写入 package.json 并备份）；本地索引不再把孤儿版本当作已缓存 (00e0edc)

### Bug Fixes
- 修复离线目录导入等批量上传时同一包多个版本并发 publish，导致 Verdaccio 元数据被覆盖、留下孤儿 tarball 的问题：同名包改为串行发布；发布遇到 409 且版本未登记时自动修复孤儿（能删则删后原名重发，否则换附件名登记） (048ccb2)

## v0.1.39 (2026-10-19)

### Features
- 新增「安装预检」页：解析 lockfile / package.json / 安装命令，对着内网 Verdaccio 检查每个 name@version 是否真的可下载（有 tarball），并指出 latest 指向缺失版本导致 npm 会选偏的情况，可导出补包清单 (96906f0)
- 外网补包 → 内网导入闭环：体检页与安装预检可导出补包清单文件，导入页读取清单得到精确版本列表并一键导出 tarball 离线包 (094ef10)
- 离线包附带 manifest.json：上传页新增「导入离线包目录」，校验 integrity，上传后核对清单并按期望值推进 dist-tags.latest（只前进不回退） (f541c62)
- 自动体检：设置页可开启启动时 / 定时体检（本地目录或远程插件模式，只检查不修复），侧边栏「体检」显示问题数徽标 (cfb2221)

### Bug Fixes
- 修复体检时无法解析的元数据被误判为健康，以及配置异步加载后体检页地址未更新的问题 (8e14d8b)

## v0.1.38 (2026-10-16)

### Features
- 仓库体检支持「远程插件」模式：storage 位于远程服务器或 Docker 卷、本机无法直接访问时，通过 verdaccio-cached-list 插件读取 storage 状态，并通过 Registry API 修复（一次 PUT 剔除幽灵版本并修正 latest，无 tarball 的包整包删除）；服务器插件过旧时给出升级提示 (8141608)
- verdaccio-cached-list 插件升级到 0.2.0：新增 `include_empty=1` 参数返回无 tarball 的包，每个条目新增原始 `dist_latest` 字段；默认响应保持兼容 (3643484)

## v0.1.37 (2026-10-16)

### Features
- 新增「仓库体检」页：扫描 Verdaccio storage，找出元数据中存在但本地没有 tarball 的幽灵版本，以及过时的 dist-tags.latest；支持一键修复（剔除幽灵版本、把 latest 修正为本地最高正式版，无 tarball 的包移除元数据，修改前自动备份 package.json）和复制缺失清单用于外网补包 (6f26bfb)

### Bug Fixes
- 修复离线 Verdaccio 安装时缺包（tarball 返回 500）：上传 publish 不携带 dist-tags 导致 latest 停留在旧的 uplink 缓存版本，而 npm 只要 latest 满足范围就优先选它；现在 publish 成功后会在新版本更高时推进 latest，补传旧版本不会回退 (7df9f75)

## v0.1.36 (2026-10-15)

### Features
- 依赖解析按 npm 客户端（npm-pick-manifest）规则选版本：优先 latest，其次未废弃且 engines.node 兼容的版本，使预热结果与实际 npm install 一致；设置中新增可选的「目标 Node 版本」，留空则使用本机 Node 版本 (1a1a785)

### Bug Fixes
- 修复从 Finder/Dock 启动的安装版显示「Node 未检测到」的问题：GUI 进程只继承精简 PATH，现通过登录 shell 读取完整 PATH 定位 nvm/Homebrew 安装的 node (1a1a785)

## v0.1.35 (2026-10-15)

### Bug Fixes
- 修复完整依赖解析过慢且无进度的问题：改用 npm 精简元数据（每个包名只请求一次）并开启 gzip 压缩传输，按层级并发请求并加入超时与重试；解析时在导入页/搜索页实时显示层级、已解析包数、等待中的包与耗时 (67b44ff)

## v0.1.34 (2026-07-10)

### Bug Fixes
- 修复安装命令导入不带版本号的包（latest 等 dist-tag）必然解析失败的问题：解析链路支持 dist-tags（latest/next/beta…），并把 404、网络错误、range 不满足等真实原因传给前端，导入列表在包名下方直接展示失败原因而非仅 hover 提示 (56bebe2)

## v0.1.33 (2026-07-10)

### Bug Fixes
- 修复 CI 构建失败：测试文件引用 node:assert/strict 但 @types/node 未在 devDependencies 中显式声明，导致干净安装后 tsc 类型检查报 TS2307 (d23ac78)

## v0.1.32 (2026-07-10)

### Features
- 新增安装命令导入：粘贴 npm/pnpm/yarn/bun 的 install/add 命令即可解析根包并展开完整依赖，展示本机 Node 版本 (2341b33, 429247f, d50dd7e, 13b00ea)
- package-lock.json 支持 lockfileVersion 1（npm 6 嵌套 dependencies 树），含 v1 alias 与 bundled/git 条目过滤 (e1e8d32)
- lockfile 导入模式区分：显示"Lockfile 精确闭包"标识，禁用"缓存包及依赖"等重新展开操作，避免引入与 lockfile 不一致的版本 (303cf3b)
- 依赖递归解析纳入 peerDependencies（npm 7+ 自动安装 peer），按 best-effort 处理失败不中断，离线缓存集覆盖更完整 (fe1b3f0)

### Bug Fixes
- 修复 package-lock.json 嵌套条目（node_modules/foo/node_modules/bar）解析出错误包名导致离线安装 404 的问题；同时跳过 link/workspace/git/file 条目并支持 npm alias 真名 (bd70471)
- 修复 pnpm-lock.yaml 把 git/file 依赖的 URL 当版本号、v5 含 "@" 的 peer 后缀 key 被误切分的问题 (7d9cb0e)

## v0.1.31 (2026-06-22)

### Features
- 新增持久化任务批次历史：记录缓存/上传任务的批次、明细、错误码、重试次数与耗时，并支持导出任务报告和从历史失败项重试 (089ea4d)

## v0.1.30 (2026-05-28)

### Bug Fixes
- 修复上传 .tgz 时含嵌套 package.json（如 throttle-debounce 等 dual ESM/CJS 包的 esm/package.json）被错误识别为 unknown/0.0.0 的问题，现仅匹配 tarball 顶级 package.json (ef709ef)

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
