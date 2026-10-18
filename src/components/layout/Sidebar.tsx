import { NavLink } from "react-router-dom";
import { Search, FileInput, Upload, Settings, RefreshCw, FileDown, Stethoscope } from "lucide-react";
import { useSyncStore } from "@/stores/syncStore";
import { healthIndicator, useHealthStore } from "@/stores/healthStore";

const navItems = [
  { to: "/", icon: Search, label: "搜索" },
  { to: "/import", icon: FileInput, label: "导入" },
  { to: "/downgrade", icon: FileDown, label: "降级" },
  { to: "/upload", icon: Upload, label: "上传" },
  { to: "/health", icon: Stethoscope, label: "体检" },
  { to: "/preflight", icon: FileInput, label: "预检" },
  { to: "/settings", icon: Settings, label: "设置" },
];

export function Sidebar() {
  const indicator = healthIndicator(useHealthStore((s) => s.latest));
  const syncStatus = useSyncStore((s) => s.status);
  const startSync = useSyncStore((s) => s.startSync);
  const isSyncing = syncStatus === "syncing";

  return (
    <aside className="flex w-16 flex-col items-center border-r bg-sidebar py-4 gap-2">
      {navItems.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          title={item.to === "/health" ? indicator.title : undefined}
          className={({ isActive }) =>
            `relative flex flex-col items-center justify-center w-12 h-12 rounded-lg text-xs gap-1 transition-colors ${
              isActive
                ? "bg-sidebar-accent text-sidebar-accent-foreground font-medium"
                : "text-sidebar-foreground/60 hover:text-sidebar-foreground hover:bg-sidebar-accent/50"
            }`
          }
        >
          <item.icon className="h-5 w-5" />
          <span>{item.label}</span>
          {item.to === "/health" && (indicator.warning || indicator.count > 0) && (
            <span aria-label={indicator.title} className={`absolute -right-1 -top-1 min-w-4 rounded-full px-1 text-[10px] leading-4 ${indicator.warning ? "bg-yellow-500 text-black" : "bg-destructive text-white"}`}>
              {indicator.count || "!"}
            </span>
          )}
        </NavLink>
      ))}
      <button
        type="button"
        onClick={startSync}
        disabled={isSyncing}
        title={isSyncing ? "同步中..." : "同步缓存索引"}
        className="mt-auto flex flex-col items-center justify-center w-12 h-12 rounded-lg text-xs gap-1 transition-colors text-sidebar-foreground/60 hover:text-sidebar-foreground hover:bg-sidebar-accent/50 disabled:opacity-60 disabled:hover:bg-transparent disabled:hover:text-sidebar-foreground/60"
      >
        <RefreshCw className={`h-5 w-5 ${isSyncing ? "animate-spin" : ""}`} />
        <span>{isSyncing ? "同步中" : "同步"}</span>
      </button>
    </aside>
  );
}
