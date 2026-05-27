import { describe, it, expect, beforeEach, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { useCacheStore } from "./cacheStore";

const initial = {
  cachedAll: [],
  cachedSource: "none" as const,
  cachedError: null,
  loading: false,
  lastLoadedAt: null,
};

describe("cacheStore.loadCachedPackages", () => {
  beforeEach(() => {
    invoke.mockReset();
    useCacheStore.setState(initial);
  });

  it("populates cachedAll from the backend on success", async () => {
    invoke.mockResolvedValueOnce([
      {
        name: "left-pad",
        description: null,
        latest_version: "1.0.0",
        versions: ["1.0.0"],
        cached_versions: ["1.0.0"],
      },
    ]);

    await useCacheStore.getState().loadCachedPackages();

    const s = useCacheStore.getState();
    expect(invoke).toHaveBeenCalledWith("get_all_cached_packages");
    expect(s.cachedAll).toHaveLength(1);
    expect(s.cachedSource).toBe("db");
    expect(s.cachedError).toBeNull();
    expect(s.loading).toBe(false);
  });

  it("records an error and clears data on failure", async () => {
    invoke.mockRejectedValueOnce("boom");

    await useCacheStore.getState().loadCachedPackages();

    const s = useCacheStore.getState();
    expect(s.cachedError).toContain("boom");
    expect(s.cachedAll).toEqual([]);
    expect(s.cachedSource).toBe("none");
    expect(s.loading).toBe(false);
  });

  it("skips re-fetch within TTL and preserves lastLoadedAt", async () => {
    invoke.mockResolvedValueOnce([
      {
        name: "left-pad",
        description: null,
        latest_version: "1.0.0",
        versions: ["1.0.0"],
        cached_versions: ["1.0.0"],
      },
    ]);

    await useCacheStore.getState().loadCachedPackages();
    const firstLoadedAt = useCacheStore.getState().lastLoadedAt;
    expect(firstLoadedAt).not.toBeNull();

    await useCacheStore.getState().loadCachedPackages();

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(useCacheStore.getState().lastLoadedAt).toBe(firstLoadedAt);
  });

  it("re-fetches when force=true even within TTL", async () => {
    invoke.mockResolvedValue([
      {
        name: "left-pad",
        description: null,
        latest_version: "1.0.0",
        versions: ["1.0.0"],
        cached_versions: ["1.0.0"],
      },
    ]);

    await useCacheStore.getState().loadCachedPackages();
    const firstLoadedAt = useCacheStore.getState().lastLoadedAt!;

    await new Promise((r) => setTimeout(r, 5));
    await useCacheStore.getState().loadCachedPackages({ force: true });

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(useCacheStore.getState().lastLoadedAt).toBeGreaterThan(firstLoadedAt);
  });
});
