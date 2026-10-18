import { DEFAULT_HEALTH_CHECK, type AppConfig } from "@/stores/configStore";
import type { HealthTarget } from "@/stores/healthStore";

export function automaticHealthTarget(config: AppConfig): HealthTarget | null {
  const { mode } = config.health_check ?? DEFAULT_HEALTH_CHECK;
  const value = (mode === "local" ? config.verdaccio_storage_path : config.registry_url)?.trim();
  return value ? { mode, value } : null;
}

/** 返回清理函数，配置变化、组件卸载时取消旧定时器。 */
export function scheduleHealthChecks(config: AppConfig, startup: boolean, check: (target: HealthTarget) => void) {
  const settings = config.health_check ?? DEFAULT_HEALTH_CHECK;
  const target = automaticHealthTarget(config);
  if (!target) return () => {};
  if (startup && settings.on_startup) check(target);
  const interval = settings.interval_minutes;
  if (![30, 60, 360].includes(interval)) return () => {};
  const timer = setInterval(() => check(target), interval * 60_000);
  return () => clearInterval(timer);
}
