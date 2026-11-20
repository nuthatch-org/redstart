import type { Metadata } from "next";
import { Playground } from "@/components/playground";
import { Bird } from "@/components/logo";

export const metadata: Metadata = {
  title: "Playground",
  description:
    "Write Redstart and watch schema.graphql, subgraph.yaml, and mappings.ts generate live. The real compiler, in your browser.",
};

export default function PlaygroundPage() {
  return (
    <div className="flex h-[calc(100dvh-3.5rem)] flex-col">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-line px-5 py-2.5">
        <div className="flex items-center gap-2">
          <Bird className="h-4 w-4 text-accent" />
          <span className="text-sm font-medium">Playground</span>
          <span className="hidden text-sm text-muted sm:inline">
            The real compiler, compiled to WebAssembly. Nothing leaves your browser.
          </span>
        </div>
        <span className="font-mono text-xs text-faint">.red → AssemblyScript · live</span>
      </div>
      <div className="min-h-0 flex-1">
        <Playground />
      </div>
    </div>
  );
}
