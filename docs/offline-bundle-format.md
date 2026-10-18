# 离线补包文件

安装预检和仓库体检可以导出补包清单。在外网机器的导入页选择该 JSON，点击「导出全部补包 tarball」，将整个目录带到内网，在上传页选择「导入离线包目录」。完成后显示逐项可下载核对，并可跳转到仓库体检。

## replenishment.json

```json
{
  "kind": "verdaccio-replenishment",
  "schema_version": 1,
  "generated_at": "2026-10-19T00:00:00.000Z",
  "source_registry": "http://intranet:4873",
  "entries": [
    { "name": "@scope/pkg", "version": "1.2.3", "reason": "missing_tarball" }
  ]
}
```

`generated_at` 为 ISO 8601 UTC 时间，`source_registry` 是发现缺包的目标仓库。`entries` 按 name/version 去重，version 必须是精确 semver，reason 为 `missing_tarball`、`missing_version` 或 `missing_package`。未知 schema、范围版本、重复项及非法包名会拒绝导入。

安装预检无法在内网元数据中确定具体版本的范围项不能成为精确补包条目；导出时会显示未确定的数量，只导出已确定版本。原始范围仍可从预检结果和复制清单查看。网络或权限失败不会当作缺包导出。

清单导入保持精确闭包，不再递归展开或重新选版本。整单导出会附带 `replenishment.json`，即使部分 tarball 下载失败也保留原清单，以便内网上传后检查完整性。可单独命名清单文件导入；离线目录内约定使用 `replenishment.json`。不含清单的旧 tarball 目录仍然支持上传。

预检以目标 packument 选版本，优先采用插件已有的 `cached_versions` 字段，不更改插件协议；插件未覆盖的包使用 HEAD，服务器不支持 HEAD 时使用带 `Range: bytes=0-0` 的 GET，收到响应头后丢弃响应体。404/410/500 记为缺 tgz，网络错误、鉴权失败及其他服务器错误显示为检查失败。范围输入按现有解析器展开依赖（包括可选依赖和 peer），结果并不模拟 npm 的完整依赖布局或 peer 冲突求解，平台可选包需结合目标系统判断。
