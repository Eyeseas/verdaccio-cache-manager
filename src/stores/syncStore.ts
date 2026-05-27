import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { useCacheStore } from "./cacheStore";

interface SyncStatusEvent {
  status: string;
  progress: number;
  total: number;
  message: string | null;
}

interface SyncInfo {
  last_registry_url: string | null;
  last_sync_at: string | null;
  is_running: boolean;
}

interface SyncStore {
  status: "idle" | "syncing" | "done" | "error";
  progress: number;
  total: number;
  lastSyncAt: number | null;
  error: string | null;
  startSync: () => Promise<void>;
  clearIndex: () => Promise<void>;
  getSyncInfo: () => Promise<void>;
  startListening: () => void;
}

const SYNC_TOAST_ID = "cache-sync";

export const useSyncStore = create<SyncStore>((set) => ({
  status: "idle",
  progress: 0,
  total: 0,
  lastSyncAt: null,
  error: null,

  startSync: async () => {
    try {
      await invoke("start_cache_sync");
    } catch (e) {
      set({ status: "error", error: String(e) });
      toast.error("同步失败", { id: SYNC_TOAST_ID, description: String(e) });
    }
  },

  clearIndex: async () => {
    try {
      await invoke("clear_cache_index");
      set({ lastSyncAt: null });
      useCacheStore.getState().loadCachedPackages({ force: true });
      toast.success("缓存索引已清除");
    } catch (e) {
      toast.error("清除失败", { description: String(e) });
    }
  },

  getSyncInfo: async () => {
    try {
      const info = await invoke<SyncInfo>("get_sync_info");
      if (info.is_running) {
        set({ status: "syncing" });
      }
      if (info.last_sync_at) {
        set({ lastSyncAt: parseInt(info.last_sync_at, 10) || null });
      }
    } catch (_) {}
  },

  startListening: () => {
    listen<SyncStatusEvent>("sync-status", (event) => {
      const { status, progress, total, message } = event.payload;
      switch (status) {
        case "started":
          set({ status: "syncing", progress: 0, total: 0, error: null });
          toast.loading("正在同步缓存索引...", { id: SYNC_TOAST_ID, description: undefined, duration: Infinity });
          break;
        case "fetching":
          set({ status: "syncing", progress: 0, total: 0 });
          toast.loading(message || "正在获取包列表...", {
            id: SYNC_TOAST_ID,
            description: undefined,
            duration: Infinity,
          });
          break;
        case "progress":
          set({ status: "syncing", progress, total });
          toast.loading(
            message || `同步缓存索引 ${progress}/${total}`,
            { id: SYNC_TOAST_ID, description: undefined, duration: Infinity }
          );
          break;
        case "done":
          set({
            status: "done",
            progress: total,
            total,
            lastSyncAt: Date.now(),
          });
          toast.success(`索引同步完成，共 ${total} 个包`, {
            id: SYNC_TOAST_ID,
            description: undefined,
            duration: 3000,
          });
          useCacheStore.getState().loadCachedPackages({ force: true });
          break;
        case "error":
          set({ status: "error", error: message });
          toast.error("索引同步失败", {
            id: SYNC_TOAST_ID,
            description: message ?? undefined,
            duration: 5000,
          });
          break;
      }
    });
  },
}));
