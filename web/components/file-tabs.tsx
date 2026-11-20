"use client";

import { useState } from "react";
import { highlight, type Lang } from "@/lib/highlight";
import { ScrollHint, useOverflowsX } from "./scroll-hint";

export type FileSpec = { name: string; lang: Lang; code: string };

export function FileTabs({
  files,
  muted = false,
  className = "",
}: {
  files: FileSpec[];
  muted?: boolean;
  className?: string;
}) {
  const [active, setActive] = useState(0);
  const [preRef, overflows] = useOverflowsX<HTMLPreElement>();
  const file = files[active];
  return (
    <div className={`pane flex flex-col overflow-hidden ${muted ? "code-muted" : ""} ${className}`}>
      <div className="flex items-center gap-1 overflow-x-auto border-b border-line px-2 py-1.5">
        {files.map((f, i) => (
          <button
            key={f.name}
            onClick={() => setActive(i)}
            className={`shrink-0 rounded-md px-3 py-1 font-mono text-xs transition-colors ${
              i === active
                ? "bg-inset text-text"
                : "text-faint hover:text-muted"
            }`}
          >
            {f.name}
          </button>
        ))}
        {overflows ? <ScrollHint /> : null}
      </div>
      <pre
        ref={preRef}
        className={`flex-1 overflow-x-auto p-5 font-mono text-[0.82rem] leading-[1.65] ${muted ? "text-muted" : "text-text/90"}`}
      >
        <code dangerouslySetInnerHTML={{ __html: highlight(file.code, file.lang) }} />
      </pre>
    </div>
  );
}
