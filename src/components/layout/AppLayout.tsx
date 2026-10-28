import { useEffect } from "react";
import { useSyncStore } from "@/stores/syncStore";
import { Sidebar } from "./Sidebar";
import { TaskBar } from "./TaskBar";
import { KeepAliveOutlet, type KeepAlivePage } from "./KeepAliveOutlet";
import { useAutomaticHealthChecks } from "@/hooks/useAutomaticHealthChecks";

export function AppLayout({ pages }: { pages: KeepAlivePage[] }) {
  useAutomaticHealthChecks();
  const syncStartListening = useSyncStore((s) => s.startListening);
  const syncGetInfo = useSyncStore((s) => s.getSyncInfo);

  useEffect(() => {
    syncStartListening();
    syncGetInfo();
  }, [syncStartListening, syncGetInfo]);

  return (
    <div className="flex h-screen flex-col">
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-hidden">
          <KeepAliveOutlet pages={pages} />
        </main>
      </div>
      <TaskBar />
    </div>
  );
}
