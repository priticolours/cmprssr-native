import * as React from "react";

/**
 * Sync the `dark` class on <html> with the OS color scheme.
 * Replaces the previous host-injected useTheme.
 */
export function useTheme(): void {
  React.useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => document.documentElement.classList.toggle("dark", mq.matches);
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, []);
}

/**
 * The dev-only "backend disconnected" / "dev server" status widgets have
 * no equivalent in the Tauri port. These stubs keep RootView compiling while
 * hiding both indicators.
 */
export function useConnection(): { error: Error | null } {
  return { error: null };
}

export function useEnvironment(): { data: Record<string, unknown> | null } {
  return { data: {} };
}
