"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { highlight } from "@/lib/highlight";

// Bundled ABI the starter snippet imports as `ERC20`, so it type-checks fully.
const ABIS = JSON.stringify({
  ERC20: [
    { type: "event", name: "Transfer", inputs: [
      { name: "from", type: "address", indexed: true },
      { name: "to", type: "address", indexed: true },
      { name: "value", type: "uint256", indexed: false },
    ] },
    { type: "event", name: "Approval", inputs: [
      { name: "owner", type: "address", indexed: true },
      { name: "spender", type: "address", indexed: true },
      { name: "value", type: "uint256", indexed: false },
    ] },
    { type: "function", name: "balanceOf", stateMutability: "view",
      inputs: [{ name: "account", type: "address" }],
      outputs: [{ name: "", type: "uint256" }] },
  ],
});

const DEFAULT_SOURCE = `// Edit me — the panels regenerate as you type.
abi ERC20 from "./abis/ERC20.json"

entity Account {
  id: Id<Bytes>
  balance: BigInt
  label: Option<String>          // nullable — there is no \`null\`
}

entity Transfer immutable {
  id: Id<Bytes>
  from: Account
  to: Account
  value: BigInt
  timestamp: BigInt
}

source Token {
  abi: ERC20
  network: mainnet
  address: 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
  startBlock: 6082465
}

handler on Token.Transfer(event) {
  let sender = Account.loadOrCreate(event.params.from, { balance: BigInt.zero })
  let receiver = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })

  sender.balance = sender.balance - event.params.value
  receiver.balance = receiver.balance + event.params.value

  Transfer.create(event.id, {
    from: event.params.from,
    to: event.params.to,
    value: event.params.value,
    timestamp: event.block.timestamp,
  })
}
`;

type Compiled = {
  ok: boolean;
  schema: string;
  manifest: string;
  mappings: string;
  diagnostics: string[];
  warnings: string[];
};

type CompileFn = (source: string, abisJson: string) => Compiled;

const TABS = [
  { key: "mappings", label: "mappings.ts", lang: "ts" as const },
  { key: "schema", label: "schema.graphql", lang: "graphql" as const },
  { key: "manifest", label: "subgraph.yaml", lang: "yaml" as const },
] as const;

type TabKey = (typeof TABS)[number]["key"];

const EDITOR_TYPE =
  "m-0 p-5 font-mono text-[0.82rem] leading-[1.65] tracking-normal whitespace-pre";

