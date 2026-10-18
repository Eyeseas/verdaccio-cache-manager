import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { HealthReport } from "@/pages/storageHealthLogic";

export interface HealthTarget {
  mode: "local" | "remote";
  value: string;
}

export interface HealthSnapshot extends HealthTarget {
  source: "automatic" | "manual";
  completedAt: number;
  report: HealthReport | null;
  error: string | null;
}

interface HealthStore {
  busy: boolean;
  latest: HealthSnapshot | null;
  runCheck: (target: HealthTarget, source: HealthSnapshot["source"]) => Promise<HealthSnapshot | null>;
  beginRepair: () => boolean;
  endRepair: () => void;
}

/** 所有检查与修复共用此锁；页面卸载不会释放正在进行的操作。 */
export const useHealthStore = create<HealthStore>((set, get) => ({
  busy: false,
  latest: null,
  runCheck: async (target, source) => {
    const value = target.value.trim();
    if (!value || get().busy) return null;
    set({ busy: true });
    let report: HealthReport | null = null;
    let error: string | null = null;
    try {
      report = target.mode === "local"
        ? await invoke<HealthReport>("check_storage_health", { storagePath: value })
        : await invoke<HealthReport>("check_remote_storage_health", { registryUrl: value });
    } catch (e) {
      error = String(e);
    }
    const latest: HealthSnapshot = { ...target, value, source, completedAt: Date.now(), report, error };
    set({ latest, busy: false });
    return latest;
  },
  beginRepair: () => {
    if (get().busy) return false;
    set({ busy: true });
    return true;
  },
  endRepair: () => set({ busy: false }),
}));

export function healthIndicator(snapshot: HealthSnapshot | null) {
  const report = snapshot?.report;
  const count = (report?.unhealthy.length ?? 0) + (report?.unreadable.length ?? 0);
  const warning = Boolean(snapshot?.error || report?.plugin_outdated || report?.unreadable.length);
  const title = snapshot?.error
    ? `体检失败：${snapshot.error}`
    : report?.plugin_outdated
      ? "体检插件版本过旧，请更新插件"
      : report?.unreadable.length
        ? `${report.unreadable.length} 个元数据无法解析`
        : count > 0 ? `${count} 个包存在问题` : "仓库体检";
  return { count, warning, title };
}
