import { BrowserRouter, Routes, Route } from "react-router-dom";
import { AppLayout } from "./components/layout/AppLayout";
import type { KeepAlivePage } from "./components/layout/KeepAliveOutlet";
import { SearchPage } from "./pages/SearchPage";
import { ImportPage } from "./pages/ImportPage";
import { PackageJsonDowngradePage } from "./pages/PackageJsonDowngradePage";
import { UploadPage } from "./pages/UploadPage";
import { SettingsPage } from "./pages/SettingsPage";
import { StorageHealthPage } from "./pages/StorageHealthPage";
import { InstallPreflightPage } from "./pages/InstallPreflightPage";
import { Toaster } from "./components/ui/sonner";

const pages: KeepAlivePage[] = [
  { path: "/", element: <SearchPage /> },
  { path: "/import", element: <ImportPage /> },
  { path: "/downgrade", element: <PackageJsonDowngradePage /> },
  { path: "/upload", element: <UploadPage /> },
  { path: "/health", element: <StorageHealthPage /> },
  { path: "/preflight", element: <InstallPreflightPage /> },
  { path: "/settings", element: <SettingsPage /> },
];

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="*" element={<AppLayout pages={pages} />} />
      </Routes>
      <Toaster position="bottom-right" />
    </BrowserRouter>
  );
}
