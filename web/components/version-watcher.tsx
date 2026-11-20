"use client";

import { useEffect, useState } from "react";

// Polls /api/version and compares the running deployment's id against the id this
// bundle was built with. When they diverge, the server has shipped a newer build
// and this tab is stale, so show a reload nudge. This is the antidote to "I fixed it
// but the browser is still running last week's JS".

const BUILD_ID = process.env.NEXT_PUBLIC_BUILD_ID || "dev";
const POLL_MS = 60_000;

export function VersionWatcher() {
  const [stale, setStale] = useState(false);

  useEffect(() => {
    // No meaningful id to compare against in local dev.
    if (BUILD_ID === "dev") return;
    let live = true;

    async function check() {
      try {
        const res = await fetch("/api/version", { cache: "no-store" });
        if (!res.ok) return;
        const { id } = (await res.json()) as { id: string };
        if (live && id && id !== "dev" && id !== BUILD_ID) setStale(true);
      } catch {
        /* offline / transient: try again next tick */
      }
    }

    check();
    const timer = setInterval(check, POLL_MS);
    const onFocus = () => check();
    window.addEventListener("focus", onFocus);
    return () => {
      live = false;
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  if (!stale) return null;

  return (
    <div className="fixed inset-x-0 bottom-0 z-50 flex items-center justify-center gap-3 border-t border-accent/40 bg-accent/10 px-4 py-2.5 backdrop-blur">
      <span className="text-sm text-text">
        A new version of The Generator is available.
      </span>
      <button onClick={() => location.reload()} className="btn">
        Reload
      </button>
    </div>
  );
}
