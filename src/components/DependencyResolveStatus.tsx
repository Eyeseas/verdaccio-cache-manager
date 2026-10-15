import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";

/** 与 Rust 端 `dependency_resolver::ResolveProgress` 对应 */
export interface DependencyResolveProgress {
  resolved: number;
  fetched: number;
  failed: number;
  in_flight: number;
  depth: number;
  current: string | null;
  waiting: string[];
}

interface DependencyResolveStatusProps {
  active: boolean;
  className?: string;
}

/** 依赖闭包解析进行中时展示实时进度：层级、已解析数、请求数、等待中的包与耗时。 */
export function DependencyResolveStatus({ active, className }: DependencyResolveStatusProps) {
  const [progress, setProgress] = useState<DependencyResolveProgress | null>(null);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    if (!active) return;
    setProgress(null);
    setElapsed(0);

    let stopped = false;
    let unlisten: (() => void) | undefined;
    listen<DependencyResolveProgress>("dependency-resolve-progress", (event) => {
      if (!stopped) setProgress(event.payload);
    }).then((un) => {
      if (stopped) un();
      else unlisten = un;
    });

    const startedAt = Date.now();
    const timer = window.setInterval(() => {
      setElapsed(Math.floor((Date.now() - startedAt) / 1000));
    }, 1000);

    return () => {
      stopped = true;
      unlisten?.();
      window.clearInterval(timer);
    };
  }, [active]);

  if (!active) return null;

  return (
    <div
      className={cn(
        "flex items-center gap-2 text-xs text-muted-foreground",
        className
      )}
    >
      <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
      {progress ? (
        <span className="truncate">
          第 {progress.depth} 层 · 已解析 {progress.resolved} 个包 · 已请求元数据{" "}
          {progress.fetched} 个
          {progress.failed > 0 && `（失败 ${progress.failed}）`}
          {progress.in_flight > 0 &&
            ` · 等待 ${progress.in_flight} 个：${progress.waiting.join(", ")}${
              progress.in_flight > progress.waiting.length ? " 等" : ""
            }`}
        </span>
      ) : (
        <span>正在解析根包版本…</span>
      )}
      <span className="shrink-0 tabular-nums">· {elapsed}s</span>
    </div>
  );
}
