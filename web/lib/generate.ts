// Client-side subgraph generation via the user's own Anthropic key (BYOK).
//
// The request is made directly from the browser with the documented
// `anthropic-dangerous-direct-browser-access` header, so the key never touches
// our server. Structured outputs (`output_config.format`) force a clean JSON
// object with the three subgraph files, so no markdown-fence parsing.

import type { ContractInfo, EventDef } from "./subgraph-abi";

export interface GeneratedSubgraph {
  schema: string; // schema.graphql
  manifest: string; // subgraph.yaml
  mappings: string; // src/mapping.ts (AssemblyScript)
  tests: string; // tests/<name>.test.ts (Matchstick)
  testUtils: string; // tests/utils.ts: event factory helpers
  notes: string; // short human summary of decisions made
}

const ANTHROPIC_URL = "https://api.anthropic.com/v1/messages";
// Opus 4.8: most capable; adaptive-thinking family. We omit `thinking` for a
// fast single-shot codegen call, and never send temperature/top_p (they 400 on 4.8).
const MODEL = "claude-opus-4-8";

// The best-practices checklist, encoded as generation guardrails. Fuses The
// Graph's official best practices, the Subgraph Linter rules, and the
// AssemblyScript-not-TypeScript footguns.
const SYSTEM = `You are an expert The Graph subgraph engineer. You generate a complete, best-practices, COMPILING subgraph for a single EVM contract: a GraphQL schema, a subgraph manifest (YAML), AssemblyScript mappings, and Matchstick unit tests.

CRITICAL: the mappings are AssemblyScript, NOT TypeScript. AssemblyScript is a strict, statically-typed subset compiled to WebAssembly. Never emit TypeScript idioms that do not compile under \`asc\`.

SCHEMA (schema.graphql):
- EVERY \`@entity\` MUST carry an explicit immutable argument: \`@entity(immutable: true)\` or \`@entity(immutable: false)\`. A bare \`@entity\` is REJECTED ("@entity directive requires \`immutable\` argument").
- Use \`Bytes!\` for IDs derived from addresses or hashes; never concatenate strings for a single-value id.
- Mark event-log/append-only entities \`@entity(immutable: true)\`; use \`@entity(immutable: false)\` where state genuinely updates (balances, owners, counters).
- Model one-to-many relations with \`@derivedFrom(field: "...")\`; never store a growing array on the parent.
- Every non-null, non-derived field must be initialised before \`.save()\`.

MANIFEST (subgraph.yaml):
- specVersion 1.2.0; schema file schema.graphql; dataSources kind ethereum/contract.
- \`network\` exactly as provided. \`source.address\`, \`source.abi\`, and \`source.startBlock\` exactly as provided.
- Add \`indexerHints:\\n  prune: auto\` at the top level.
- mapping apiVersion 0.0.9, language wasm/assemblyscript, file ./src/mapping.ts.
- One eventHandler per selected event, with the EXACT event signature (from the ABI) and a handler function name like \`handleTransfer\`.
- Prefer event handlers only (no call/block handlers).

MAPPINGS (src/mapping.ts, AssemblyScript):
- Import generated event classes from ../generated/<DataSourceName>/<ContractName> and entity classes from ../generated/schema, plus BigInt/Bytes/etc. from @graphprotocol/graph-ts as needed.
- Use \`==\`, never \`===\` (AssemblyScript has no strict equality).
- Never force-unwrap \`Entity.load(id)!\`; load, null-check, and construct if null (loadOrCreate pattern).
- Guard nullable arithmetic; initialise BigInt fields with \`BigInt.zero()\`.
- \`.save()\` after every mutation. Never leave a required field unset before save.
- Use \`try_\` variants for any contract call and handle \`.reverted\`; avoid eth_calls entirely when the data is already in the event args; never call a contract inside a loop.
- No \`console.log\` (it traps at runtime); use \`log.info\`/\`log.warning\` from graph-ts if logging.
- Derive a Bytes id for event-log entities from \`event.transaction.hash.concatI32(event.logIndex.toI32())\`.

Match entity modelling to the contract kind (ERC-20: Account/Transfer with balances; ERC-721/1155: Token/Owner/Transfer; unknown: one immutable entity per selected event). Only index the events the user selected.

TESTS (Matchstick: \`tests\` is tests/<name>.test.ts, \`testUtils\` is tests/utils.ts):
- \`tests/utils.ts\`: export one factory per handled event that builds a mock event with \`newMockEvent()\`, then sets \`.parameters\` via \`new ethereum.EventParam(name, ethereum.Value.fromX(...))\` for each arg, matching the ABI types exactly (address→\`fromAddress\`, uint→\`fromUnsignedBigInt\`, bytes→\`fromBytes\`, bool→\`fromBoolean\`, string→\`fromString\`). Import \`newMockEvent\` from \`matchstick-as/assembly/index\` and \`ethereum, Address, BigInt, Bytes\` from \`@graphprotocol/graph-ts\`. Also import the generated event class(es) from \`../generated/<DataSource>/<Contract>\` and return that type (cast the mock: build a \`new XEvent(mock.address, mock.logIndex, ...)\` OR reuse the changetype pattern \`changetype<XEvent>(newMockEvent())\` then assign params).
- \`tests/<name>.test.ts\`: import \`assert, describe, test, clearStore, beforeAll, afterAll\` from \`matchstick-as/assembly/index\`, the handler(s) from \`../src/mapping\`, and the factories from \`./utils\`. In a \`describe\`, write at least one \`test\` per handler: build a mock event with realistic-looking values, call the handler, then \`assert.entityCount(...)\` and \`assert.fieldEquals("Entity", id, "field", expected)\`. Use \`beforeAll\`/\`afterAll\` with \`clearStore()\`.
- These are AssemblyScript too, with the same rules (no \`===\`, no TS idioms). They must compile under \`graph test\`.

Output ONLY the structured object with all fields. \`notes\` is 1-3 short sentences on the key modelling decisions.`;

