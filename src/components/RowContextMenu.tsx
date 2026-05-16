import { useState, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import {
  Package,
  Boxes,
  Download,
  Box,
  ChevronRight,
  Trash2,
  AlertTriangle,
} from "lucide-react";
import { positionContextMenu } from "./rowContextMenuLogic";

export interface ContextMenuPosition {
  x: number;
  y: number;
}

export interface RowContextMenuProps {
  position: ContextMenuPosition;
  onClose: () => void;
  onCache?: () => void;
  onCacheWithDeps?: () => void;
  onExportTarball?: () => void;
  onExportTarballWithDeps?: () => void;
  cacheDisabled?: boolean;
  exportDisabled?: boolean;
  showManageActions?: boolean;
  onUnpublish?: () => void;
  onDeprecate?: () => void;
  onUnpublishAll?: () => void;
}

const itemClass =
  "relative flex w-full cursor-default items-center gap-1.5 rounded-md px-1.5 py-1 text-sm outline-hidden select-none hover:bg-accent hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-50";

export function RowContextMenu({
  position,
  onClose,
  onCache,
  onCacheWithDeps,
  onExportTarball,
  onExportTarballWithDeps,
  cacheDisabled,
  exportDisabled,
  showManageActions,
  onUnpublish,
  onDeprecate,
  onUnpublishAll,
}: RowContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const subMenuRef = useRef<HTMLDivElement>(null);
  const [subOpen, setSubOpen] = useState(false);
  const [layout, setLayout] = useState({
    left: position.x,
    top: position.y,
    submenuSide: "right" as "left" | "right",
  });
  const subTimeoutRef = useRef<number | null>(null);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const handleScroll = () => onClose();
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };

    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("scroll", handleScroll, true);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("scroll", handleScroll, true);
      document.removeEventListener("keydown", handleKeyDown);
      if (subTimeoutRef.current) clearTimeout(subTimeoutRef.current);
    };
  }, [onClose]);

  useEffect(() => {
    if (!menuRef.current) return;
    const menuRect = menuRef.current.getBoundingClientRect();
    const subMenuRect = subMenuRef.current?.getBoundingClientRect();
    setLayout(
      positionContextMenu({
        anchor: position,
        menuSize: { width: menuRect.width, height: menuRect.height },
        submenuSize: {
          width: subMenuRect?.width ?? 176,
          height: subMenuRect?.height ?? 80,
        },
        viewport: {
          width: window.innerWidth,
          height: window.innerHeight,
        },
      })
    );
  }, [position, subOpen]);

  const handleAction = (action?: () => void) => {
    if (!action) return;
    onClose();
    action();
  };

  const openSub = () => {
    if (subTimeoutRef.current) clearTimeout(subTimeoutRef.current);
    setSubOpen(true);
  };
  const closeSub = () => {
    subTimeoutRef.current = window.setTimeout(() => setSubOpen(false), 150);
  };
  const keepSub = () => {
    if (subTimeoutRef.current) clearTimeout(subTimeoutRef.current);
  };

  return createPortal(
    <div
      ref={menuRef}
      className="fixed z-50 min-w-40 rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10 animate-in fade-in-0 zoom-in-95"
      style={{ top: layout.top, left: layout.left }}
    >
      {onCache && (
        <button
          className={itemClass}
          onClick={() => handleAction(onCache)}
          disabled={cacheDisabled}
        >
          <Package className="h-4 w-4" />
          缓存
        </button>
      )}
      {onCacheWithDeps && (
        <button
          className={itemClass}
          onClick={() => handleAction(onCacheWithDeps)}
          disabled={cacheDisabled}
        >
          <Boxes className="h-4 w-4" />
          缓存及依赖
        </button>
      )}
      {(onCache || onCacheWithDeps) &&
        (onExportTarball || onExportTarballWithDeps) && (
          <div className="-mx-1 my-1 h-px bg-border" />
        )}
      {(onExportTarball || onExportTarballWithDeps) && (
      <div
        className="relative"
        onMouseEnter={openSub}
        onMouseLeave={closeSub}
      >
        <button
          className={`${itemClass} justify-between`}
          disabled={exportDisabled}
          onClick={openSub}
        >
          <span className="flex items-center gap-1.5">
            <Download className="h-4 w-4" />
            导出 Tarball
          </span>
          <ChevronRight className="h-3 w-3 text-muted-foreground" />
        </button>
        {subOpen && (
          <div
            ref={subMenuRef}
            className={`absolute top-0 z-50 min-w-44 whitespace-nowrap rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10 animate-in fade-in-0 zoom-in-95 ${
              layout.submenuSide === "right"
                ? "left-full ml-1"
                : "right-full mr-1"
            }`}
            onMouseEnter={keepSub}
            onMouseLeave={closeSub}
          >
            {onExportTarball && (
              <button
                className={itemClass}
                onClick={() => handleAction(onExportTarball)}
                disabled={exportDisabled}
              >
                <Box className="h-4 w-4" />
                仅导出该包
              </button>
            )}
            {onExportTarballWithDeps && (
              <button
                className={itemClass}
                onClick={() => handleAction(onExportTarballWithDeps)}
                disabled={exportDisabled}
              >
                <Boxes className="h-4 w-4" />
                导出该包及其依赖
              </button>
            )}
          </div>
        )}
      </div>
      )}
      {showManageActions &&
        (onUnpublish || onDeprecate || onUnpublishAll) && (
          <>
            {(onCache ||
              onCacheWithDeps ||
              onExportTarball ||
              onExportTarballWithDeps) && (
              <div className="-mx-1 my-1 h-px bg-border" />
            )}
            {onUnpublish && (
              <button
                className={`${itemClass} text-destructive hover:bg-destructive/10 hover:text-destructive`}
                onClick={() => handleAction(onUnpublish)}
              >
                <Trash2 className="h-4 w-4" />
                Unpublish 此版本
              </button>
            )}
            {onDeprecate && (
              <button
                className={`${itemClass} text-amber-600 hover:bg-amber-500/10 hover:text-amber-600 dark:text-amber-500 dark:hover:text-amber-500`}
                onClick={() => handleAction(onDeprecate)}
              >
                <AlertTriangle className="h-4 w-4" />
                Deprecate 此版本
              </button>
            )}
            {onUnpublishAll && (
              <button
                className={`${itemClass} text-destructive hover:bg-destructive/10 hover:text-destructive`}
                onClick={() => handleAction(onUnpublishAll)}
              >
                <Trash2 className="h-4 w-4" />
                Unpublish 整个包
              </button>
            )}
          </>
        )}
    </div>,
    document.body
  );
}
