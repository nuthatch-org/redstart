// Chains the generator supports. Etherscan V2 reaches all of these with a single
// API key via the `chainid` query param, so one proxy covers every network.
//
// `graphNetwork` is the name that goes into `subgraph.yaml` (`dataSources[].network`),
// which differs from the human label and the chain id, e.g. Polygon is `matic`.

export interface Chain {
  id: number;
  label: string;
  /** The `network:` value graph-node expects in the manifest. */
  graphNetwork: string;
  /** A public RPC for event sampling (no key). Best-effort. */
  rpc?: string;
}

export const CHAINS: Chain[] = [
  { id: 1, label: "Ethereum", graphNetwork: "mainnet", rpc: "https://eth.llamarpc.com" },
  { id: 42161, label: "Arbitrum One", graphNetwork: "arbitrum-one", rpc: "https://arb1.arbitrum.io/rpc" },
  { id: 8453, label: "Base", graphNetwork: "base", rpc: "https://mainnet.base.org" },
  { id: 10, label: "Optimism", graphNetwork: "optimism", rpc: "https://mainnet.optimism.io" },
  { id: 137, label: "Polygon", graphNetwork: "matic", rpc: "https://polygon-rpc.com" },
  { id: 56, label: "BNB Chain", graphNetwork: "bsc", rpc: "https://bsc-dataseed.binance.org" },
  { id: 43114, label: "Avalanche", graphNetwork: "avalanche", rpc: "https://api.avax.network/ext/bc/C/rpc" },
  { id: 100, label: "Gnosis", graphNetwork: "gnosis", rpc: "https://rpc.gnosischain.com" },
  { id: 534352, label: "Scroll", graphNetwork: "scroll", rpc: "https://rpc.scroll.io" },
  { id: 59144, label: "Linea", graphNetwork: "linea", rpc: "https://rpc.linea.build" },
  { id: 11155111, label: "Sepolia", graphNetwork: "sepolia", rpc: "https://rpc.sepolia.org" },
  { id: 84532, label: "Base Sepolia", graphNetwork: "base-sepolia", rpc: "https://sepolia.base.org" },
  { id: 421614, label: "Arbitrum Sepolia", graphNetwork: "arbitrum-sepolia", rpc: "https://sepolia-rollup.arbitrum.io/rpc" },
];

export const DEFAULT_CHAIN_ID = 1;

export function chainById(id: number): Chain | undefined {
  return CHAINS.find((c) => c.id === id);
}
