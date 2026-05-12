import { NavLink } from "react-router-dom";
import { Search, FileInput, Upload, Settings } from "lucide-react";

const navItems = [
  { to: "/", icon: Search, label: "搜索" },
  { to: "/import", icon: FileInput, label: "导入" },
  { to: "/upload", icon: Upload, label: "上传" },
  { to: "/settings", icon: Settings, label: "设置" },
];

export function Sidebar() {
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
    </aside>
  );
}
