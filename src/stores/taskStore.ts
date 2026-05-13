import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type TaskStatus =
  | "Pending"
  | "Downloading"
  | "Uploading"
  | "Success"
  | "Failed"
  | "Skipped";

export interface CacheTask {
  id: string;
  package_name: string;
  version: string;
  tarball_url: string | null;
  status: TaskStatus;
  error: string | null;
}

interface TaskProgressEvent {
  id: string;
  status: TaskStatus;
  error: string | null;
}

interface ResolvedDep {
  package_name: string;
  version: string;
}

interface TaskStore {
  tasks: CacheTask[];
  listening: boolean;
  fetchTasks: () => Promise<void>;
  startCacheTasks: (
    packages: { package_name: string; version: string; tarball_url?: string }[]
  ) => Promise<void>;
  resolveDependencies: (
    packages: { package_name: string; version: string }[]
  ) => Promise<ResolvedDep[]>;
  retryFailed: () => Promise<void>;
  clearCompleted: () => Promise<void>;
  startListening: () => void;
}

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: [],
  listening: false,

  fetchTasks: async () => {
    const tasks = await invoke<CacheTask[]>("get_tasks");
    set({ tasks });
  },

  startCacheTasks: async (packages) => {
    await invoke("start_cache_tasks", { packages });
    await get().fetchTasks();
  },

  resolveDependencies: async (packages) => {
    const resolved = await invoke<ResolvedDep[]>("resolve_dependencies", {
      packages,
    });
    return resolved;
  },

  retryFailed: async () => {
    await invoke("retry_failed_tasks");
    await get().fetchTasks();
  },

  clearCompleted: async () => {
    await invoke("clear_completed_tasks");
    await get().fetchTasks();
  },

  startListening: () => {
    if (get().listening) return;
    set({ listening: true });

    listen<TaskProgressEvent>("task-progress", (event) => {
      const payload = event.payload;
      set((state) => ({
        tasks: state.tasks.map((t) =>
          t.id === payload.id
            ? { ...t, status: payload.status, error: payload.error }
            : t
        ),
      }));
    });
  },
}));
