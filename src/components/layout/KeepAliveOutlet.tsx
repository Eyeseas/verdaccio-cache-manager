import { useState, type ReactNode } from "react";
import { useLocation } from "react-router-dom";

export interface KeepAlivePage {
  path: string;
  element: ReactNode;
}

// 页面首次访问后常驻挂载，切换时仅隐藏，避免进行中的任务状态/事件监听随卸载丢失
export function KeepAliveOutlet({ pages }: { pages: KeepAlivePage[] }) {
  const { pathname } = useLocation();
  const [visited, setVisited] = useState<Set<string>>(() => new Set([pathname]));

  if (!visited.has(pathname)) {
    setVisited(new Set(visited).add(pathname));
  }

  return (
    <>
      {pages
        .filter((page) => visited.has(page.path) || page.path === pathname)
        .map((page) => (
          <div key={page.path} hidden={page.path !== pathname} className="h-full">
            {page.element}
          </div>
        ))}
    </>
  );
}
