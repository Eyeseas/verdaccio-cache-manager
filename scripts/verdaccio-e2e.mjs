// Opt-in real-server test. No Docker or globally installed Verdaccio is needed.
import { spawn, execFile } from "node:child_process";
import { mkdtemp, mkdir, cp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "node:net";
import { setTimeout as delay } from "node:timers/promises";

const repo = fileURLToPath(new URL("../", import.meta.url));
const root = await mkdtemp(join(tmpdir(), "vcm-verdaccio-e2e-"));
const storage = join(root, "storage");
const plugins = join(root, "plugins");
const config = join(root, "config.yaml");
const port = await new Promise((resolvePort, reject) => {
  const server = createServer();
  server.on("error", reject);
  server.listen(0, "127.0.0.1", () => {
    const { port } = server.address();
    server.close(() => resolvePort(port));
  });
});
const url = `http://127.0.0.1:${port}`;
let logs = "";
let verdaccio;
const windows = process.platform === "win32";
const keep = process.env.VCM_E2E_KEEP === "1";

try {
  await mkdir(storage);
  await mkdir(plugins);
  await cp(join(repo, "verdaccio-cached-list"), join(plugins, "verdaccio-cached-list"), { recursive: true });
  await writeFile(config, `storage: ${JSON.stringify(storage)}
plugins: ${JSON.stringify(plugins)}
auth:
  htpasswd:
    file: ${JSON.stringify(join(root, "htpasswd"))}
uplinks: {}
packages:
  '@*/*':
    access: $all
    publish: $all
    unpublish: $all
  '**':
    access: $all
    publish: $all
    unpublish: $all
middlewares:
  cached-list:
    enabled: true
    storage: ${JSON.stringify(storage)}
log: { type: stdout, format: pretty, level: warn }
`);
  console.log(`[e2e] pnpm dlx verdaccio@6 --config ${config} --listen ${url}`);
  // Launch the .cmd shim explicitly on Windows; POSIX uses a process group for cleanup.
  const command = windows ? process.env.ComSpec || "cmd.exe" : "pnpm";
  const args = windows
    ? ["/d", "/s", "/c", `pnpm dlx verdaccio@6 --config "${config}" --listen ${url}`]
    : ["dlx", "verdaccio@6", "--config", config, "--listen", url];
  verdaccio = spawn(command, args, {
    cwd: root, windowsHide: true, windowsVerbatimArguments: windows, detached: !windows,
    stdio: ["ignore", "pipe", "pipe"],
  });
  verdaccio.stdout.on("data", (data) => { logs += data; });
  verdaccio.stderr.on("data", (data) => { logs += data; });
  let spawnError;
  verdaccio.on("error", (error) => { spawnError = error; });
  const deadline = Date.now() + 240_000;
  while (true) {
    if (spawnError) throw spawnError;
    if (verdaccio.exitCode !== null) throw new Error(`Verdaccio exited: ${verdaccio.exitCode}`);
    try {
      const response = await fetch(`${url}/-/cached-packages?include_empty=1`, { signal: AbortSignal.timeout(1000) });
      if (response.ok && Array.isArray(await response.json())) break;
    } catch { /* npm installation / server startup still in progress */ }
    if (Date.now() > deadline) throw new Error("Verdaccio startup timed out after 240s");
    await delay(300);
  }
  console.log(`[e2e] Verdaccio ready at ${url}; isolated storage ${storage}; uplinks disabled`);
  const code = await new Promise((resolveExit, reject) => {
    const test = spawn("cargo", ["test", "--manifest-path", join(repo, "src-tauri/Cargo.toml"), "--test", "verdaccio_e2e", "--", "--ignored", "--nocapture"], {
      cwd: repo, env: { ...process.env, VCM_E2E_URL: url, VCM_E2E_STORAGE: storage }, stdio: "inherit", windowsHide: true,
    });
    test.on("error", reject);
    test.on("exit", resolveExit);
  });
  if (code !== 0) throw new Error(`Rust E2E failed (exit ${code})`);
  console.log("[e2e] PASS: real Verdaccio plugin, local/remote checks and repairs, tarballs, latest, backups");
} catch (error) {
  console.error(logs);
  console.error(error);
  process.exitCode = 1;
} finally {
  if (verdaccio?.pid) {
    if (windows) {
      await new Promise((done) => execFile("taskkill", ["/PID", String(verdaccio.pid), "/T", "/F"], { windowsHide: true }, done));
    } else {
      try { process.kill(-verdaccio.pid, "SIGTERM"); } catch { /* already exited */ }
      await Promise.race([new Promise((done) => verdaccio.once("exit", done)), delay(5000)]);
    }
  }
  if (keep) console.log(`[e2e] Kept fixtures at ${root}`);
  else if (resolve(root).startsWith(resolve(tmpdir()) + (windows ? "\\" : "/"))) await rm(root, { recursive: true, force: true });
}
