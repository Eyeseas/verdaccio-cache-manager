import assert from "node:assert/strict";
import { beforeEach, describe, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { healthIndicator, useHealthStore } from "./healthStore";
import type { HealthReport } from "@/pages/storageHealthLogic";

const healthy: HealthReport = { scanned: 2, unhealthy: [], unreadable: [], plugin_outdated: false };
beforeEach(() => {
  invoke.mockReset();
  useHealthStore.setState({ latest: null, busy: false });
});

describe("health store", () => {
  it("shares the automatic result and timestamp with manual consumers", async () => {
    invoke.mockResolvedValueOnce(healthy);
    const before = Date.now();
    const result = await useHealthStore.getState().runCheck({ mode: "remote", value: " http://registry " }, "automatic");
    assert.deepEqual(invoke.mock.calls, [["check_remote_storage_health", { registryUrl: "http://registry" }]]);
    assert.equal(result, useHealthStore.getState().latest);
    assert.equal(result?.report, healthy);
    assert.equal(result?.source, "automatic");
    assert.ok(result && result.completedAt >= before);
    assert.equal(useHealthStore.getState().busy, false);
    assert.deepEqual(healthIndicator(result), { count: 0, warning: false, title: "仓库体检" });
  });

  it("prevents automatic/manual duplicate checks and repair overlap", async () => {
    let finish!: (report: HealthReport) => void;
    invoke.mockImplementationOnce(() => new Promise<HealthReport>((resolve) => { finish = resolve; }));
    const state = useHealthStore.getState();
    const first = state.runCheck({ mode: "local", value: "/storage" }, "automatic");
    assert.equal(useHealthStore.getState().busy, true);
    assert.equal(await state.runCheck({ mode: "local", value: "/storage" }, "manual"), null);
    assert.equal(state.beginRepair(), false);
    assert.equal(invoke.mock.calls.length, 1);
    finish(healthy);
    await first;
    assert.equal(state.beginRepair(), true);
    assert.equal(await state.runCheck({ mode: "local", value: "/storage" }, "automatic"), null);
    state.endRepair();
    assert.equal(useHealthStore.getState().busy, false);
  });

  it("skips whitespace targets and retains failures as warning tooltips", async () => {
    const state = useHealthStore.getState();
    assert.equal(await state.runCheck({ mode: "local", value: "  " }, "automatic"), null);
    assert.equal(invoke.mock.calls.length, 0);
    invoke.mockRejectedValueOnce("插件未安装");
    const result = await state.runCheck({ mode: "remote", value: "http://registry" }, "automatic");
    assert.deepEqual(healthIndicator(result), { count: 0, warning: true, title: "体检失败：插件未安装" });
    assert.equal(useHealthStore.getState().busy, false);
    invoke.mockResolvedValueOnce(healthy);
    await state.runCheck({ mode: "local", value: "/storage" }, "manual");
    assert.equal(healthIndicator(useHealthStore.getState().latest).warning, false);
  });

  it("counts problematic and unreadable packages and warns about old plugins", () => {
    const report = { ...healthy, unreadable: ["broken/package.json"], plugin_outdated: true };
    const snapshot = { mode: "local" as const, value: "/storage", source: "automatic" as const, completedAt: 1, error: null, report };
    assert.equal(healthIndicator(snapshot).count, 1);
    assert.equal(healthIndicator(snapshot).warning, true);
  });
});
