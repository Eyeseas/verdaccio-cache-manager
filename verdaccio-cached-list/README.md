# verdaccio-cached-list

配套 Verdaccio Cache Manager 的可选中间件插件。应用的「搜索 → Verdaccio」和「体检 → 远程插件」会调用本插件读取 proxy 缓存；如果应用只连接本地 publish 包，可以不安装。

Verdaccio middleware plugin that exposes **all cached packages** (including proxy-cached) via a single HTTP endpoint.

Verdaccio's built-in `/-/verdaccio/data/packages` only returns locally **published** packages — proxy-cached packages are absent from the result. This plugin fills that gap by scanning the storage directory directly.

## Install

In the same container where Verdaccio runs:

```bash
npm install -g verdaccio-cached-list
```

Or if using the official Docker image, build a custom image:

```dockerfile
FROM verdaccio/verdaccio:6
RUN npm install -g verdaccio-cached-list
```

## Configure

Add to `config.yaml`:

```yaml
middlewares:
  cached-list:
    # optional — defaults to the global `storage` path
    # storage: /verdaccio/storage
```

Restart Verdaccio.

## Usage

```
GET /-/cached-packages
```

Returns a JSON array:

```json
[
  {
    "name": "lodash",
    "description": "Lodash modular utilities.",
    "latest": "4.17.21",
    "versions": ["4.17.0", "4.17.1", "...", "4.17.21"],
    "cached_versions": ["4.17.20", "4.17.21"]
  },
  ...
]
```

- `versions` — all versions known to Verdaccio's cached manifest (the full upstream version list).
- `cached_versions` — versions that actually have a `.tgz` on disk (the strict "cached" set).
- `latest` — `dist-tags.latest`, falling back to the highest version when the tag is absent.
- `dist_latest` — the raw `dist-tags.latest`, or `null` when absent (since 0.2.0).

### Query parameters

- `include_empty=1` — also return packages that have no `.tgz` on disk at all (metadata-only leftovers from uplinks). By default they are omitted. Since 0.2.0.

健康检查模式建议使用 `include_empty=1`，这样插件会返回没有任何 tarball 的 metadata-only 包。客户端同时读取 `dist_latest`，用来判断 `dist-tags.latest` 是否指向实际存在的 tarball。插件升级后必须重启 Verdaccio 才会加载新代码。

Verdaccio Cache Manager uses `include_empty=1` and `dist_latest` for its remote storage health check, which finds versions listed in metadata without a tarball and stale `dist-tags.latest` values that break offline installs. The endpoint is read-only; repairs are performed later through the Registry API by the desktop client.

## How it works

The plugin reads `options.config.storage` (Verdaccio's global storage path) at registration time and scans:

```
<storage>/
├── @scope/
│   └── package-name/
│       └── package.json   ← read here
└── unscoped-package/
    └── package.json       ← read here
```

For each `package.json` it extracts `name`, `description`, `dist-tags.latest`, and `versions` keys.

## License

MIT
