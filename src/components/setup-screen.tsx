import * as React from "react";
import { Clapperboard, Download } from "lucide-react";
import { Button, Callout, Text } from "./ui";
import { api } from "../lib/api";

/** Shown when Adobe DNG Converter (the compression engine) isn't installed. */
export function SetupScreen({ onReady }: { onReady: () => void }) {
  const [installing, setInstalling] = React.useState(false);
  const [checking, setChecking] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const handleDownload = () => {
    setError(null);
    void api.openDownloadPage();
  };

  const handleAutoInstall = async () => {
    setInstalling(true);
    setError(null);
    try {
      const result = await api.installEngine();
      if (result.success) {
        onReady();
      } else {
        setError(result.error ?? "Automatic install didn't work. Use the download button above instead.");
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Automatic install didn't work. Use the download button above instead.");
    } finally {
      setInstalling(false);
    }
  };

  const handleRecheck = async () => {
    setChecking(true);
    setError(null);
    try {
      const status = await api.checkEngine();
      if (status.installed) onReady();
      else setError("Still not detected. Make sure the installer finished, then try again.");
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="absolute inset-0 flex flex-col items-center justify-center px-8">
      <div className="flex w-full max-w-md flex-col items-center gap-5 text-center animate-soft-rise">
        <div className="flex size-14 items-center justify-center rounded-panel bg-accent/10 text-accent">
          <Clapperboard className="size-7" />
        </div>
        <div className="flex flex-col gap-2">
          <Text variant="heading1">One quick setup step</Text>
          <Text variant="regular" color="tertiary">
            This app compresses your CinemaDNG footage using Adobe DNG Converter — a free tool from Adobe. Download
            it, run the installer, then come back here.
          </Text>
        </div>

        {error ? (
          <Callout color="orange" className="w-full">
            {error}
          </Callout>
        ) : null}

        <div className="flex w-full flex-col items-center gap-2">
          <Button variant="accent" size="large" onClick={handleDownload} disabled={installing || checking}>
            <Download className="size-5" />
            Download from Adobe
          </Button>
          <Button variant="filled" size="medium" onClick={handleRecheck} disabled={checking || installing}>
            {checking ? "Checking…" : "I've installed it"}
          </Button>
        </div>

        <Button variant="transparent" size="small" onClick={handleAutoInstall} disabled={installing || checking}>
          {installing ? "Trying automatic install…" : "Try automatic install instead"}
        </Button>

        <Text variant="small" color="quaternary">
          Adobe's installer needs your Mac password, so it can only prompt you directly — that's why the download
          above is the reliable path.
        </Text>
      </div>
    </div>
  );
}
