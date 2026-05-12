import { useEffect, useState } from "react";
import { useTaskStore, CacheTask } from "@/stores/taskStore";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ChevronUp, ChevronDown, RotateCcw, Trash2 } from "lucide-react";

export function TaskBar() {
  const { tasks, fetchTasks, startListening, retryFailed, clearCompleted } =
    useTaskStore();
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    startListening();
    fetchTasks();
  }, [startListening, fetchTasks]);

  const counts = {
    total: tasks.length,
    success: tasks.filter((t) => t.status === "Success").length,
    failed: tasks.filter((t) => t.status === "Failed").length,
    running: tasks.filter(
      (t) => t.status === "Downloading" || t.status === "Uploading"
    ).length,
    pending: tasks.filter((t) => t.status === "Pending").length,
    skipped: tasks.filter((t) => t.status === "Skipped").length,
  };

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

      {expanded && (
        <ScrollArea className="max-h-48 border-t">
          <div className="divide-y">
            {tasks.map((task) => (
              <TaskRow key={task.id} task={task} />
            ))}
          </div>
        </ScrollArea>
      )}
    </footer>
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
