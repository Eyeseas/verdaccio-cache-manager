import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface CachedPackage {
  name: string;
  description: string | null;
  latest_version: string | null;
  versions: string[];
  cached_versions: string[];
}

export type CacheSource = "db" | "none";

interface CacheStore {
  cachedAll: CachedPackage[];
  cachedSource: CacheSource;
  cachedError: string | null;
  loading: boolean;
  lastLoadedAt: number | null;
  loadCachedPackages: (options?: { force?: boolean }) => Promise<void>;
}

const CACHE_TTL_MS = 30_000;

export const useCacheStore = create<CacheStore>((set, get) => ({
  cachedAll: [],
  cachedSource: "none",
  cachedError: null,
  loading: false,
  lastLoadedAt: null,

  loadCachedPackages: async (options) => {
    const force = options?.force === true;
    if (!force) {
      const { lastLoadedAt, loading, cachedSource } = get();
      if (loading) return;
      if (
        cachedSource === "db" &&
        lastLoadedAt !== null &&
        Date.now() - lastLoadedAt < CACHE_TTL_MS
      ) {
        return;
      }
    }
    set({ loading: true, cachedError: null });
    try {
      const packages = await invoke<CachedPackage[]>("get_all_cached_packages");
      set({
        cachedAll: packages,
        cachedSource: "db",
        lastLoadedAt: Date.now(),
      });
    } catch (e) {
      set({ cachedError: `加载缓存索引失败: ${e}`, cachedAll: [], cachedSource: "none" });
    } finally {
      set({ loading: false });
    }
  },
}));
