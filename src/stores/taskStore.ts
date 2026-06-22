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

export type TaskErrorCode =
  | "AUTH_FAILED"
  | "PACKAGE_EXISTS"
  | "PAYLOAD_TOO_LARGE"
  | "NOT_FOUND"
  | "NETWORK_TIMEOUT"
  | "DOWNLOAD_FAILED"
  | "UPLOAD_FAILED"
  | "LOCAL_READ_FAILED"
  | "PACK_FAILED"
  | "UNPUBLISH_RETRY_FAILED"
  | "UNKNOWN";

export interface CacheTask {
  id: string;
  batch_id: string;
  package_name: string;
  version: string;
  tarball_url: string | null;
  status: TaskStatus;
  error: string | null;
  error_code: TaskErrorCode | null;
  attempt_count: number;
  started_at: string | null;
  finished_at: string | null;
  duration_ms: number | null;
}

export interface TaskBatchSummary {
  id: string;
  source: string;
  target_registry: string;
  created_at: string;
  finished_at: string | null;
  total: number;
  success: number;
  failed: number;
  skipped: number;
}

export interface TaskItemRecord {
  id: string;
  batch_id: string;
  package_name: string;
  version: string;
  tarball_url: string | null;
  status: TaskStatus;
  error_code: TaskErrorCode | null;
  error_message: string | null;
  attempt_count: number;
  started_at: string | null;
  finished_at: string | null;
  duration_ms: number | null;
}

interface TaskProgressEvent {
  id: string;
  batch_id: string;
  package_name: string;
  version: string;
  status: TaskStatus;
  error: string | null;
  error_code: TaskErrorCode | null;
  attempt_count: number;
  started_at: string | null;
  finished_at: string | null;
  duration_ms: number | null;
}

interface ResolvedDep {
  package_name: string;
  version: string;
}

interface TaskStore {
  tasks: CacheTask[];
  listening: boolean;
  currentBatchId: string | null;
  currentBatch: TaskBatchSummary | null;
  batches: TaskBatchSummary[];
  selectedBatchItems: TaskItemRecord[];
  fetchTasks: () => Promise<void>;
  startCacheTasks: (
    packages: { package_name: string; version: string; tarball_url?: string }[]
  ) => Promise<void>;
  resolveDependencies: (
    packages: { package_name: string; version: string }[]
  ) => Promise<ResolvedDep[]>;
  retryFailed: () => Promise<void>;
  clearCompleted: () => Promise<void>;
  fetchBatches: (limit?: number) => Promise<void>;
  fetchBatchItems: (batchId: string) => Promise<TaskItemRecord[]>;
  fetchCurrentBatch: () => Promise<void>;
  retryBatchFailed: (batchId: string) => Promise<string>;
  exportBatchReport: (
    batchId: string,
    outputPath: string,
    format: "markdown" | "json"
  ) => Promise<string>;
  startListening: () => void;
}

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: [],
  listening: false,
  currentBatchId: null,
  currentBatch: null,
  batches: [],
  selectedBatchItems: [],

  fetchTasks: async () => {
    const tasks = await invoke<CacheTask[]>("get_tasks");
    set({
      tasks,
      currentBatchId: tasks[0]?.batch_id ?? get().currentBatchId,
    });
  },

  startCacheTasks: async (packages) => {
    const batchId = await invoke<string>("start_cache_tasks", { packages });
    set({ currentBatchId: batchId });
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

  fetchBatches: async (limit = 20) => {
    const batches = await invoke<TaskBatchSummary[]>("get_task_batches", {
      limit,
    });
    set({ batches });
  },

  fetchBatchItems: async (batchId) => {
    const selectedBatchItems = await invoke<TaskItemRecord[]>(
      "get_task_batch_items",
      { batchId }
    );
    set({ selectedBatchItems });
    return selectedBatchItems;
  },

  fetchCurrentBatch: async () => {
    const currentBatch = await invoke<TaskBatchSummary | null>(
      "get_current_task_batch"
    );
    set({
      currentBatch,
      currentBatchId: currentBatch?.id ?? get().currentBatchId,
    });
  },

  retryBatchFailed: async (batchId) => {
    const nextBatchId = await invoke<string>("retry_batch_failed_tasks", {
      batchId,
    });
    set({ currentBatchId: nextBatchId });
    await get().fetchTasks();
    return nextBatchId;
  },

  exportBatchReport: async (batchId, outputPath, format) => {
    return invoke<string>("export_task_batch_report", {
      batchId,
      outputPath,
      format,
    });
  },

  startListening: () => {
    if (get().listening) return;
    set({ listening: true });

    listen<TaskProgressEvent>("task-progress", (event) => {
      const payload = event.payload;
      set((state) => ({
        tasks: state.tasks.map((t) =>
          t.id === payload.id
            ? {
                ...t,
                batch_id: payload.batch_id,
                package_name: payload.package_name,
                version: payload.version,
                status: payload.status,
                error: payload.error,
                error_code: payload.error_code,
                attempt_count: payload.attempt_count,
                started_at: payload.started_at,
                finished_at: payload.finished_at,
                duration_ms: payload.duration_ms,
              }
            : t
        ),
        currentBatchId: payload.batch_id ?? state.currentBatchId,
      }));
    });
  },
}));
