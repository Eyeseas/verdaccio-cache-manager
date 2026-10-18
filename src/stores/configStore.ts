import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface HealthCheckConfig {
  on_startup: boolean;
  interval_minutes: number;
  mode: "local" | "remote";
}

export const DEFAULT_HEALTH_CHECK: HealthCheckConfig = {
  on_startup: false,
  interval_minutes: 0,
  mode: "local",
};

export interface AppConfig {
  registry_url: string;
  concurrency: number;
  retry_count: number;
  timeout_secs: number;
  verdaccio_storage_path: string | null;
  target_node_version: string | null;
  health_check?: HealthCheckConfig;
}

interface ConfigStore {
  config: AppConfig;
  loading: boolean;
  loaded: boolean;
  loadConfig: () => Promise<void>;
  saveConfig: (config: AppConfig) => Promise<void>;
  testConnection: (url: string) => Promise<void>;
}

export const useConfigStore = create<ConfigStore>((set) => ({
  config: {
    registry_url: "http://localhost:4873",
    concurrency: 5,
    retry_count: 3,
    timeout_secs: 60,
    verdaccio_storage_path: null,
    target_node_version: null,
    health_check: DEFAULT_HEALTH_CHECK,
  },
  loading: false,
  loaded: false,

  loadConfig: async () => {
    set({ loading: true });
    try {
      const config = await invoke<AppConfig>("get_config");
      set({ config, loaded: true });
    } finally {
      set({ loading: false });
    }
  },

  saveConfig: async (config: AppConfig) => {
    const saved = await invoke<AppConfig>("save_config", { config });
    set({ config: saved });
  },

  testConnection: async (url: string) => {
    await invoke("test_connection", { registryUrl: url });
  },
}));
