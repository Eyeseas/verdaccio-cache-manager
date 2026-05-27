import { NavLink } from "react-router-dom";
import { Search, FileInput, Upload, Settings, RefreshCw, FileDown } from "lucide-react";
import { useSyncStore } from "@/stores/syncStore";

const navItems = [
  { to: "/", icon: Search, label: "搜索" },
  { to: "/import", icon: FileInput, label: "导入" },
  { to: "/downgrade", icon: FileDown, label: "降级" },
  { to: "/upload", icon: Upload, label: "上传" },
  { to: "/settings", icon: Settings, label: "设置" },
];

export function Sidebar() {
  const syncStatus = useSyncStore((s) => s.status);
  const startSync = useSyncStore((s) => s.startSync);
  const isSyncing = syncStatus === "syncing";

  return (
    <aside className="flex w-16 flex-col items-center border-r bg-sidebar py-4 gap-2">
      {navItems.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          className={({ isActive }) =>
            `flex flex-col items-center justify-center w-12 h-12 rounded-lg text-xs gap-1 transition-colors ${
              isActive
                ? "bg-sidebar-accent text-sidebar-accent-foreground font-medium"
                : "text-sidebar-foreground/60 hover:text-sidebar-foreground hover:bg-sidebar-accent/50"
            }`
          }
        >
          <item.icon className="h-5 w-5" />
          <span>{item.label}</span>
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
