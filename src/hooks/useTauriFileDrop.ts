import { useEffect, useRef, useState, type RefObject } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

interface UseTauriFileDropOptions {
  zoneRef: RefObject<HTMLElement | null>;
  onDrop: (paths: string[]) => void | Promise<void>;
  filter?: (path: string) => boolean;
  enabled?: boolean;
}

export function useTauriFileDrop({
  zoneRef,
  onDrop,
  filter,
  enabled = true,
}: UseTauriFileDropOptions) {
  const [isOver, setIsOver] = useState(false);
  const onDropRef = useRef(onDrop);
  const filterRef = useRef(filter);

  useEffect(() => {
    onDropRef.current = onDrop;
    filterRef.current = filter;
  }, [onDrop, filter]);

  useEffect(() => {
    if (!enabled) return;

    let unlisten: (() => void) | undefined;
    let disposed = false;

    const isInsideZone = (x: number, y: number) => {
      const el = zoneRef.current;
      if (!el) return false;
      const rect = el.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      const px = x / dpr;
      const py = y / dpr;
      return (
        px >= rect.left &&
        px <= rect.right &&
        py >= rect.top &&
        py <= rect.bottom
      );
    };

    (async () => {
      try {
        const fn = await getCurrentWebview().onDragDropEvent(({ payload }) => {
          if (disposed) return;
          if (payload.type === "enter" || payload.type === "over") {
            const { x, y } = payload.position;
            setIsOver(isInsideZone(x, y));
          } else if (payload.type === "leave") {
            setIsOver(false);
          } else if (payload.type === "drop") {
            setIsOver(false);
            const { x, y } = payload.position;
            if (!isInsideZone(x, y)) return;
            const paths = filterRef.current
              ? payload.paths.filter(filterRef.current)
              : payload.paths;
            if (paths.length > 0) {
              void onDropRef.current(paths);
            }
          }
        });
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      } catch (err) {
        console.warn("无法注册 Tauri drag-drop 监听:", err);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [enabled, zoneRef]);

  return { isOver };
}
