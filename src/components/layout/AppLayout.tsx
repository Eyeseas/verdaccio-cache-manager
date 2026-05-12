import { Outlet } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { TaskBar } from "./TaskBar";

export function AppLayout() {
  return (
    <div className="flex h-screen flex-col">
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-hidden">
          <Outlet />
        </main>
      </div>
      <TaskBar />
    </div>
  );
}
