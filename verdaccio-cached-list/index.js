"use strict";

const fs = require("fs");
const path = require("path");

class CachedListPlugin {
  constructor(config, options) {
    this.config = config || {};
    this.logger = options && options.logger ? options.logger : console;
    const globalConfig = (options && options.config) || {};
    this.storagePath = this.config.storage || globalConfig.storage || null;
  }

  register_middlewares(app) {
    const self = this;

    app.get("/-/cached-packages", async function handler(req, res) {
      try {
        if (!self.storagePath) {
          res.status(500).json({ error: "storage path not configured" });
          return;
        }

        const absolute = path.isAbsolute(self.storagePath)
          ? self.storagePath
          : path.resolve(process.cwd(), self.storagePath);

        const includeEmpty = isTruthy(req.query && req.query.include_empty);
        const results = await scanStorage(absolute, { includeEmpty });
        res.status(200).json(results);
      } catch (err) {
        self.logger.error(
          { err: err && err.message },
          "[cached-list] failed to scan storage"
        );
        res.status(500).json({ error: String(err && err.message) });
      }
    });
  }
}

async function scanStorage(storagePath, options) {
  const includeEmpty = Boolean(options && options.includeEmpty);
  const stat = await fs.promises.stat(storagePath).catch(() => null);
  if (!stat || !stat.isDirectory()) {
    throw new Error(`storage directory not found: ${storagePath}`);
  }

  const results = [];
  const entries = await fs.promises.readdir(storagePath, {
    withFileTypes: true,
  });

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const name = entry.name;
    if (name.startsWith(".") || name === "node_modules") continue;

    const dir = path.join(storagePath, name);

    if (name.startsWith("@")) {
      const subEntries = await fs.promises
        .readdir(dir, { withFileTypes: true })
        .catch(() => []);
      for (const sub of subEntries) {
        if (!sub.isDirectory()) continue;
        const pkg = await readManifest(path.join(dir, sub.name), includeEmpty);
        if (pkg) results.push(pkg);
      }
    } else {
      const pkg = await readManifest(dir, includeEmpty);
      if (pkg) results.push(pkg);
    }
  }

  results.sort((a, b) => a.name.localeCompare(b.name));
  return results;
}

async function readManifest(pkgDir, includeEmpty) {
  const manifestPath = path.join(pkgDir, "package.json");
  const content = await fs.promises
    .readFile(manifestPath, "utf-8")
    .catch(() => null);
  if (!content) return null;

  let json;
  try {
    json = JSON.parse(content);
  } catch {
    return null;
  }

  if (!json || typeof json.name !== "string") return null;

  const cachedVersions = await listCachedVersions(pkgDir, json.name);
  // 默认只返回至少有一个 tarball 的包；体检需要看到纯 uplink 残留的空壳元数据
  if (cachedVersions.length === 0 && !includeEmpty) return null;

  const allVersions = json.versions ? Object.keys(json.versions) : [];
  allVersions.sort(compareVersions);

  const distTagLatest =
    json["dist-tags"] && typeof json["dist-tags"].latest === "string"
      ? json["dist-tags"].latest
      : null;
  const latest =
    distTagLatest ||
    allVersions[allVersions.length - 1] ||
    cachedVersions[cachedVersions.length - 1] ||
    null;

  let description = typeof json.description === "string" ? json.description : null;
  if (!description && latest && json.versions && json.versions[latest]) {
    const v = json.versions[latest];
    if (typeof v.description === "string") description = v.description;
  }

  return {
    name: json.name,
    description,
    latest,
    versions: allVersions,
    cached_versions: cachedVersions,
    // 原始 dist-tags.latest（可能为 null）；`latest` 字段在缺失时会回退到最高版本
    dist_latest: distTagLatest,
  };
}

async function listCachedVersions(pkgDir, pkgName) {
  const lastSlash = pkgName.lastIndexOf("/");
  const namePart = lastSlash >= 0 ? pkgName.slice(lastSlash + 1) : pkgName;
  const prefix = `${namePart}-`;

  const entries = await fs.promises
    .readdir(pkgDir, { withFileTypes: true })
    .catch(() => []);

  const versions = [];
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    const name = entry.name;
    if (!name.startsWith(prefix) || !name.endsWith(".tgz")) continue;
    versions.push(name.slice(prefix.length, name.length - 4));
  }

  versions.sort(compareVersions);
  return versions;
}

function isTruthy(value) {
  return value === "1" || value === "true";
}

function compareVersions(a, b) {
  const parse = (s) =>
    s.split(/[.-]/).map((p) => {
      const n = parseInt(p, 10);
      return Number.isNaN(n) ? 0 : n;
    });
  const pa = parse(a);
  const pb = parse(b);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const na = pa[i] || 0;
    const nb = pb[i] || 0;
    if (na !== nb) return na - nb;
  }
  return 0;
}

module.exports = function factory(config, options) {
  return new CachedListPlugin(config, options);
};
