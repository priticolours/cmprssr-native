import { Outlet } from "@tanstack/react-router";
import * as React from "react";
import { Status } from "./components/ui";
import { useTheme, useConnection, useEnvironment } from "./lib/hooks";

export function RootView() {
  useTheme();

  const connectionQuery = useConnection();
  const environmentQuery = useEnvironment();

  React.useEffect(() => {
    // Previous host IPC teardown had no equivalent here; nothing to disconnect.
  }, []);

  return (
    <div className="h-full relative">
      {/* Draggable top bar (no toolbar in this full-bleed flow) */}
      <div className="drag-region fixed top-0 left-0 right-0 h-13 z-30" />

      <Outlet />

      <div className="flex flex-col items-end gap-1 fixed bottom-2 right-2 z-40">
        {import.meta.env.DEV ? (
          <>
            {connectionQuery.error ? <Status variant="error">Backend disconnected</Status> : null}
            {environmentQuery.data ? null : <Status variant="error">Dev Server not found</Status>}
          </>
        ) : null}
      </div>
    </div>
  );
}
