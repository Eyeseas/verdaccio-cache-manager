import { useEffect, useRef } from "react";
import { useConfigStore } from "@/stores/configStore";
import { useHealthStore } from "@/stores/healthStore";
import { scheduleHealthChecks } from "./automaticHealthLogic";

export function useAutomaticHealthChecks() {
  const { config, loaded, loadConfig } = useConfigStore();
  const startupHandled = useRef(false);
  useEffect(() => {
    // 配置失败时不启动后台任务，设置页仍可重试读取。
    void loadConfig().catch(() => {});
  }, [loadConfig]);
  const mode = config.health_check?.mode ?? "local";
  const onStartup = config.health_check?.on_startup ?? false;
  const interval = config.health_check?.interval_minutes ?? 0;
  const target = mode === "local" ? config.verdaccio_storage_path : config.registry_url;
  useEffect(() => {
    if (!loaded) return;
    const startup = !startupHandled.current;
    startupHandled.current = true;
    return scheduleHealthChecks({
      ...useConfigStore.getState().config,
      health_check: { mode, on_startup: onStartup, interval_minutes: interval },
    }, startup, (value) => { void useHealthStore.getState().runCheck(value, "automatic"); });
  }, [loaded, mode, onStartup, interval, target]);
}
