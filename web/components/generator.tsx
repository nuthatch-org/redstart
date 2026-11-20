"use client";

import { useEffect, useMemo, useState } from "react";
import { CHAINS, DEFAULT_CHAIN_ID } from "@/lib/chains";
import {
  type AnthropicError,
  fixSubgraph,
  generateSubgraph,
  type GeneratedSubgraph,
} from "@/lib/generate";
import { sanitizeName } from "@/lib/generate";
import { connectGitHub, type CreatedRepo, createSubgraphRepo } from "@/lib/github";
import { highlight } from "@/lib/highlight";
import { downloadProject, projectFiles, type VerifyResult, verifyProject } from "@/lib/project";

// GitHub OAuth client id (public). Inlined at build time: a NEXT_PUBLIC change
// requires a fresh compile of this module (touch the source to bust the cache).
const GITHUB_CLIENT_ID = process.env.NEXT_PUBLIC_GITHUB_CLIENT_ID;
// Public MCP endpoint: the "use your Claude plan" power path.
const MCP_URL = "https://mcp.89.167.109.4.sslip.io/mcp";
import type { ContractInfo, EventDef } from "@/lib/subgraph-abi";

const KEY_STORAGE = "redstart.anthropicKey";
const STATE_STORAGE = "redstart.generator.v1";

const KIND_LABEL: Record<ContractInfo["kind"], string> = {
  erc20: "ERC-20 token",
  erc721: "ERC-721 NFT",
  erc1155: "ERC-1155 multi-token",
  unknown: "contract",
};

