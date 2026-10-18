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

## manifest.json

所有「导出 tarball」操作都额外生成该文件，原有 `.tgz` 文件名保持兼容：普通包 `name-1.2.3.tgz`，scoped 包 `@scope-name-1.2.3.tgz`。如果不同包转换后文件名冲突，后一个条目导出失败，不覆盖前者。

```json
{
  "kind": "verdaccio-offline-bundle",
  "schema_version": 1,
  "generated_at": 1792368000000,
  "entries": [
    {
      "name": "@scope/pkg",
      "version": "1.2.3",
      "file_name": "@scope-pkg-1.2.3.tgz",
      "integrity": "sha512-<base64 摘要>",
      "shasum": "<SHA-1 十六进制摘要>",
      "expected_latest": "1.2.3"
    }
  ],
  "failures": []
}
```

`generated_at` 为 Unix 毫秒时间戳。`entries` 仅包含成功写入的文件，`failures` 记录未成功导出的 `{name, version, reason}`，内网上传页会显示这些缺文件项。期望 latest 来自外网 packument 的 `dist-tags.latest`，没有标签时为 null；期望版本可能不包含在当前导出子集中。

下载使用 packument 中的 tarball URL（缺少时回退到标准 URL），若上游声明 integrity/shasum，先核对上游摘要，再根据实际文件生成 SHA-512 integrity 和 SHA-1 shasum。上传支持 SHA-512、SHA-384、SHA-256、SHA-1 的 SRI，多摘要时仅认可最强算法；如果还提供 shasum，也必须匹配。未知算法、摘要不符、文件缺失、包名版本与清单不符的条目会标记失败且不上传。manifest 存在时，目录内未声明的 `.tgz` 也不会上传。单独选择 `.tgz` 上传同样会检查相邻的 manifest，并在实际读取上传内容时重新验证。

上传附带 manifest 时不再对每个上传版本自动改 latest，而是在整批完成后，确认期望版本可下载，再调用现有 `promote_latest`。目标标签更新时只前进不回退；期望版本缺失、权限错误、超时，以及目标已是较新版本等差异都会显示。导入不带 manifest 的旧目录时保留现有上传和 latest 推进行为。
