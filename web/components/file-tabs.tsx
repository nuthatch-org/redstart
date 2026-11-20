"use client";

import { useState } from "react";
import { highlight, type Lang } from "@/lib/highlight";

export type FileSpec = { name: string; lang: Lang; code: string };

export function FileTabs({
  files,
  className = "",
}: {
  files: FileSpec[];
  className?: string;
}) {
  const [active, setActive] = useState(0);
  const file = files[active];
  return (
    <div className={`card flex flex-col overflow-hidden ${className}`}>
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
      </div>
      <pre className="flex-1 overflow-x-auto p-5 font-mono text-[0.82rem] leading-[1.65] text-text/90">
        <code dangerouslySetInnerHTML={{ __html: highlight(file.code, file.lang) }} />
      </pre>
    </div>
  );
}