const OUTPUT_SCHEMA = {
  type: "object",
  properties: {
    schema: { type: "string", description: "Full contents of schema.graphql" },
    manifest: { type: "string", description: "Full contents of subgraph.yaml" },
    mappings: { type: "string", description: "Full contents of src/mapping.ts (AssemblyScript)" },
    tests: { type: "string", description: "Full contents of tests/<name>.test.ts (Matchstick)" },
    testUtils: { type: "string", description: "Full contents of tests/utils.ts (event factory helpers)" },
    notes: { type: "string", description: "1-3 sentences on the modelling decisions made" },
  },
  required: ["schema", "manifest", "mappings", "tests", "testUtils", "notes"],
  additionalProperties: false,
};

function buildUserMessage(
  contract: ContractInfo,
  selectedEvents: EventDef[],
  dataSourceName: string,
): string {
  const events = selectedEvents
    .map(
      (e) =>
        `- ${e.signature}  [inputs: ${e.inputs.map((i) => `${i.type}${i.indexed ? " indexed" : ""} ${i.name}`).join(", ") || "none"}]`,
    )
    .join("\n");

  return `Generate a subgraph for this contract.

Contract name: ${contract.name}
Kind: ${contract.kind}
Data source name (use for the manifest name and the generated import path): ${dataSourceName}
Address (index at this address): ${contract.address}
Network: ${contract.graphNetwork}
Start block: ${contract.startBlock ?? "0 (unknown, use 0)"}
${contract.isProxy ? `Proxy: yes, implementation ${contract.implementation}. Index the proxy address with the implementation ABI.` : ""}

Events to index (${selectedEvents.length}):
${events || "(none; this should not happen)"}

Full ABI (JSON):
${JSON.stringify(contract.abi)}`;
}

export interface AnthropicError {
  status: number;
  message: string;
}

/** One structured-output call to Claude, returning the parsed subgraph files. */
async function callClaude(apiKey: string, userMessage: string): Promise<GeneratedSubgraph> {
  const res = await fetch(ANTHROPIC_URL, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-api-key": apiKey,
      "anthropic-version": "2023-06-01",
      "anthropic-dangerous-direct-browser-access": "true",
    },
    body: JSON.stringify({
      model: MODEL,
      max_tokens: 20000,
      system: SYSTEM,
      messages: [{ role: "user", content: userMessage }],
      output_config: { format: { type: "json_schema", schema: OUTPUT_SCHEMA } },
    }),
  });

  const data = await res.json();
  if (!res.ok) {
    throw { status: res.status, message: data?.error?.message ?? `Anthropic API error (${res.status}).` } as AnthropicError;
  }
  if (data.stop_reason === "refusal") {
    throw { status: 200, message: "The model declined this request." } as AnthropicError;
  }
  if (data.stop_reason === "max_tokens") {
    throw { status: 200, message: "Response was cut off (max tokens). Try fewer events." } as AnthropicError;
  }
  const text: string | undefined = data?.content?.find(
    (b: { type: string; text?: string }) => b.type === "text",
  )?.text;
  if (!text) throw { status: 200, message: "Empty response from the model." } as AnthropicError;
  try {
    return JSON.parse(text) as GeneratedSubgraph;
  } catch {
    throw { status: 200, message: "Could not parse the model's response. Try again." } as AnthropicError;
  }
}

/**
 * Generate a subgraph from a contract, client-side with the user's key.
 * @throws {@link AnthropicError}
 */
export async function generateSubgraph(
  apiKey: string,
  contract: ContractInfo,
  selectedEvents: EventDef[],
): Promise<GeneratedSubgraph> {
  return callClaude(apiKey, buildUserMessage(contract, selectedEvents, sanitizeName(contract.name)));
}

/**
 * Feed a compile/test failure back to Claude and get corrected files. Used by the
 * "Fix issues" loop, which re-verifies after each pass.
 * @throws {@link AnthropicError}
 */
export async function fixSubgraph(
  apiKey: string,
  contract: ContractInfo,
  files: GeneratedSubgraph,
  errorLog: string,
): Promise<GeneratedSubgraph> {
  const name = sanitizeName(contract.name);
  const msg = `The subgraph below FAILED verification (graph codegen / build / test). Fix the reported errors and return ALL files corrected. Change only what's needed; keep the parts that work.

VERIFICATION OUTPUT (the errors to fix):
${errorLog || "(no log)"}

CURRENT FILES:

=== schema.graphql ===
${files.schema}

=== subgraph.yaml ===
${files.manifest}

=== src/mapping.ts ===
${files.mappings}

=== tests/${name.toLowerCase()}.test.ts ===
${files.tests}

=== tests/utils.ts ===
${files.testUtils}

CONTRACT: ${contract.name} (${contract.kind}) on ${contract.graphNetwork}, indexed at ${contract.address}${contract.isProxy ? `, a proxy with implementation ${contract.implementation}` : ""}.
ABI (for reference):
${JSON.stringify(contract.abi)}

Return every file (schema, manifest, mappings, tests, testUtils, notes).`;
  return callClaude(apiKey, msg);
}

/** A safe PascalCase data-source/contract name for the manifest and imports. */
export function sanitizeName(raw: string): string {
  const cleaned = raw.replace(/[^A-Za-z0-9]/g, " ").trim();
  const pascal = cleaned
    .split(/\s+/)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join("");
  return /^[A-Za-z]/.test(pascal) ? pascal : `Contract${pascal}`;
}
