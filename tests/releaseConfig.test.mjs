import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

async function readJson(...segments) {
  return JSON.parse(await readFile(join(repoRoot, ...segments), "utf8"));
}

async function readText(...segments) {
  return readFile(join(repoRoot, ...segments), "utf8");
}

describe("release updater configuration", () => {
  it("creates updater artifacts for signed release builds", async () => {
    const config = await readJson("src-tauri", "tauri.conf.json");

    assert.equal(config.bundle.createUpdaterArtifacts, true);
    assert.deepEqual(config.plugins.updater.endpoints, [
      "https://github.com/Eyeseas/verdaccio-cache-manager/releases/latest/download/latest.json",
    ]);
    assert.match(config.plugins.updater.pubkey, /\S/);
  });

  it("does not publish offline WebView2 installers as updater artifacts", async () => {
    const config = await readJson("src-tauri", "tauri.windows-offline.conf.json");

    assert.equal(config.bundle.createUpdaterArtifacts, false);
  });

  it("keeps GitHub Release updater JSON publishing enabled for standard builds", async () => {
    const workflow = await readText(".github", "workflows", "release.yml");

    assert.match(workflow, /uses:\s+tauri-apps\/tauri-action@v0\.6/);
    assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY:\s+\$\{\{\s*secrets\.TAURI_SIGNING_PRIVATE_KEY\s*\}\}/);
    assert.match(
      workflow,
      /TAURI_SIGNING_PRIVATE_KEY_PASSWORD:\s+\$\{\{\s*secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD\s*\}\}/
    );
    assert.match(workflow, /includeUpdaterJson:\s+\$\{\{\s*matrix\.includeUpdaterJson\s*\}\}/);

    for (const name of ["macos-arm64", "macos-x64", "linux-x64", "linux-arm64", "windows-x64"]) {
      assert.match(
        workflow,
        new RegExp(`- name: ${name}[\\s\\S]*?includeUpdaterJson: true`),
        `${name} must upload the static updater JSON`
      );
    }

    assert.match(
      workflow,
      /- name: windows-x64-offline-webview2[\s\S]*?includeUpdaterJson: false/,
      "offline WebView2 installer must not replace the updater JSON"
    );
  });

  it("serializes release asset publishing to avoid latest.json write races", async () => {
    const workflow = await readText(".github", "workflows", "release.yml");

    assert.match(
      workflow,
      /strategy:\s*\n\s+fail-fast:\s+false\s*\n\s+max-parallel:\s+1\s*\n\s+matrix:/,
      "release matrix must publish one job at a time because tauri-action updates the shared latest.json asset"
    );
  });
});
