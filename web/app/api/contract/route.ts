import type { NextRequest } from "next/server";
import { chainById } from "@/lib/chains";
import {
  type AbiItem,
  type ContractInfo,
  detectKind,
  extractEvents,
} from "@/lib/subgraph-abi";

// Stateless proxy for the contract-research pipeline. It only ever talks to
// Etherscan with Pete's key; it never sees the user's AI key (all AI calls are
// client-side). Verified-contract endpoints (source/ABI/creation) are free on
// every chain via the V2 single-key API.

const ETHERSCAN_V2 = "https://api.etherscan.io/v2/api";
const ADDRESS_RE = /^0x[a-fA-F0-9]{40}$/;

interface EtherscanResponse {
  status: string;
  message: string;
  result: unknown;
}

async function etherscan(
  chainId: number,
  params: Record<string, string>,
  apiKey: string,
): Promise<EtherscanResponse> {
  const url = new URL(ETHERSCAN_V2);
  url.searchParams.set("chainid", String(chainId));
  url.searchParams.set("apikey", apiKey);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  const res = await fetch(url, { headers: { accept: "application/json" } });
  if (!res.ok) throw new Error(`Etherscan HTTP ${res.status}`);
  return (await res.json()) as EtherscanResponse;
}

interface SourceResult {
  ABI: string;
  ContractName: string;
  Proxy: string;
  Implementation: string;
}

/** Parse the ABI string from a getsourcecode result, or null if unverified. */
function parseAbi(abiStr: string | undefined): AbiItem[] | null {
  if (!abiStr || abiStr.trim().startsWith("Contract source code not verified")) {
    return null;
  }
  try {
    const parsed = JSON.parse(abiStr);
    return Array.isArray(parsed) ? (parsed as AbiItem[]) : null;
  } catch {
    return null;
  }
}

async function fetchSource(
  chainId: number,
  address: string,
  apiKey: string,
): Promise<SourceResult | null> {
  const resp = await etherscan(
    chainId,
    { module: "contract", action: "getsourcecode", address },
    apiKey,
  );
  if (resp.status !== "1" || !Array.isArray(resp.result) || resp.result.length === 0) {
    return null;
  }
  return resp.result[0] as SourceResult;
}

async function fetchStartBlock(
  chainId: number,
  address: string,
  apiKey: string,
): Promise<number | undefined> {
  try {
    const resp = await etherscan(
      chainId,
      { module: "contract", action: "getcontractcreation", contractaddresses: address },
      apiKey,
    );
    if (resp.status !== "1" || !Array.isArray(resp.result) || resp.result.length === 0) {
      return undefined;
    }
    const row = resp.result[0] as { blockNumber?: string };
    const n = row.blockNumber ? Number(row.blockNumber) : NaN;
    return Number.isFinite(n) ? n : undefined;
  } catch {
    return undefined;
  }
}

export async function GET(request: NextRequest) {
  const apiKey = process.env.ETHERSCAN_API_KEY;
  if (!apiKey) {
    return Response.json(
      { error: "Server missing ETHERSCAN_API_KEY. Set it in the environment to enable lookups." },
      { status: 500 },
    );
  }

  const sp = request.nextUrl.searchParams;
  const address = (sp.get("address") ?? "").trim();
  const chainId = Number(sp.get("chainId") ?? "1");

  if (!ADDRESS_RE.test(address)) {
    return Response.json({ error: "Enter a valid 0x… contract address." }, { status: 400 });
  }
  const chain = chainById(chainId);
  if (!chain) {
    return Response.json({ error: `Unsupported chain ${chainId}.` }, { status: 400 });
  }

  let source: SourceResult | null;
  try {
    source = await fetchSource(chainId, address, apiKey);
  } catch (e) {
    return Response.json(
      { error: `Contract lookup failed: ${(e as Error).message}` },
      { status: 502 },
    );
  }

  if (!source) {
    return Response.json(
      { error: "No contract found at that address on this chain." },
      { status: 404 },
    );
  }

  const isProxy = source.Proxy === "1" && ADDRESS_RE.test(source.Implementation);
  const implementation = isProxy ? source.Implementation : undefined;

  // For a proxy, the events live in the implementation ABI. Index at the proxy,
  // but read the implementation's ABI.
  let abi = parseAbi(source.ABI);
  let verified = abi !== null;
  if (isProxy && implementation) {
    try {
      const impl = await fetchSource(chainId, implementation, apiKey);
      const implAbi = parseAbi(impl?.ABI);
      if (implAbi) {
        abi = implAbi;
        verified = true;
      }
    } catch {
      // keep proxy ABI (or null) if the implementation lookup fails
    }
  }

  if (!abi) {
    // v0.1 is verified-only. Bytecode ABI recovery (whatsabi) lands in Stage 1.
    return Response.json(
      {
        error:
          "This contract isn't verified on Etherscan. ABI recovery from bytecode is coming soon. For now, try a verified contract.",
        verified: false,
      },
      { status: 422 },
    );
  }

  const startBlock = await fetchStartBlock(chainId, address, apiKey);

  const info: ContractInfo = {
    address,
    chainId,
    graphNetwork: chain.graphNetwork,
    name: source.ContractName || "Contract",
    reconstructed: false,
    verified,
    isProxy,
    implementation,
    startBlock,
    abi,
    events: extractEvents(abi),
    kind: detectKind(abi),
  };

  return Response.json(info);
}