export function Playground() {
  const compileRef = useRef<CompileFn | null>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const [source, setSource] = useState(DEFAULT_SOURCE);
  const [result, setResult] = useState<Compiled | null>(null);
  const [tab, setTab] = useState<TabKey | "diagnostics">("mappings");
  const [status, setStatus] = useState<"loading" | "ok" | "warn" | "err">("loading");
  const [statusText, setStatusText] = useState("loading engine…");

  const runWith = useCallback((compile: CompileFn, src: string) => {
    let r: Compiled;
    try {
      r = compile(src, ABIS);
    } catch (e) {
      setStatus("err");
      setStatusText("engine error");
      setResult({ ok: false, schema: "", manifest: "", mappings: "", diagnostics: [String(e)], warnings: [] });
      return;
    }
    setResult(r);
    if (r.ok) {
      setStatus(r.warnings.length ? "warn" : "ok");
      setStatusText(r.warnings.length ? `${r.warnings.length} warning(s)` : "compiled");
    } else {
      setStatus("err");
      setStatusText(`${r.diagnostics.length} error(s)`);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        // @ts-expect-error runtime asset, resolved by the browser at /wasm/…
        const mod = await import(/* webpackIgnore: true */ /* turbopackIgnore: true */ "/wasm/redstart_wasm.js");
        await mod.default();
        if (cancelled) return;
        compileRef.current = mod.compile as CompileFn;
        runWith(mod.compile as CompileFn, DEFAULT_SOURCE);
      } catch (e) {
        if (cancelled) return;
        setStatus("err");
        setStatusText("failed to load engine");
        setResult({ ok: false, schema: "", manifest: "", mappings: "", diagnostics: [String(e)], warnings: [] });
      }
    })();
    return () => { cancelled = true; };
  }, [runWith]);

  useEffect(() => {
    if (!compileRef.current) return;
    const id = setTimeout(() => {
      if (compileRef.current) runWith(compileRef.current, source);
    }, 180);
    return () => clearTimeout(id);
  }, [source, runWith]);

  const syncScroll = () => {
    const ta = taRef.current, pre = preRef.current;
    if (ta && pre) {
      pre.scrollTop = ta.scrollTop;
      pre.scrollLeft = ta.scrollLeft;
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Tab") {
      e.preventDefault();
      const ta = e.currentTarget;
      const s = ta.selectionStart, en = ta.selectionEnd;
      const next = source.slice(0, s) + "  " + source.slice(en);
      setSource(next);
      requestAnimationFrame(() => {
        ta.selectionStart = ta.selectionEnd = s + 2;
      });
    }
  };

  const errored = status === "err" && result ? result.diagnostics : null;
  const activeLang = tab === "diagnostics" ? "ts" : TABS.find((t) => t.key === tab)!.lang;
  const activeCode =
    tab === "diagnostics" || !result ? "" : result[tab as "mappings" | "schema" | "manifest"];

  return (
    <div className="grid h-full min-h-0 grid-cols-1 grid-rows-2 lg:grid-cols-2 lg:grid-rows-1">
      {/* Editor with live highlighting overlay */}
      <div className="flex min-h-0 min-w-0 flex-col border-b border-line lg:border-b-0 lg:border-r">
        <PaneHead label="source.red">
          <StatusDot status={status} text={statusText} />
        </PaneHead>
        <div className="relative min-h-0 flex-1">
          <pre
            ref={preRef}
            aria-hidden
            className={`pointer-events-none absolute inset-0 overflow-hidden text-text/90 ${EDITOR_TYPE}`}
          >
            <code dangerouslySetInnerHTML={{ __html: highlight(source, "red") + "\n" }} />
          </pre>
          <textarea
            ref={taRef}
            value={source}
            onChange={(e) => setSource(e.target.value)}
            onScroll={syncScroll}
            onKeyDown={onKeyDown}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            wrap="off"
            className={`absolute inset-0 resize-none overflow-auto border-0 bg-transparent text-transparent caret-accent outline-none ${EDITOR_TYPE}`}
          />
        </div>
      </div>

      {/* Output */}
      <div className="flex min-h-0 min-w-0 flex-col">
        <div className="flex items-center gap-1 overflow-x-auto border-b border-line px-2 py-1.5">
          {TABS.map((t) => (
            <TabButton key={t.key} active={tab === t.key} onClick={() => setTab(t.key)}>
              {t.label}
            </TabButton>
          ))}
          <TabButton active={tab === "diagnostics"} onClick={() => setTab("diagnostics")} danger={!!errored}>
            diagnostics
          </TabButton>
        </div>

        {tab === "diagnostics" ? (
          <pre className="flex-1 overflow-auto p-5 font-mono text-[0.8rem] leading-[1.7]">
            {errored ? (
              <span className="text-bad">{errored.join("\n\n")}</span>
            ) : result?.warnings.length ? (
              <span className="text-warn">{result.warnings.map((w) => `warning: ${w}`).join("\n")}</span>
            ) : (
              <span className="text-muted">No diagnostics. Clean build.</span>
            )}
          </pre>
        ) : (
          <pre className="flex-1 overflow-auto p-5 font-mono text-[0.82rem] leading-[1.65] text-text/90">
            <code dangerouslySetInnerHTML={{ __html: highlight(activeCode || "", activeLang) }} />
          </pre>
        )}
      </div>
    </div>
  );
}

function PaneHead({ label, children }: { label: string; children?: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between border-b border-line px-4 py-2 font-mono text-xs text-faint">
      <span>{label}</span>
      {children}
    </div>
  );
}

function StatusDot({ status, text }: { status: string; text: string }) {
  const color =
    status === "ok" ? "bg-ok" : status === "warn" ? "bg-warn" : status === "err" ? "bg-bad" : "bg-faint";
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={`h-1.5 w-1.5 rounded-full ${color}`} />
      {text}
    </span>
  );
}

function TabButton({
  active,
  danger,
  onClick,
  children,
}: {
  active: boolean;
  danger?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`shrink-0 rounded-md px-3 py-1 font-mono text-xs transition-colors ${
        active ? "bg-inset text-text" : danger ? "text-bad" : "text-faint hover:text-muted"
      }`}
    >
      {children}
    </button>
  );
}
