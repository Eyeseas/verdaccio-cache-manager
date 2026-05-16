import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { ManageTarget } from "./UnpublishConfirmDialog";

interface DeprecateConfirmDialogProps {
  open: boolean;
  targets: ManageTarget[];
  loading?: boolean;
  onCancel: () => void;
  onConfirm: (message: string) => void;
}

function targetLabel(t: ManageTarget): string {
  return t.version ? `${t.name}@${t.version}` : t.name;
}

export function DeprecateConfirmDialog({
  open,
  targets,
  loading,
  onCancel,
  onConfirm,
}: DeprecateConfirmDialogProps) {
  const [message, setMessage] = useState("");

  useEffect(() => {
    if (open) setMessage("");
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !loading) onCancel();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [open, loading, onCancel]);

  if (!open) return null;

  const valid = message.trim().length > 0;
  const shown = targets.slice(0, 10);
  const rest = targets.length - shown.length;

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 animate-in fade-in-0">
      <div className="mx-4 w-full max-w-md rounded-lg bg-popover p-5 text-popover-foreground shadow-lg ring-1 ring-foreground/10 animate-in zoom-in-95">
        <div className="flex items-center gap-2 text-amber-600 dark:text-amber-500">
          <AlertTriangle className="h-5 w-5" />
          <h2 className="text-lg font-semibold">确认 Deprecate</h2>
        </div>
        <p className="mt-2 text-sm text-muted-foreground">
          以下 {targets.length} 项将被标记为废弃：
        </p>
        <div className="mt-3 max-h-48 overflow-y-auto rounded-md border bg-muted/30 p-2">
          {shown.map((t) => (
            <div
              key={targetLabel(t)}
              className="truncate px-1 py-0.5 font-mono text-xs"
            >
              {targetLabel(t)}
            </div>
          ))}
          {rest > 0 && (
            <div className="px-1 py-0.5 text-xs text-muted-foreground">
              +{rest} 个
            </div>
          )}
        </div>
        <div className="mt-4">
          <label className="text-sm">废弃提示信息（必填）：</label>
          <Input
            className="mt-1.5"
            value={message}
            autoFocus
            disabled={loading}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && valid && !loading)
                onConfirm(message.trim());
            }}
            placeholder="例如：请升级到 2.x，此版本存在安全问题"
          />
        </div>
        <div className="mt-5 flex flex-nowrap justify-end gap-2">
          <Button
            variant="ghost"
            className="min-w-16 shrink-0"
            onClick={onCancel}
            disabled={loading}
          >
            取消
          </Button>
          <Button
            className="min-w-28 shrink-0"
            onClick={() => onConfirm(message.trim())}
            disabled={!valid || loading}
          >
            {loading ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                处理中...
              </>
            ) : (
              "确认标记"
            )}
          </Button>
        </div>
      </div>
    </div>,
    document.body
  );
}
