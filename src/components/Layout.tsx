// src/components/Layout.tsx
// TopBar + Sidebar + main content. Mounts the store sync once.
import { useState, type ReactNode } from "react";
import { TopBar } from "./Navigation/TopBar";
import { Sidebar } from "./Navigation/Sidebar";
import { useNavigationSync } from "../hooks/useNavigationSync";

export function Layout({ children }: { children: ReactNode }) {
  useNavigationSync();
  const [collapsed, setCollapsed] = useState(() => window.innerWidth < 800);

  return (
    <div className={`app ${collapsed ? "sidebar-collapsed" : ""}`}>
      <TopBar onToggleSidebar={() => setCollapsed((c) => !c)} />
      <Sidebar collapsed={collapsed} />
      <main className="main">{children}</main>
    </div>
  );
}
