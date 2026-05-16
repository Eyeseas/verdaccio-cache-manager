import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Loader2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export interface ManageTarget {
  name: string;
  version?: string;
}

interface UnpublishConfirmDialogProps {
  open: boolean;
  targets: ManageTarget[];
  loading?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

const CONFIRM_WORD = "unpublish";

function targetLabel(t: ManageTarget): string {
  return t.version ? `${t.name}@${t.version}` : `${t.name}（整个包）`;
}

export function UnpublishConfirmDialog({
  open,
  targets,
  loading,
  onCancel,
  onConfirm,
}: UnpublishConfirmDialogProps) {
  const [text, setText] = useState("");

  useEffect(() => {
    if (open) setText("");
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

  const confirmed = text.trim() === CONFIRM_WORD;
  const shown = targets.slice(0, 10);
  const rest = targets.length - shown.length;

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 animate-in fade-in-0">
      <div className="mx-4 w-full max-w-md rounded-lg bg-popover p-5 text-popover-foreground shadow-lg ring-1 ring-foreground/10 animate-in zoom-in-95">
        <div className="flex items-center gap-2 text-destructive">
          <Trash2 className="h-5 w-5" />
          <h2 className="text-lg font-semibold">确认 Unpublish</h2>
        </div>
        <p className="mt-2 text-sm text-muted-foreground">
          以下 {targets.length} 项将被永久删除，此操作不可恢复：
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
          <label className="text-sm">
            请输入{" "}
            <code className="rounded bg-muted px-1 py-0.5 font-mono text-destructive">
              {CONFIRM_WORD}
            </code>{" "}
            以确认：
          </label>
          <Input
            className="mt-1.5"
            value={text}
            autoFocus
            disabled={loading}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && confirmed && !loading) onConfirm();
            }}
            placeholder={CONFIRM_WORD}
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
            variant="destructive"
            className="min-w-28 shrink-0"
            onClick={onConfirm}
            disabled={!confirmed || loading}
          >
            {loading ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                处理中...
              </>
            ) : (
              "确认删除"
            )}
          </Button>
        </div>
      </div>
    </div>,
    document.body
  );
}
