import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface CachedPackage {
  name: string;
  description: string | null;
  latest_version: string | null;
  versions: string[];
  cached_versions: string[];
}

export type CacheSource = "plugin" | "storage" | "none";

interface CacheStore {
  cachedAll: CachedPackage[];
  cachedSource: CacheSource;
  cachedError: string | null;
  loading: boolean;
  lastLoadedAt: number | null;
  loadCachedPackages: (
    registryUrl: string,
    storagePath: string | null
  ) => Promise<void>;
}

export const useCacheStore = create<CacheStore>((set) => ({
  cachedAll: [],
  cachedSource: "none",
  cachedError: null,
  loading: false,
  lastLoadedAt: null,

  loadCachedPackages: async (registryUrl, storagePath) => {
    set({ loading: true, cachedError: null });
    try {
      try {
        const viaPlugin = await invoke<CachedPackage[]>(
          "list_cached_via_plugin",
          { registryUrl }
        );
        set({
          cachedAll: viaPlugin,
          cachedSource: "plugin",
          lastLoadedAt: Date.now(),
        });
        return;
      } catch (e) {
        const msg = String(e);
        if (!msg.includes("PLUGIN_NOT_INSTALLED")) {
          console.warn("plugin endpoint 错误:", e);
        }
      }

      if (storagePath) {
        try {
          const res = await invoke<CachedPackage[]>(
            "scan_verdaccio_storage",
            { storagePath }
          );
          set({
            cachedAll: res,
            cachedSource: "storage",
            lastLoadedAt: Date.now(),
          });
          return;
        } catch (e) {
          set({ cachedError: `扫描 storage 失败: ${e}` });
        }
      } else {
        set({
          cachedError:
            "未安装 verdaccio-cached-list 插件，且未配置 storage 路径（设置页中可填）",
        });
      }
      set({ cachedAll: [], cachedSource: "none" });
    } finally {
      set({ loading: false });
    }
  },
}));
