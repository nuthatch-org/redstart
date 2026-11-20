// Generator regression harness.
//
// For each corpus contract: hit the deployed /api/contract, run the REAL
// generation (imports lib/generate.ts, the exact prompt users get), then run
// genuine `graph codegen && graph build && graph test` against it. Records a
// scorecard and a frozen results file, so "≥80% first-try green" becomes a number
// and regressions are caught as the prompt evolves.
//
//   ANTHROPIC_API_KEY=sk-ant-... npx tsx scripts/harness/run.ts
//   HARNESS_API=http://localhost:3000 npx tsx scripts/harness/run.ts   # against local dev
//   ONLY=Seaport npx tsx scripts/harness/run.ts                        # single contract
//
// One-time: `cd scripts/harness/base && npm install` (graph-cli + graph-ts + matchstick).

import { execFile } from "node:child_process";
import { cp, mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { generateSubgraph, sanitizeName } from "../../lib/generate";
import type { ContractInfo } from "../../lib/subgraph-abi";
import { CORPUS, type CorpusEntry } from "./corpus";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = join(HERE, "base");
const API = process.env.HARNESS_API ?? "https://redstart-lang.com";
const KEY = process.env.ANTHROPIC_API_KEY ?? "";
const ONLY = process.env.ONLY;
const execFileP = promisify(execFile);

interface Result {
  name: string;
  tier: number;
  researched: boolean;
  generated: boolean;
  codegen: boolean;
  build: boolean;
  test: boolean;
  green: boolean; // build && test
  entities: string[];
  error?: string;
}

async function fetchContract(e: CorpusEntry): Promise<ContractInfo | { error: string }> {
  const res = await fetch(`${API}/api/contract?address=${e.address}&chainId=${e.chainId}`);
  const data = await res.json();
  if (!res.ok) return { error: data.error ?? `HTTP ${res.status}` };
  return data as ContractInfo;
}

function entitiesFromSchema(schema: string): string[] {
  return [...schema.matchAll(/type\s+(\w+)\s+@entity/g)].map((m) => m[1]);
}

function abiPaths(manifest: string): string[] {
  return [...manifest.matchAll(/file:\s*\.?\/?(abis\/[\w.-]+\.json)/g)].map((m) => m[1]);
}

async function run(cmd: string, args: string[], cwd: string): Promise<{ ok: boolean; out: string }> {
  try {
    const { stdout } = await execFileP(cmd, args, { cwd, timeout: 150_000, maxBuffer: 10_000_000 });
    return { ok: true, out: stdout };
  } catch (err) {
    const e = err as { stdout?: string; stderr?: string };
    return { ok: false, out: (e.stderr || e.stdout || String(err)).slice(-1200) };
  }
}

async function verifyOne(e: CorpusEntry): Promise<Result> {
  const r: Result = {
    name: e.name, tier: e.tier, researched: false, generated: false,
    codegen: false, build: false, test: false, green: false, entities: [],
  };

  const info = await fetchContract(e);
  if ("error" in info) return { ...r, error: `research: ${info.error}` };
  r.researched = true;

  let files;
  try {
    files = await generateSubgraph(KEY, info, info.events);
  } catch (err) {
    return { ...r, error: `generate: ${(err as { message?: string }).message ?? err}` };
  }
  r.generated = true;
  r.entities = entitiesFromSchema(files.schema);

  // Assemble the project in a temp dir with node_modules symlinked from base.
  const dir = await mkdtemp(join(tmpdir(), `harness-${e.name}-`));
  try {
    await symlink(join(BASE, "node_modules"), join(dir, "node_modules"), "dir");
    await cp(join(BASE, "package.json"), join(dir, "package.json"));
    const name = sanitizeName(info.name);
    const write: Record<string, string> = {
      "schema.graphql": files.schema,
      "subgraph.yaml": files.manifest,
      "src/mapping.ts": files.mappings,
      [`tests/${name.toLowerCase()}.test.ts`]: files.tests,
      "tests/utils.ts": files.testUtils,
    };
    // Write the ABI at every path the manifest references (robust to naming).
    for (const p of abiPaths(files.manifest)) write[p] = JSON.stringify(info.abi, null, 2);
    for (const [p, content] of Object.entries(write)) {
      await mkdir(join(dir, dirname(p)), { recursive: true });
      await writeFile(join(dir, p), content);
    }

    const graph = join(dir, "node_modules", ".bin", "graph");
    const codegen = await run(graph, ["codegen"], dir);
    r.codegen = codegen.ok;
    if (!codegen.ok) return { ...r, error: `codegen:\n${codegen.out}` };
    const build = await run(graph, ["build"], dir);
    r.build = build.ok;
    if (!build.ok) return { ...r, error: `build:\n${build.out}` };
    const test = await run(graph, ["test"], dir);
    r.test = test.ok;
    r.green = r.build && r.test;
    if (!test.ok) r.error = `test:\n${test.out}`;
    return r;
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

async function main() {
  if (!KEY) {
    console.error("Set ANTHROPIC_API_KEY (a test-only key: the harness has no human to BYOK).");
    process.exit(1);
  }
  if (!existsSync(join(BASE, "node_modules"))) {
    console.error(`Base deps missing. Run:  cd ${BASE} && npm install`);
    process.exit(1);
  }

  const entries = ONLY ? CORPUS.filter((e) => e.name.toLowerCase() === ONLY.toLowerCase()) : CORPUS;
  console.log(`Running ${entries.length} contract(s) against ${API}\n`);

  const results: Result[] = [];
  for (const e of entries) {
    process.stdout.write(`  ${e.name.padEnd(18)} `);
    const r = await verifyOne(e);
    results.push(r);
    const mark = r.green ? "✓ green" : r.build ? "◑ compiles, tests fail" : r.generated ? "✕ no compile" : r.researched ? "✕ gen failed" : "✕ no research";
    console.log(mark + (r.entities.length ? `  [${r.entities.slice(0, 4).join(", ")}]` : ""));
    if (r.error && process.env.VERBOSE) console.log(r.error.split("\n").slice(0, 8).map((l) => "      " + l).join("\n"));
  }

  const green = results.filter((r) => r.green).length;
  const compiles = results.filter((r) => r.build).length;
  console.log(`\n──────────────────────────────────`);
  console.log(`compiles:      ${compiles}/${results.length}`);
  console.log(`first-try green (compiles+tests): ${green}/${results.length}  (${Math.round((green / results.length) * 100)}%)`);

  // Regressions: corpus expected green but wasn't.
  const regressions = results.filter((r) => {
    const exp = CORPUS.find((c) => c.name === r.name)!.expect;
    return exp.compiles && exp.testsPass && !r.green;
  });
  if (regressions.length) console.log(`\n⚠ below target: ${regressions.map((r) => r.name).join(", ")}`);

  await writeFile(join(HERE, "results.json"), JSON.stringify(results, null, 2));
  console.log(`\nwrote ${join(HERE, "results.json")}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
