#!/usr/bin/env node

import { readFile, writeFile } from "node:fs/promises";

const fallbackNotes = "See the assets to download this version and install.";

const platformDefinitions = [
  {
    keys: ["darwin-aarch64", "darwin-aarch64-app"],
    test: (name) => /_aarch64\.app\.tar\.gz$/.test(name),
  },
  {
    keys: ["darwin-x86_64", "darwin-x86_64-app"],
    test: (name) => /_x64\.app\.tar\.gz$/.test(name),
  },
  {
    keys: ["linux-x86_64", "linux-x86_64-appimage"],
    test: (name) => /_amd64\.AppImage$/.test(name),
  },
  {
    keys: ["linux-x86_64-deb"],
    test: (name) => /_amd64\.deb$/.test(name),
  },
  {
    keys: ["linux-x86_64-rpm"],
    test: (name) => /-1\.x86_64\.rpm$/.test(name),
  },
  {
    keys: ["linux-aarch64", "linux-aarch64-appimage"],
    test: (name) => /_aarch64\.AppImage$/.test(name),
  },
  {
    keys: ["linux-aarch64-deb"],
    test: (name) => /_arm64\.deb$/.test(name),
  },
  {
    keys: ["linux-aarch64-rpm"],
    test: (name) => /-1\.aarch64\.rpm$/.test(name),
  },
  {
    keys: ["windows-x86_64", "windows-x86_64-msi"],
    test: (name) => /_x64_en-US\.msi$/.test(name),
  },
  {
    keys: ["windows-x86_64-nsis"],
    test: (name) => /_x64-setup\.exe$/.test(name),
  },
];

function parseArgs(argv) {
  const args = new Map();

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];

    if (!arg.startsWith("--")) {
      throw new Error(`Unexpected argument: ${arg}`);
    }

    const value = argv[i + 1];
    if (!value || value.startsWith("--")) {
      throw new Error(`Missing value for ${arg}`);
    }

    args.set(arg.slice(2), value);
    i += 1;
  }

  for (const required of ["assets", "notes", "out"]) {
    if (!args.has(required)) {
      throw new Error(`Missing --${required}`);
    }
  }

  return Object.fromEntries(args);
}

function normalizeAssets(input) {
  const assets = Array.isArray(input) ? input : input.assets;

  if (!Array.isArray(assets)) {
    throw new Error("Release asset JSON must contain an assets array");
  }

  return assets;
}

function assetDownloadUrl(asset) {
  return asset.url || asset.browser_download_url;
}

async function fetchSignature(asset) {
  if (asset.signature) {
    return String(asset.signature).trim();
  }

  const url = asset.apiUrl || asset.url || asset.browser_download_url;
  if (!url) {
    throw new Error(`Signature asset ${asset.name} has no downloadable URL`);
  }

  const headers = {
    Accept: asset.apiUrl ? "application/octet-stream" : "text/plain, application/octet-stream",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  const response = await fetch(url, { headers });
  if (!response.ok) {
    throw new Error(`Failed to download ${asset.name}: ${response.status} ${response.statusText}`);
  }

  return (await response.text()).trim();
}

async function platformEntry(bundle, assetsByName) {
  const url = assetDownloadUrl(bundle);
  if (!url) {
    throw new Error(`Bundle asset ${bundle.name} has no downloadable URL`);
  }

  const signatureAsset = assetsByName.get(`${bundle.name}.sig`);
  if (!signatureAsset) {
    throw new Error(`Missing updater signature for ${bundle.name}`);
  }

  return {
    signature: await fetchSignature(signatureAsset),
    url,
  };
}

function releaseVersion(release) {
  const tag = process.env.TAG || release.tagName;
  if (!tag) {
    throw new Error("TAG environment variable or release tagName is required");
  }

  return tag.replace(/^v/, "");
}

async function main() {
  const { assets: assetsPath, notes: notesPath, out } = parseArgs(process.argv.slice(2));
  const release = JSON.parse(await readFile(assetsPath, "utf8"));
  const assets = normalizeAssets(release);
  const notes = (await readFile(notesPath, "utf8")).trim() || fallbackNotes;
  const assetsByName = new Map(assets.map((asset) => [asset.name, asset]));
  const platforms = {};
  const missing = [];

  const updateBundles = assets.filter(
    (asset) => !asset.name.endsWith(".sig") && asset.name !== "latest.json" && !asset.name.includes("offline-webview2")
  );

  for (const definition of platformDefinitions) {
    const bundle = updateBundles.find((asset) => definition.test(asset.name));

    if (!bundle) {
      missing.push(definition.keys.join("/"));
      continue;
    }

    const entry = await platformEntry(bundle, assetsByName);
    for (const key of definition.keys) {
      platforms[key] = entry;
    }
  }

  if (missing.length > 0) {
    throw new Error(`Missing updater bundles for: ${missing.join(", ")}`);
  }

  const latest = {
    version: releaseVersion(release),
    notes,
    pub_date: process.env.PUB_DATE || release.publishedAt || new Date().toISOString(),
    platforms,
  };

  await writeFile(out, `${JSON.stringify(latest, null, 2)}\n`);
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