export function Generator() {
  const [address, setAddress] = useState("");
  const [chainId, setChainId] = useState(DEFAULT_CHAIN_ID);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [contract, setContract] = useState<ContractInfo | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());

  // BYOK: the Anthropic key lives only in sessionStorage and is never sent to us.
  const [apiKey, setApiKey] = useState("");
  const [generating, setGenerating] = useState(false);
  const [genError, setGenError] = useState<string | null>(null);
  const [generated, setGenerated] = useState<GeneratedSubgraph | null>(null);

  const [hydrated, setHydrated] = useState(false);

  useEffect(() => {
    // Hydrate the key from sessionStorage on mount (client-only; not available
    // during SSR, so this can't be a lazy initializer without a hydration mismatch).
    /* eslint-disable react-hooks/set-state-in-effect */
    setApiKey(sessionStorage.getItem(KEY_STORAGE) ?? "");
    // Restore the working session (contract, events, generated files) so a refresh
    // or dropped connection doesn't start from scratch.
    try {
      const raw = localStorage.getItem(STATE_STORAGE);
      if (raw) {
        const s = JSON.parse(raw);
        if (s.address) setAddress(s.address);
        if (s.chainId) setChainId(s.chainId);
        if (s.contract) setContract(s.contract);
        if (Array.isArray(s.selected)) setSelected(new Set(s.selected));
        if (s.generated) setGenerated(s.generated);
      }
    } catch {
      /* ignore corrupt snapshot */
    }
    setHydrated(true);
    /* eslint-enable react-hooks/set-state-in-effect */
  }, []);

  // Persist the working session whenever it changes (after hydration, so the
  // empty first render doesn't clobber a saved snapshot).
  useEffect(() => {
    if (!hydrated) return;
    try {
      localStorage.setItem(
        STATE_STORAGE,
        JSON.stringify({ address, chainId, contract, selected: [...selected], generated }),
      );
    } catch {
      /* quota / serialization: non-fatal */
    }
  }, [hydrated, address, chainId, contract, selected, generated]);

  function startOver() {
    localStorage.removeItem(STATE_STORAGE);
    setContract(null);
    setGenerated(null);
    setSelected(new Set());
    setError(null);
    setGenError(null);
  }

  function saveKey(k: string) {
    setApiKey(k);
    if (k) sessionStorage.setItem(KEY_STORAGE, k);
    else sessionStorage.removeItem(KEY_STORAGE);
  }

  async function generate() {
    if (!contract || generating || !apiKey) return;
    setGenerating(true);
    setGenError(null);
    setGenerated(null);
    const events = contract.events.filter((e) => selected.has(e.signature));
    try {
      const result = await generateSubgraph(apiKey, contract, events);
      setGenerated(result);
    } catch (e) {
      setGenError((e as AnthropicError).message ?? "Generation failed.");
    } finally {
      setGenerating(false);
    }
  }

  const canLookup = /^0x[a-fA-F0-9]{40}$/.test(address.trim());

  async function lookup() {
    if (!canLookup || loading) return;
    setLoading(true);
    setError(null);
    setContract(null);
    setGenerated(null);
    try {
      const res = await fetch(
        `/api/contract?address=${encodeURIComponent(address.trim())}&chainId=${chainId}`,
      );
      const data = await res.json();
      if (!res.ok) {
        setError(data.error ?? "Lookup failed.");
        return;
      }
      const info = data as ContractInfo;
      setContract(info);
      setSelected(new Set(info.events.map((e) => e.signature)));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  function toggle(sig: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(sig)) next.delete(sig);
      else next.add(sig);
      return next;
    });
  }

  return (
    <div className="mx-auto w-full max-w-3xl px-4 py-10 sm:py-14">
      <header className="mb-8">
        <span className="eyebrow">The Generator</span>
        <h1 className="display mt-3 text-3xl sm:text-4xl">
          Paste a contract. Get a tested subgraph.
        </h1>
        <p className="mt-3 max-w-xl text-muted">
          Point it at any verified contract. It researches the ABI, proxies and start
          block, then generates best-practices AssemblyScript, verified before you
          ever see it. Your AI, your repo, your keys.
        </p>
      </header>

      {/* Input row */}
      <div className="card p-4">
        <label className="mb-1.5 block font-mono text-xs text-faint">CONTRACT ADDRESS</label>
        <div className="flex flex-col gap-2 sm:flex-row">
          <input
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && lookup()}
            placeholder="0x…"
            spellCheck={false}
            className="min-w-0 flex-1 rounded-lg border border-line-2 bg-elev px-3 py-2.5 font-mono text-sm text-text outline-none placeholder:text-faint focus:border-accent"
          />
          <select
            value={chainId}
            onChange={(e) => setChainId(Number(e.target.value))}
            className="rounded-lg border border-line-2 bg-elev px-3 py-2.5 text-sm text-text outline-none focus:border-accent"
          >
            {CHAINS.map((c) => (
              <option key={c.id} value={c.id}>
                {c.label}
              </option>
            ))}
          </select>
          <button
            onClick={lookup}
            disabled={!canLookup || loading}
            className="btn justify-center disabled:cursor-not-allowed disabled:opacity-40"
          >
            {loading ? "Researching…" : "Research contract"}
          </button>
        </div>
      </div>

      {error && (
        <div className="mt-4 rounded-lg border border-bad/40 bg-bad/10 px-4 py-3 text-sm text-bad">
          {error}
        </div>
      )}

      {contract && (
        <div className="mt-3 text-right">
          <button
            onClick={startOver}
            className="font-mono text-xs text-faint transition-colors hover:text-accent"
          >
            start over ✕
          </button>
        </div>
      )}

      {contract && (
        <ContractPanel
          contract={contract}
          selected={selected}
          toggle={toggle}
          apiKey={apiKey}
          onKey={saveKey}
          onGenerate={generate}
          generating={generating}
          genError={genError}
          generated={generated}
          onFilesUpdate={setGenerated}
        />
      )}
    </div>
  );
}

