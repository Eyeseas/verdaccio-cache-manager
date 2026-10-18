import assert from "node:assert/strict";
import { afterEach, describe, it, vi } from "vitest";
import { automaticHealthTarget, scheduleHealthChecks } from "./automaticHealthLogic";
import { DEFAULT_HEALTH_CHECK, useConfigStore } from "@/stores/configStore";

const base = useConfigStore.getState().config;
afterEach(() => vi.useRealTimers());

describe("automatic health schedule", () => {
  it("uses configured local/remote targets and silently skips blanks", () => {
    assert.equal(automaticHealthTarget({ ...base, verdaccio_storage_path: "  " }), null);
    assert.deepEqual(automaticHealthTarget({ ...base, verdaccio_storage_path: " /storage " }), { mode: "local", value: "/storage" });
    const remote = { ...DEFAULT_HEALTH_CHECK, mode: "remote" as const };
    assert.deepEqual(automaticHealthTarget({ ...base, registry_url: " http://registry ", health_check: remote }), { mode: "remote", value: "http://registry" });
    assert.equal(automaticHealthTarget({ ...base, registry_url: "", health_check: remote }), null);
  });

  it("runs once on startup, repeats at the selected interval, and cancels the timer", () => {
    vi.useFakeTimers();
    const check = vi.fn();
    const config = { ...base, verdaccio_storage_path: "/storage", health_check: { ...DEFAULT_HEALTH_CHECK, on_startup: true, interval_minutes: 30 } };
    const stop = scheduleHealthChecks(config, true, check);
    assert.equal(check.mock.calls.length, 1);
    vi.advanceTimersByTime(30 * 60_000 - 1);
    assert.equal(check.mock.calls.length, 1);
    vi.advanceTimersByTime(1);
    assert.equal(check.mock.calls.length, 2);
    stop();
    vi.advanceTimersByTime(360 * 60_000);
    assert.equal(check.mock.calls.length, 2);
  });

  it("does not repeat startup on rescheduling and supports every interval", () => {
    vi.useFakeTimers();
    for (const interval of [0, 30, 60, 360]) {
      const check = vi.fn();
      const stop = scheduleHealthChecks({ ...base, verdaccio_storage_path: "/storage", health_check: { ...DEFAULT_HEALTH_CHECK, on_startup: true, interval_minutes: interval } }, false, check);
      assert.equal(check.mock.calls.length, 0);
      vi.advanceTimersByTime((interval || 360) * 60_000);
      assert.equal(check.mock.calls.length, interval === 0 ? 0 : 1);
      stop();
    }
  });

  it("schedules nothing without a target or with invalid intervals", () => {
    vi.useFakeTimers();
    const check = vi.fn();
    scheduleHealthChecks({ ...base, verdaccio_storage_path: null, health_check: { ...DEFAULT_HEALTH_CHECK, on_startup: true, interval_minutes: 30 } }, true, check);
    scheduleHealthChecks({ ...base, verdaccio_storage_path: "/storage", health_check: { ...DEFAULT_HEALTH_CHECK, interval_minutes: -1 } }, true, check);
    vi.advanceTimersByTime(360 * 60_000);
    assert.equal(check.mock.calls.length, 0);
    assert.equal(vi.getTimerCount(), 0);
  });
});
