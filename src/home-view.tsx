import { useQuery } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { Text } from "./components/ui";
import { api } from "./lib/api";
import { SetupScreen } from "./components/setup-screen";
import { Faceplate } from "./components/faceplate";

function LoadingScreen() {
  return (
    <div className="absolute inset-0 flex flex-col items-center justify-center gap-3">
      <Loader2 className="size-6 animate-spin text-tertiary" />
      <Text variant="small" color="tertiary">
        Getting things ready…
      </Text>
    </div>
  );
}

export function HomeView() {
  const engineQuery = useQuery({ queryKey: ["engine"], queryFn: api.checkEngine });

  // ── Engine gate ─────────────────────────────────────────────────────
  if (engineQuery.isLoading) return <LoadingScreen />;
  if (engineQuery.isError || (engineQuery.data && !engineQuery.data.installed)) {
    return <SetupScreen onReady={() => void engineQuery.refetch()} />;
  }

  return <Faceplate />;
}
