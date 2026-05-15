import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { describe, it } from "node:test";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const execFileAsync = promisify(execFile);

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

  it("publishes release assets in parallel and generates latest.json in a final job", async () => {
    const workflow = await readText(".github", "workflows", "release.yml");

    assert.match(workflow, /uses:\s+tauri-apps\/tauri-action@v0\.6/);
    assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY:\s+\$\{\{\s*secrets\.TAURI_SIGNING_PRIVATE_KEY\s*\}\}/);
    assert.match(
      workflow,
      /TAURI_SIGNING_PRIVATE_KEY_PASSWORD:\s+\$\{\{\s*secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD\s*\}\}/
    );

    for (const name of ["macos-arm64", "macos-x64", "linux-x64", "linux-arm64", "windows-x64"]) {
      assert.match(
        workflow,
        new RegExp(`- name: ${name}[\\s\\S]*?(?=- name:|\\n\\s*runs-on:)`),
        `${name} must remain in the release matrix`
      );
    }

    assert.doesNotMatch(workflow, /max-parallel:\s*1/, "release matrix must not serialize platform builds");
    assert.doesNotMatch(workflow, /includeUpdaterJson:\s+true/, "matrix builds must not write shared updater JSON");
    assert.doesNotMatch(
      workflow,
      /includeUpdaterJson:\s+\$\{\{\s*matrix\.includeUpdaterJson\s*\}\}/,
      "updater JSON publishing must not be controlled by matrix jobs"
    );

    assert.match(workflow, /release-info:/, "a single release setup job must create or find the release first");
    assert.match(
      workflow,
      /releaseId:\s+\$\{\{\s*needs\.release-info\.outputs\.release-id\s*\}\}/,
      "parallel package jobs must upload assets to the same release id"
    );
    assert.match(
      workflow,
      /publish-updater-json:[\s\S]*?needs:\s*\n\s+- release-info\s*\n\s+- build/,
      "latest.json must be generated only after every platform build finishes"
    );
    assert.match(
      workflow,
      /node \.github\/scripts\/build-updater-json\.mjs --assets release-assets\.json --notes release-notes\.md --out latest\.json/,
      "the final job must use the deterministic updater JSON generator"
    );
    assert.match(workflow, /gh release upload "\$TAG" latest\.json --clobber/);
    assert.match(workflow, /gh release edit "\$TAG" --draft=false --latest/);
  });

  it("generates updater JSON from release assets while ignoring offline installers", async () => {
    const script = join(repoRoot, ".github", "scripts", "build-updater-json.mjs");
    const dir = await mkdtemp(join(tmpdir(), "updater-json-"));
    const assetsPath = join(dir, "assets.json");
    const notesPath = join(dir, "notes.md");
    const outPath = join(dir, "latest.json");

    const asset = (name) => ({
      name,
      url: `https://github.com/Eyeseas/verdaccio-cache-manager/releases/download/untagged-test/${encodeURIComponent(
        name
      )}`,
    });
    const sig = (name, signature) => ({
      name: `${name}.sig`,
      url: `https://github.com/Eyeseas/verdaccio-cache-manager/releases/download/untagged-test/${encodeURIComponent(
        name
      )}.sig`,
      signature,
    });

    const bundles = [
      "Verdaccio.Cache.Manager_aarch64.app.tar.gz",
      "Verdaccio.Cache.Manager_x64.app.tar.gz",
      "Verdaccio.Cache.Manager_0.1.21_aarch64.AppImage",
      "Verdaccio.Cache.Manager_0.1.21_arm64.deb",
      "Verdaccio.Cache.Manager-0.1.21-1.aarch64.rpm",
      "Verdaccio.Cache.Manager_0.1.21_amd64.AppImage",
      "Verdaccio.Cache.Manager_0.1.21_amd64.deb",
      "Verdaccio.Cache.Manager-0.1.21-1.x86_64.rpm",
      "Verdaccio.Cache.Manager_0.1.21_x64_en-US.msi",
      "Verdaccio.Cache.Manager_0.1.21_x64-setup.exe",
    ];

    await writeFile(
      assetsPath,
      JSON.stringify(
        {
          assets: [
            ...bundles.flatMap((name) => [asset(name), sig(name, `sig:${name}`)]),
            asset("Verdaccio.Cache.Manager_0.1.21_windows_x64_offline-webview2.msi"),
            asset("Verdaccio.Cache.Manager_0.1.21_windows_x64_offline-webview2-setup.exe"),
          ],
        },
        null,
        2
      )
    );
    await writeFile(notesPath, "### Bug Fixes\n- release updater metadata safely");

    try {
      await execFileAsync(process.execPath, [script, "--assets", assetsPath, "--notes", notesPath, "--out", outPath], {
        env: {
          ...process.env,
          TAG: "v0.1.21",
          PUB_DATE: "2026-05-16T00:00:00.000Z",
        },
      });

      const latest = JSON.parse(await readFile(outPath, "utf8"));

      assert.equal(latest.version, "0.1.21");
      assert.equal(latest.notes, "### Bug Fixes\n- release updater metadata safely");
      assert.equal(latest.pub_date, "2026-05-16T00:00:00.000Z");
      assert.deepEqual(Object.keys(latest.platforms).sort(), [
        "darwin-aarch64",
        "darwin-aarch64-app",
        "darwin-x86_64",
        "darwin-x86_64-app",
        "linux-aarch64",
        "linux-aarch64-appimage",
        "linux-aarch64-deb",
        "linux-aarch64-rpm",
        "linux-x86_64",
        "linux-x86_64-appimage",
        "linux-x86_64-deb",
        "linux-x86_64-rpm",
        "windows-x86_64",
        "windows-x86_64-msi",
        "windows-x86_64-nsis",
      ]);
      assert.equal(
        latest.platforms["windows-x86_64"].url,
        "https://github.com/Eyeseas/verdaccio-cache-manager/releases/download/v0.1.21/Verdaccio.Cache.Manager_0.1.21_x64_en-US.msi"
      );
      assert.equal(
        latest.platforms["windows-x86_64-nsis"].url,
        "https://github.com/Eyeseas/verdaccio-cache-manager/releases/download/v0.1.21/Verdaccio.Cache.Manager_0.1.21_x64-setup.exe"
      );
      assert.equal(
        latest.platforms["linux-aarch64"].url,
        "https://github.com/Eyeseas/verdaccio-cache-manager/releases/download/v0.1.21/Verdaccio.Cache.Manager_0.1.21_aarch64.AppImage"
      );
      assert.equal(latest.platforms["darwin-x86_64"].signature, "sig:Verdaccio.Cache.Manager_x64.app.tar.gz");
      assert.ok(
        Object.values(latest.platforms).every((platform) => !platform.url.includes("offline-webview2")),
        "offline WebView2 installers must not be included in updater metadata"
      );
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});