function ContractPanel({
  contract,
  selected,
  toggle,
  apiKey,
  onKey,
  onGenerate,
  generating,
  genError,
  generated,
  onFilesUpdate,
}: {
  contract: ContractInfo;
  selected: Set<string>;
  toggle: (sig: string) => void;
  apiKey: string;
  onKey: (k: string) => void;
  onGenerate: () => void;
  generating: boolean;
  genError: string | null;
  generated: GeneratedSubgraph | null;
  onFilesUpdate: (f: GeneratedSubgraph) => void;
}) {
  const explorer = useMemo(() => explorerLink(contract), [contract]);
  const canGenerate = apiKey.trim().length > 0 && selected.size > 0 && !generating;

  return (
    <div className="mt-6 space-y-5">
      {/* Summary */}
      <div className="card p-4">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span className="font-medium text-text">{contract.name}</span>
          <span className="tag">{KIND_LABEL[contract.kind]}</span>
          {contract.verified && <Badge tone="green">verified</Badge>}
          {contract.isProxy && <Badge tone="amber">proxy → impl ABI</Badge>}
        </div>
        <dl className="mt-3 grid grid-cols-1 gap-x-6 gap-y-1.5 font-mono text-xs text-muted sm:grid-cols-2">
          <Row label="network" value={contract.graphNetwork} />
          <Row
            label="startBlock"
            value={contract.startBlock != null ? String(contract.startBlock) : "unknown"}
          />
          {contract.implementation && (
            <Row label="implementation" value={shorten(contract.implementation)} />
          )}
          <Row
            label="address"
            value={
              <a href={explorer} target="_blank" rel="noopener noreferrer" className="hover:text-accent">
                {shorten(contract.address)} ↗
              </a>
            }
          />
        </dl>
      </div>

      {/* Event selection */}
      <div className="card p-4">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-medium">Events to index</h2>
          <span className="font-mono text-xs text-faint">
            {selected.size}/{contract.events.length} selected
          </span>
        </div>
        {contract.events.length === 0 ? (
          <p className="text-sm text-muted">
            This ABI declares no events. Event handlers are the fast, portable path. A
            contract with no events needs call/block handlers, which is a later feature.
          </p>
        ) : (
          <ul className="space-y-1">
            {contract.events.map((ev) => (
              <EventRow
                key={ev.signature}
                event={ev}
                checked={selected.has(ev.signature)}
                onToggle={() => toggle(ev.signature)}
              />
            ))}
          </ul>
        )}
      </div>

      {/* Connect AI + generate */}
      <div className="card p-4">
        <div className="mb-1.5 flex items-center justify-between">
          <label className="font-mono text-xs text-faint">ANTHROPIC API KEY</label>
          <span className="font-mono text-[0.68rem] text-faint">
            stored in your browser only · never sent to us
          </span>
        </div>
        <div className="flex flex-col gap-2 sm:flex-row">
          <input
            value={apiKey}
            onChange={(e) => onKey(e.target.value)}
            type="password"
            placeholder="sk-ant-…"
            spellCheck={false}
            autoComplete="off"
            className="min-w-0 flex-1 rounded-lg border border-line-2 bg-elev px-3 py-2.5 font-mono text-sm text-text outline-none placeholder:text-faint focus:border-accent"
          />
          <button
            onClick={onGenerate}
            disabled={!canGenerate}
            className="btn justify-center disabled:cursor-not-allowed disabled:opacity-40"
          >
            {generating ? "Generating…" : "Generate subgraph"}
          </button>
        </div>
        <p className="mt-2 text-xs text-muted">
          Your key calls Claude (<span className="text-text">Opus 4.8</span>) directly from this
          page. Generation runs on your
          account and typically costs <span className="text-text">under $0.50 per subgraph</span>.
          Get a key at{" "}
          <a
            href="https://console.anthropic.com/settings/keys"
            target="_blank"
            rel="noopener noreferrer"
            className="text-accent hover:underline"
          >
            console.anthropic.com
          </a>
          .
        </p>

        <details className="mt-3 border-t border-line pt-3">
          <summary className="cursor-pointer text-xs text-accent hover:underline">
            No API key? Use your Claude subscription instead →
          </summary>
          <div className="mt-2 space-y-2 text-xs text-muted">
            <p>
              Connect The Generator&apos;s <span className="text-text">MCP server</span> to{" "}
              <span className="text-text">Claude Code</span> or{" "}
              <span className="text-text">Claude Desktop</span> and generate from inside your
              own Claude. The inference runs on <span className="text-text">your Max/Pro plan</span>,
              with no API key and no per-token cost.
            </p>
            <p className="text-muted">Claude Code, one command:</p>
            <div className="flex items-center gap-2">
              <code className="flex-1 overflow-x-auto rounded-md border border-line-2 bg-elev px-2.5 py-2 font-mono text-[0.7rem] text-text">
                claude mcp add --transport http redstart {MCP_URL}
              </code>
              <CopyButton text={`claude mcp add --transport http redstart ${MCP_URL}`} />
            </div>
            <p>
              Then ask Claude:{" "}
              <span className="text-muted">
                &ldquo;use the redstart tools to build a subgraph for &lt;address&gt;: fetch the
                contract, read the best-practices resource, then compile_subgraph until it&apos;s
                green.&rdquo;
              </span>{" "}
              Your Claude does the work; the server gives it contract research and the real compile
              gate.
            </p>
            <p className="text-faint">
              Works best in Claude Code / Desktop. claude.ai web can be flaky at surfacing custom
              connectors.
            </p>
          </div>
        </details>
      </div>

      {generating && <GeneratingPanel />}

      {genError && (
        <div className="rounded-lg border border-bad/40 bg-bad/10 px-4 py-3 text-sm text-bad">
          {genError}
        </div>
      )}

      {generated && (
        <FilesPanel
          files={generated}
          contract={contract}
          apiKey={apiKey}
          onFilesUpdate={onFilesUpdate}
        />
      )}
    </div>
  );
}

