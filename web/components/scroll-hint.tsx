"use client";

import { useEffect, useRef, useState } from "react";

/** Tracks whether an element's content is wider than its box. */
export function useOverflowsX<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [overflows, setOverflows] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const check = () => setOverflows(el.scrollWidth > el.clientWidth + 1);
    check();
    const ro = new ResizeObserver(check);
    ro.observe(el);
    return () => ro.disconnect();
  });

  return [ref, overflows] as const;
}

export function ScrollHint() {
  return <span className="ml-auto shrink-0 font-mono text-xs text-muted">scroll →</span>;
}
