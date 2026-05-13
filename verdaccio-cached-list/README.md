# verdaccio-cached-list

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