const FILE_TABS = [
  { key: "schema", label: "schema.graphql", lang: "graphql" },
  { key: "manifest", label: "subgraph.yaml", lang: "yaml" },
  { key: "mappings", label: "src/mapping.ts", lang: "ts" },
  { key: "tests", label: "tests/…test.ts", lang: "ts" },
] as const;

function FilesPanel({
  files,
  contract,
  apiKey,
  onFilesUpdate,
}: {
  files: GeneratedSubgraph;
  contract: ContractInfo;
  apiKey: string;
  onFilesUpdate: (f: GeneratedSubgraph) => void;
}) {
  const [tab, setTab] = useState<(typeof FILE_TABS)[number]["key"]>("schema");
  const [verifying, setVerifying] = useState(false);
  const [verdict, setVerdict] = useState<VerifyResult | null>(null);
  const [autoFixing, setAutoFixing] = useState(false);
  const [fixStatus, setFixStatus] = useState<string | null>(null);
  const [fixError, setFixError] = useState<string | null>(null);
  const [ghBusy, setGhBusy] = useState(false);
  const [ghPhase, setGhPhase] = useState<"connect" | "push" | null>(null);
  const [ghError, setGhError] = useState<string | null>(null);
  const [ghRepo, setGhRepo] = useState<CreatedRepo | null>(null);
  const [repoName, setRepoName] = useState(
    () => `${sanitizeName(contract.name).toLowerCase()}-subgraph`,
  );
  const body = files[tab];
  const lang = FILE_TABS.find((t) => t.key === tab)!.lang;
  const MAX_FIX = 4;

  // Feed the compiler/test errors back to Claude and iterate until it verifies
  // (or we hit the attempt cap). Drives verify itself, so the auto-verify effect
  // below is suppressed while this runs.
  async function runFix() {
    if (autoFixing) return;
    if (!apiKey.trim()) {
      setFixError("Add your Anthropic key above to auto-fix.");
      return;
    }
    setFixError(null);
    setAutoFixing(true);
    let cur = files;
    let log = verdict?.log ?? verdict?.error ?? "";
    try {
      for (let i = 1; i <= MAX_FIX; i++) {
        setFixStatus(`Fixing with Claude, attempt ${i} of ${MAX_FIX}…`);
        const next = await fixSubgraph(apiKey, contract, cur, log);
        onFilesUpdate(next);
        cur = next;
        setFixStatus(`Re-checking attempt ${i}…`);
        setVerifying(true);
        const res = await verifyProject(next, contract);
        setVerdict(res);
        setVerifying(false);
        if (res.ok) {
          setFixStatus(null);
          return;
        }
        log = res.log ?? res.error ?? "";
        if (i === MAX_FIX) {
          setFixStatus(`Still failing after ${MAX_FIX} attempts. You can try again or tweak the files.`);
        }
      }
    } catch (e) {
      setFixError((e as { message?: string }).message ?? String(e));
      setVerifying(false);
    } finally {
      setAutoFixing(false);
    }
  }

  async function createRepo() {
    if (!GITHUB_CLIENT_ID || ghBusy || !repoName.trim()) return;
    setGhError(null);
    setGhBusy(true);
    setGhPhase("connect");
    try {
      // A popup opens on GitHub; the user signs in (if needed) and clicks Authorize.
      // The button says "Waiting for GitHub…" so this isn't mistaken for a hang.
      const token = await connectGitHub(GITHUB_CLIENT_ID);
      setGhPhase("push");
      const repo = await createSubgraphRepo(
        token,
        repoName.trim(),
        projectFiles(files, contract),
        `${contract.name} subgraph, generated by The Generator (redstart-lang.com)`,
      );
      setGhRepo(repo);
    } catch (e) {
      const msg = (e as Error).message;
      // The popup-closed case almost always means the sign-in/Authorize wasn't
      // finished, so say so rather than the bare "Window closed".
      setGhError(
        /window closed/i.test(msg)
          ? "Authorization cancelled. In the GitHub popup, sign in and click Authorize (grant repository access), then try again."
          : msg,
      );
    } finally {
      setGhBusy(false);
      setGhPhase(null);
    }
  }

  // Auto-run the compile gate as soon as files are generated. A fetch-on-mount
  // with a loading flag is the intended use of an effect here.
  /* eslint-disable react-hooks/set-state-in-effect */
  useEffect(() => {
    // The fix loop drives its own verify; don't double-run when it swaps files.
    if (autoFixing) return;
    let live = true;
    setVerdict(null);
    setVerifying(true);
    verifyProject(files, contract)
      .then((r) => live && setVerdict(r))
      .catch((e) => live && setVerdict({ ok: false, stage: "error", error: String(e?.message ?? e) }))
      .finally(() => live && setVerifying(false));
    return () => {
      live = false;
    };
    // autoFixing is read from the closure (files-change re-runs the effect during
    // the loop); adding it as a dep would fire a wasteful extra verify on loop end.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [files, contract]);
  /* eslint-enable react-hooks/set-state-in-effect */

  return (
    <div className="card overflow-hidden">
      <VerifyBanner verifying={verifying} verdict={verdict} />
      {verdict && !verdict.ok && verdict.stage !== "config" && (
        <div className="flex flex-wrap items-center gap-3 border-b border-line bg-bad/10 px-4 py-2.5">
          <button onClick={runFix} disabled={autoFixing || verifying} className="btn disabled:opacity-50">
            {autoFixing ? (
              <span className="flex items-center gap-2">
                <span className="h-3 w-3 motion-safe:animate-spin rounded-full border-2 border-on-accent/30 border-t-on-accent" />
                Fixing…
              </span>
            ) : (
              "⚡ Fix issues"
            )}
          </button>
          <span className="text-xs text-muted">
            {fixStatus ?? "Send the errors back to Claude and auto-fix until it verifies."}
          </span>
        </div>
      )}
      {fixError && (
        <p className="border-b border-line px-4 py-2 text-xs text-bad">Auto-fix: {fixError}</p>
      )}
      {files.notes && (
        <p className="border-b border-line px-4 py-3 text-sm text-muted">
          <span className="mr-1.5 text-accent">⚡</span>
          {files.notes}
        </p>
      )}
      <div className="flex items-center justify-between border-b border-line px-2">
        <div className="flex">
          {FILE_TABS.map((t) => (
            <button
              key={t.key}
              onClick={() => setTab(t.key)}
              className={`px-3 py-2.5 font-mono text-xs transition-colors ${
                tab === t.key ? "text-accent" : "text-faint hover:text-muted"
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>
        <CopyButton text={body} />
      </div>
      <pre
        className="max-h-[28rem] overflow-auto p-4 font-mono text-xs leading-relaxed text-text"
        dangerouslySetInnerHTML={{ __html: highlight(body, lang) }}
      />
      <div className="border-t border-line px-4 py-3">
        {ghRepo ? (
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-sm text-ok">
              ✓ Repo created:{" "}
              <a
                href={ghRepo.url}
                target="_blank"
                rel="noopener noreferrer"
                className="font-mono text-ok hover:underline"
              >
                {ghRepo.fullName} ↗
              </a>
              {ghRepo.workflowSkipped && (
                <span className="ml-2 text-xs text-muted">
                  (CI workflow skipped. It&apos;s in the .zip; reconnect and approve
                  &ldquo;workflow&rdquo; to include it)
                </span>
              )}
            </span>
            <button
              onClick={() => downloadProject(files, contract)}
              className="btn btn-ghost"
            >
              Download .zip
            </button>
          </div>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-xs text-muted">
              A complete project: schema, manifest, mappings, tests, CI. Ejects to the
              canonical toolchain unchanged.
            </span>
            <div className="flex items-center gap-2">
              <button
                onClick={() => downloadProject(files, contract)}
                className="btn btn-ghost"
              >
                Download .zip
              </button>
              {GITHUB_CLIENT_ID && (
                <>
                  <input
                    value={repoName}
                    onChange={(e) => setRepoName(e.target.value)}
                    aria-label="Repository name"
                    spellCheck={false}
                    className="w-44 rounded-lg border border-line-2 bg-elev px-2.5 py-2 font-mono text-xs text-text outline-none focus:border-accent"
                  />
                  <button
                    onClick={createRepo}
                    disabled={ghBusy || !repoName.trim()}
                    className="btn disabled:opacity-50"
                  >
                    {ghPhase === "connect"
                      ? "Waiting for GitHub…"
                      : ghPhase === "push"
                        ? "Creating repo…"
                        : "Create GitHub repo"}
                  </button>
                </>
              )}
            </div>
          </div>
        )}
        {ghPhase === "connect" && (
          <p className="mt-2 text-xs text-muted">
            A GitHub popup opened. Sign in if needed and click <strong>Authorize</strong> (grant
            repository access). Keep it open until it closes itself.
          </p>
        )}
        {ghError && (
          <p className="mt-2 text-xs text-bad">GitHub: {ghError}</p>
        )}
      </div>
    </div>
  );
}

const GEN_STEPS = [
  "Reading the ABI & events",
  "Modelling entities & relations",
  "Writing AssemblyScript handlers",
  "Generating Matchstick tests",
  "Finalising the subgraph",
];

function GeneratingPanel() {
  const [step, setStep] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setStep((s) => (s < GEN_STEPS.length - 1 ? s + 1 : s)), 5000);
    return () => clearInterval(t);
  }, []);

  return (
    <div className="card overflow-hidden">
      <div className="h-[3px] w-full overflow-hidden bg-line">
        <div className="loadbar h-full w-1/4 rounded-full bg-accent" />
      </div>
      <div className="p-5">
        <div className="mb-4 flex items-center gap-3">
          <span className="h-4 w-4 shrink-0 motion-safe:animate-spin rounded-full border-2 border-accent/25 border-t-accent" />
          <div>
            <p className="text-sm text-text">Generating with Claude Opus 4.8…</p>
            <p className="text-xs text-faint">
              Writing best-practices AssemblyScript, usually in 20–60s.
            </p>
          </div>
        </div>
        <ul className="space-y-2">
          {GEN_STEPS.map((s, i) => (
            <li key={s} className="flex items-center gap-2.5 text-sm">
              <span className="flex h-4 w-4 shrink-0 items-center justify-center">
                {i < step ? (
                  <span className="text-ok">✓</span>
                ) : i === step ? (
                  <span className="relative flex h-2 w-2">
                    <span className="absolute inline-flex h-full w-full motion-safe:animate-ping rounded-full bg-accent opacity-75" />
                    <span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
                  </span>
                ) : (
                  <span className="h-2 w-2 rounded-full border border-line-2" />
                )}
              </span>
              <span className={i <= step ? "text-muted" : "text-faint"}>{s}</span>
            </li>
          ))}
        </ul>
        <div className="mt-5 space-y-2">
          <div className="skel h-3 w-1/3" />
          <div className="skel h-3 w-2/3" />
          <div className="skel h-3 w-1/2" />
          <div className="skel h-3 w-3/5" />
        </div>
      </div>
    </div>
  );
}

function VerifyBanner({
  verifying,
  verdict,
}: {
  verifying: boolean;
  verdict: VerifyResult | null;
}) {
  if (verifying) {
    return (
      <div className="border-b border-line">
        <div className="h-[3px] w-full overflow-hidden bg-line">
          <div className="loadbar h-full w-1/4 rounded-full bg-accent" />
        </div>
        <div className="flex items-center gap-2.5 bg-inset px-4 py-2.5 text-sm text-muted">
          <span className="h-3.5 w-3.5 shrink-0 motion-safe:animate-spin rounded-full border-2 border-accent/25 border-t-accent" />
          Compiling on the real Graph toolchain: running codegen, build &amp; test…
        </div>
      </div>
    );
  }
  if (!verdict) return null;

  if (verdict.ok) {
    return (
      <div className="border-b border-ok/25 bg-ok/10 px-4 py-2.5 text-sm text-ok">
        ✓ Verified: <span className="text-text">graph codegen, build &amp; Matchstick tests all passed.</span>
      </div>
    );
  }

  // Config-not-set is informational, not a failure of the generated code.
  const isConfig = verdict.stage === "config";
  return (
    <details className="border-b border-line" open={!isConfig}>
      <summary
        className={`cursor-pointer list-none px-4 py-2.5 text-sm ${
          isConfig ? "text-muted" : "text-bad"
        }`}
      >
        {isConfig
          ? "Compile check unavailable (verifier not configured yet)."
          : verdict.timedOut
            ? "⚠ Compile check timed out."
            : `✕ Did not compile at ${verdict.stage}. Show details`}
      </summary>
      {(verdict.log || verdict.error) && (
        <pre className="max-h-56 overflow-auto border-t border-line px-4 py-3 font-mono text-xs leading-relaxed text-muted">
          {verdict.log || verdict.error}
        </pre>
      )}
    </details>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() => {
        navigator.clipboard.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      }}
      className="mr-1 rounded-md px-2 py-1 font-mono text-xs text-faint transition-colors hover:bg-inset hover:text-text"
    >
      {copied ? "copied" : "copy"}
    </button>
  );
}

function EventRow({
  event,
  checked,
  onToggle,
}: {
  event: EventDef;
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <li>
      <label className="flex cursor-pointer items-center gap-3 rounded-md px-2 py-1.5 hover:bg-inset">
        <input
          type="checkbox"
          checked={checked}
          onChange={onToggle}
          className="h-4 w-4 accent-accent"
        />
        <span className="font-mono text-sm text-text">{event.name}</span>
        <span className="truncate font-mono text-xs text-faint">
          ({event.inputs.map((i) => `${i.type} ${i.name}`).join(", ")})
        </span>
      </label>
    </li>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex gap-2">
      <dt className="text-faint">{label}</dt>
      <dd className="truncate text-muted">{value}</dd>
    </div>
  );
}

function Badge({ tone, children }: { tone: "green" | "amber"; children: React.ReactNode }) {
  const cls =
    tone === "green"
      ? "border-ok/30 bg-ok/10 text-ok"
      : "border-warn/30 bg-warn/10 text-warn";
  return (
    <span className={`rounded-full border px-2 py-0.5 font-mono text-[0.68rem] ${cls}`}>
      {children}
    </span>
  );
}

function shorten(addr: string): string {
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}

function explorerLink(c: ContractInfo): string {
  // Etherscan V2 multichain explorer deep-link is chain-specific; the address page
  // pattern is consistent enough for the common chains.
  const host: Record<number, string> = {
    1: "etherscan.io",
    42161: "arbiscan.io",
    8453: "basescan.org",
    10: "optimistic.etherscan.io",
    137: "polygonscan.com",
    56: "bscscan.com",
    43114: "snowtrace.io",
    100: "gnosisscan.io",
    534352: "scrollscan.com",
    59144: "lineascan.build",
    11155111: "sepolia.etherscan.io",
    84532: "sepolia.basescan.org",
    421614: "sepolia.arbiscan.io",
  };
  return `https://${host[c.chainId] ?? "etherscan.io"}/address/${c.address}`;
}
