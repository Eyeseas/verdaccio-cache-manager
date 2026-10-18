# 仓库体检：真实 Verdaccio 测试

该测试是显式启用的集成测试，普通 `cargo test` 会忽略它。需要 Node.js 22+、pnpm、Rust，以及本平台的 Tauri 构建依赖；首次运行需要联网下载 Verdaccio 6。

在仓库根目录运行：

```sh
pnpm install --frozen-lockfile
node scripts/verdaccio-e2e.mjs
```

脚本用 `pnpm dlx verdaccio@6` 启动临时服务器，自动分配回环端口，在系统临时目录创建独立 storage、htpasswd 和配置，并加载仓库中的 `verdaccio-cached-list` 插件。允许匿名访问、发布和删除，开放用户注册，所有 uplink 关闭。测试结束后停止服务器、移除临时文件，不使用现有 Verdaccio、Docker 或全局配置。

Rust 测试位于 `src-tauri/tests/verdaccio_e2e.rs`，通过脚本注入的 `VCM_E2E_URL` 和 `VCM_E2E_STORAGE` 访问临时服务器。不要把它指向生产仓库。

覆盖内容：

- 用实际 `RegistryClient::publish_package` 发布普通包和 scoped 包的多个版本。
- 修改临时 storage 元数据，构造幽灵版本、过时 latest，以及普通/scoped 空壳包。
- 验证插件默认过滤空壳，`include_empty=1` 保留空壳，两个模式报告完全一致。
- 调用实际远程修复 API，确认再次体检健康；packument 不含幽灵版本、无效 tag 已删除，空壳包与幽灵 tarball 返回 404，已有 tarball 内容正确。
- 验证 `promote_latest` 前进、重复调用与补发旧版本均不会回退 latest。
- 在 storage 的独立副本执行本地修复，确认再次体检健康，以及每个 `.bak` 与修复前的完整元数据一致。

故障排查时可保留临时文件（PowerShell）：

```powershell
$env:VCM_E2E_KEEP = "1"
node scripts/verdaccio-e2e.mjs
Remove-Item Env:VCM_E2E_KEEP
```

本地修复生产仓库前应先停止 Verdaccio，避免与正在写入的进程竞争。集成测试对在线服务器修改元数据仅用于注入故障，本地修复验证始终在独立副本进行。
