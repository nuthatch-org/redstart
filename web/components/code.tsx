import { highlight, type Lang } from "@/lib/highlight";

export function CodeBlock({
  code,
  lang = "red",
  filename,
  className = "",
}: {
  code: string;
  lang?: Lang;
  filename?: string;
  className?: string;
}) {
  return (
    <div className={`pane overflow-hidden ${className}`}>
      {filename ? (
        <div className="flex items-center gap-2 border-b border-line px-4 py-2.5">
          <span className="flex gap-1.5">
            <i className="h-2.5 w-2.5 rounded-full bg-line-2" />
            <i className="h-2.5 w-2.5 rounded-full bg-line-2" />
            <i className="h-2.5 w-2.5 rounded-full bg-line-2" />
          </span>
          <span className="ml-1 font-mono text-xs text-faint">{filename}</span>
        </div>
      ) : null}
      <pre className="overflow-x-auto p-5 font-mono text-[0.82rem] leading-[1.65] text-text/90">
        <code dangerouslySetInnerHTML={{ __html: highlight(code, lang) }} />
      </pre>
    </div>
  );
}
