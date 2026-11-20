// The graded test corpus. Each entry stresses a specific stage of the pipeline.
// Expectations are the TARGET: the harness reports actual vs. target and a
// first-try-green percentage. Verify addresses on Etherscan before trusting them.

export interface CorpusEntry {
  name: string;
  address: string;
  chainId: number;
  tier: 1 | 2 | 3 | 4;
  /** What this contract is here to stress. */
  stresses: string;
  expect: {
    researched: boolean;
    compiles: boolean;
    testsPass: boolean;
    /** Substrings expected in the generated schema (entity types). */
    entitiesInclude?: string[];
  };
}

export const CORPUS: CorpusEntry[] = [
  // ── Tier 1: the happy path ──
  {
    name: "WETH",
    address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
    chainId: 1,
    tier: 1,
    stresses: "Simplest verified ERC-20, no proxy, 4 events",
    expect: { researched: true, compiles: true, testsPass: true },
  },
  {
    name: "DAI",
    address: "0x6B175474E89094C44Da98b954EedeAC495271d0F",
    chainId: 1,
    tier: 1,
    stresses: "Plain verified ERC-20, non-proxy",
    expect: { researched: true, compiles: true, testsPass: true },
  },
  {
    name: "BAYC",
    address: "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D",
    chainId: 1,
    tier: 1,
    stresses: "ERC-721 detection + token-template selection",
    expect: { researched: true, compiles: true, testsPass: true },
  },
  // ── Tier 2: proxies / non-standard ──
  {
    name: "USDC",
    address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    chainId: 1,
    tier: 2,
    stresses: "Proxy: index at proxy, use implementation ABI (FiatTokenV2)",
    expect: { researched: true, compiles: true, testsPass: true },
  },
  {
    name: "USDT",
    address: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
    chainId: 1,
    tier: 2,
    stresses: "Non-standard ERC-20 (missing return values, Issue/Redeem/Deprecate)",
    expect: { researched: true, compiles: true, testsPass: true },
  },
  // ── Tier 3: ABI torture ──
  {
    name: "Seaport",
    address: "0x00000000000000ADc04C56Bf30aC9d3c0aAF14dC",
    chainId: 1,
    tier: 3,
    stresses: "Deeply nested tuple-array events (OrderFulfilled), the AS codegen killer",
    expect: { researched: true, compiles: true, testsPass: true },
  },
  {
    name: "CryptoPunks",
    address: "0xb47e3cd837dDF8e4c57F05d70Ab865de6e193BBB",
    chainId: 1,
    tier: 3,
    stresses: "Pre-ERC-721: NFT heuristic should NOT fire; generic fallback",
    expect: { researched: true, compiles: true, testsPass: true },
  },
  // ── Tier 4: factories (observe: factory templates not built yet) ──
  {
    name: "UniswapV2Factory",
    address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc8aA6f",
    chainId: 1,
    tier: 4,
    stresses: "Factory (PairCreated): today produces a FLAT subgraph (no child templates)",
    expect: { researched: true, compiles: true, testsPass: true },
  },
];
