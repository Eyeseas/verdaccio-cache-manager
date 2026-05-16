import { useEffect, useMemo, useRef, useState } from "react";
import { useTaskStore, CacheTask } from "@/stores/taskStore";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ChevronUp,
  ChevronDown,
  RotateCcw,
  Trash2,
} from "lucide-react";

export function TaskBar() {
  const { tasks, fetchTasks, startListening, retryFailed, clearCompleted } =
    useTaskStore();
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    startListening();
    fetchTasks();
  }, [startListening, fetchTasks]);

  const counts = useMemo(
    () => ({
      total: tasks.length,
      success: tasks.filter((t) => t.status === "Success").length,
      failed: tasks.filter((t) => t.status === "Failed").length,
      running: tasks.filter(
        (t) => t.status === "Downloading" || t.status === "Uploading"
      ).length,
      pending: tasks.filter((t) => t.status === "Pending").length,
      skipped: tasks.filter((t) => t.status === "Skipped").length,
    }),
    [tasks]
  );

  const isAllDone = counts.total > 0 && counts.running === 0 && counts.pending === 0;
  const percent = counts.total > 0 ? Math.round(((counts.success + counts.skipped) / counts.total) * 100) : 0;

  if (counts.total === 0) {
    return (
      <footer className="flex h-10 items-center border-t bg-muted/30 px-4 text-sm text-muted-foreground">
        <span>暂无任务</span>
      </footer>
    );
  }

  return (
    <footer className="border-t bg-muted/30">
      <div
        className="flex h-10 cursor-pointer items-center justify-between px-4 text-sm"
        onClick={() => setExpanded(!expanded)}
      >
        <div className="flex items-center gap-3">
          {expanded ? (
            <ChevronDown className="h-4 w-4" />
          ) : (
            <ChevronUp className="h-4 w-4" />
          )}
          <span>
            任务: {counts.success}/{counts.total} 完成
          </span>
          {counts.running > 0 && (
            <Badge variant="default" className="h-5">
              {counts.running} 进行中
            </Badge>
          )}
          {counts.failed > 0 && (
            <Badge variant="destructive" className="h-5">
              {counts.failed} 失败
            </Badge>
          )}
          {counts.skipped > 0 && (
            <Badge variant="secondary" className="h-5">
              {counts.skipped} 跳过
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2">
          {!isAllDone && (
            <span className="text-xs text-muted-foreground">{percent}%</span>
          )}
          <div className="flex gap-1">
            {counts.failed > 0 && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2"
                onClick={(e) => {
                  e.stopPropagation();
                  retryFailed();
                }}
              >
                <RotateCcw className="mr-1 h-3 w-3" />
                重试
              </Button>
            )}
            {counts.success > 0 && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2"
                onClick={(e) => {
                  e.stopPropagation();
                  clearCompleted();
                }}
              >
                <Trash2 className="mr-1 h-3 w-3" />
                清除
              </Button>
            )}
          </div>
        </div>
      </div>

      <ProgressBar counts={counts} isAllDone={isAllDone} />

      {expanded && <TaskList tasks={tasks} />}
    </footer>
  );
}

interface Counts {
  total: number;
  success: number;
  failed: number;
  running: number;
  pending: number;
  skipped: number;
}

function ProgressBar({ counts, isAllDone }: { counts: Counts; isAllDone: boolean }) {
  const [visible, setVisible] = useState(true);
  const timerRef = useRef<ReturnType<typeof setTimeout>>(null);

  useEffect(() => {
    if (isAllDone && counts.failed === 0) {
      timerRef.current = setTimeout(() => setVisible(false), 3000);
    } else {
      setVisible(true);
      if (timerRef.current) clearTimeout(timerRef.current);
    }
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [isAllDone, counts.failed]);

  if (!visible) return null;

  const { total, success, running, failed, pending, skipped } = counts;
  const successPct = (success / total) * 100;
  const runningPct = (running / total) * 100;
  const failedPct = (failed / total) * 100;
  const restPct = ((pending + skipped) / total) * 100;

  return (
    <div
      className="px-4 pb-1.5 transition-opacity duration-300"
      style={{ opacity: isAllDone && counts.failed === 0 ? 0.5 : 1 }}
    >
      <div className="flex h-1 overflow-hidden rounded-full bg-muted">
        <div
          className="bg-green-500 transition-[width] duration-300 ease-out"
          style={{ width: `${successPct}%` }}
        />
        <div
          className="animate-pulse bg-blue-500 transition-[width] duration-300 ease-out"
          style={{ width: `${runningPct}%` }}
        />
        <div
          className="bg-red-500 transition-[width] duration-300 ease-out"
          style={{ width: `${failedPct}%` }}
        />
        <div
          className="bg-muted-foreground/30 transition-[width] duration-300 ease-out"
          style={{ width: `${restPct}%` }}
        />
      </div>
    </div>
  );
}

const TASK_ROW_HEIGHT = 32;

function TaskList({ tasks }: { tasks: CacheTask[] }) {
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: tasks.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => TASK_ROW_HEIGHT,
    overscan: 5,
  });

  return (
    <div ref={parentRef} className="max-h-60 overflow-y-auto border-t">
      <div
        className="relative w-full"
        style={{ height: `${virtualizer.getTotalSize()}px` }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => (
          <div
            key={virtualRow.key}
            className="absolute left-0 top-0 w-full"
            style={{
              height: `${virtualRow.size}px`,
              transform: `translateY(${virtualRow.start}px)`,
            }}
          >
            <TaskRow task={tasks[virtualRow.index]} />
          </div>
        ))}
      </div>
    </div>
  );
}

function TaskRow({ task }: { task: CacheTask }) {
  return (
    <div className="flex items-center gap-3 px-4 py-1.5 text-sm">
      <StatusDot status={task.status} />
      <span className="min-w-0 flex-1 truncate">
        {task.package_name}@{task.version}
      </span>
      <span className="text-xs text-muted-foreground">
        {statusLabel(task.status)}
      </span>
      {task.error && (
        <span className="max-w-48 truncate text-xs text-destructive">
          {task.error}
        </span>
      )}
    </div>
  );
}

function StatusDot({ status }: { status: CacheTask["status"] }) {
  const color = {
    Pending: "bg-muted-foreground",
    Downloading: "bg-blue-500 animate-pulse",
    Uploading: "bg-yellow-500 animate-pulse",
    Success: "bg-green-500",
    Failed: "bg-red-500",
    Skipped: "bg-gray-400",
  }[status];

  return <span className={`h-2 w-2 shrink-0 rounded-full ${color}`} />;
}

function statusLabel(status: CacheTask["status"]): string {
  return {
    Pending: "等待中",
    Downloading: "下载中",
    Uploading: "上传中",
    Success: "成功",
    Failed: "失败",
    Skipped: "已跳过",
  }[status];
}
