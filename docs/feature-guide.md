# 功能使用指南

本文按客户端侧边栏顺序说明每个页面的输入、结果和推荐用法。页面截图见 [`screenshots/README.md`](screenshots/README.md)。

## 使用前准备

1. 在「设置」填写 Registry 地址并测试连接。
2. 需要读取 proxy 缓存时，填写本机可访问的 Verdaccio storage；如果 storage 在远程主机或 Docker 卷，安装 `verdaccio-cached-list` 后使用「远程插件」模式。
3. 点击「同步」建立或刷新本地缓存索引。降级和本地上传会使用该索引判断哪些版本已经存在。

## 搜索

「npmjs」标签用于从公网 packument 搜索包。搜索结果可以展开版本、复制 `name@version`、勾选多个版本后缓存到私服，也可以导出 tarball。依赖上传任务会在底部任务栏显示实时状态。

「Verdaccio」标签显示私服中的版本和本地缓存状态。若启用了 `verdaccio-cached-list`，proxy 缓存也会出现在列表中；storage 中有 tarball 但元数据没有登记的版本会标记为「未登记」，可单包修复。

## 导入

入口有「依赖文件」和「安装命令」两个标签：

- 依赖文件支持 `package.json`、`pnpm-lock.yaml`、`package-lock.json`、`replenishment.json`。
- 安装命令支持 npm、pnpm、yarn、bun 的 `install`/`add` 命令，会先解析根包，再展开依赖。
- 常规依赖会按当前 Registry 解析具体版本，并标记已缓存、未缓存和解析失败；lockfile 与补包清单保持精确版本，不会重新选版本。
- 「缓存包及依赖」会递归准备依赖；「导出 Tarball」会生成可带入内网的离线目录。带补包清单的导出目录会包含 `replenishment.json` 和 `manifest.json`。

## 降级

选择项目的 `package.json` 后，页面只分析 `dependencies`、`devDependencies` 和 `optionalDependencies`。对每个依赖优先选择缓存索引中同 major 的最高可用版本；勾选「允许跨 major 降级」后才会扩大到其他 major。

结果可按「全部」「将修改」「无缓存/跳过」筛选。另存为会生成 `package.downgraded.json`，覆盖原文件前会先写入带时间戳的 `.bak` 备份。缓存索引为空时，页面会提示先同步。

## 上传

「扫描 node_modules」选择项目目录后扫描其 `node_modules`，默认选中未缓存项；可搜索包名、重新扫描，再上传选中包。

「上传 .tgz」适用于单个或多个 tarball。若旁边存在 `manifest.json`，会校验文件名、包名、版本、integrity 和 shasum，校验失败的文件不会上传。

「导入离线包目录」会读取目录中的 tarball、manifest 和 `replenishment.json`，逐项显示读取、上传、成功、已存在或失败状态。带清单的整批上传完成后才会按期望值推进 `dist-tags.latest`，并且只前进不回退。

## 体检

体检支持「本地目录」和「远程插件」两种模式：

- 本地目录直接扫描 storage 中的 package.json 和 tarball，适合 storage 已挂载到当前机器的情况。
- 远程插件通过 `/-/cached-packages?include_empty=1` 读取远程 storage，适合 Docker 卷或远程服务器。

报告会找出无 tarball 的元数据版本、tarball 已存在但未登记的孤儿版本、缺失或过时的 `dist-tags.latest`，并提供复制/导出补包清单和批量修复。修复前本地模式会备份 package.json；远程模式通过 Registry API 修改，需要相应权限。

## 预检

填写目标 Registry，选择 lockfile/package.json，或者粘贴安装命令后点击解析。预检会逐项检查目标 packument 中的版本和 tarball 是否可下载，区分可用、缺 tarball、缺版本、范围未确定和网络/权限失败。

缺失项可以复制为文本，也可以导出精确版本的 `replenishment.json`。预检不会把网络错误或鉴权错误误判为缺包；范围依赖无法确定具体版本时会保留为未确定项。

## 设置与自动体检

设置页的关键选项：

- Registry 地址：搜索、上传、体检远程模式和预检使用的目标。
- Storage 路径：本地搜索索引和本地体检使用；远程插件模式可不填。
- Verdaccio 插件：导出可离线安装的 `verdaccio-cached-list` tarball。
- 自动体检：可在启动时执行，也可选择每 30 分钟、1 小时或 6 小时；只检查不自动修复，结果会在侧边栏徽标和体检页显示。
- 任务参数：并发数 1–20、重试次数 0–10、超时 10–300 秒、可选目标 Node 版本。

## 离线闭环排查

1. 预检或体检没有发现缺包时，仍应确认目标 Registry 地址和 `latest` 指向的是有 tarball 的版本。
2. 导入页只导入精确补包清单，不会再次按范围选版本。
3. 上传页出现 manifest 校验失败时，检查 tarball 是否被改名、截断或来自错误的 Registry。
4. 上传后重新执行体检；若 latest 仍不正确，确认 Verdaccio 允许发布/修改元数据且 uplink 已关闭或可控。

文件格式和校验规则详见 [`offline-bundle-format.md`](offline-bundle-format.md)，真实 Verdaccio 测试详见 [`storage-health-testing.md`](storage-health-testing.md)。
