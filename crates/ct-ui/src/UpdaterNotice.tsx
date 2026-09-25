import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";

type State = "current" | "available" | "installing" | "error";

export default function UpdaterNotice() {
  const [state, setState] = useState<State>("current");
  const [supported, setSupported] = useState(false);
  const [update, setUpdate] = useState<Update | null>(null);
  const [message, setMessage] = useState("");
  const updateRef = useRef<Update | null>(null);
  const requestRef = useRef(0);

  const checkForUpdates = useCallback(async () => {
    const request = ++requestRef.current;
    const previous = updateRef.current;
    updateRef.current = null;
    setUpdate(null);
    if (previous) await previous.close();
    // Keep routine startup checks quiet. Only an available update should
    // interrupt the app's normal workspace view.
    setState("current");
    setMessage("");
    try {
      const found = await check();
      if (request !== requestRef.current) {
        if (found) await found.close();
        return;
      }
      updateRef.current = found;
      setUpdate(found);
      setState(found ? "available" : "current");
    } catch {
      if (request !== requestRef.current) return;
      // A release feed or network failure must not leave a permanent banner.
      // Update installation errors remain visible in install().
      setMessage("");
      setState("current");
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const timer = window.setTimeout(async () => {
      try {
        const installed = await invoke<boolean>("is_installed_build");
        if (cancelled || !installed) return;
        setSupported(true);
        await checkForUpdates();
      } catch {
        // Development and portable builds do not use the installer updater.
      }
    }, 1800);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [checkForUpdates]);

  const dismiss = () => {
    requestRef.current += 1;
    const current = updateRef.current;
    updateRef.current = null;
    setUpdate(null);
    setState("current");
    if (current) void current.close();
  };

  const install = async () => {
    if (!update) return;
    setState("installing");
    setMessage("Downloading and verifying the signed update...");
    try {
      // The Windows updater exits this process after launching the NSIS installer.
      await update.downloadAndInstall();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
      setState("error");
    }
  };

  if (!isTauri() || !supported || state === "current") return null;

  return (
    <aside className="updater-notice" role="status" aria-live="polite">
      {state === "available" && update ? (
        <>
          <div><strong>ContextTrace {update.version} is ready</strong><span>Install when convenient. The app closes while Windows installs it.</span></div>
          <button type="button" onClick={() => void install()}>Install update</button>
          <button type="button" className="updater-dismiss" onClick={dismiss}>Dismiss</button>
        </>
      ) : state === "installing" ? (
        <><strong>Updating ContextTrace</strong><span>{message}</span></>
      ) : state === "error" ? (
        <><div><strong>Update failed</strong><span>{message}</span></div><button type="button" onClick={() => void checkForUpdates()}>Retry</button></>
      ) : null}
    </aside>
  );
}
