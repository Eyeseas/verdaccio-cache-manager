import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface AppConfig {
  registry_url: string;
  concurrency: number;
  retry_count: number;
  timeout_secs: number;
  verdaccio_storage_path: string | null;
}

interface ConfigStore {
  config: AppConfig;
  loading: boolean;
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
  },
  loading: false,

  loadConfig: async () => {
    set({ loading: true });
    try {
      const config = await invoke<AppConfig>("get_config");
      set({ config });
    } finally {
      set({ loading: false });
    }
  },

  saveConfig: async (config: AppConfig) => {
    await invoke("save_config", { config });
    set({ config });
  },

  testConnection: async (url: string) => {
    await invoke("test_connection", { registryUrl: url });
  },
}));
