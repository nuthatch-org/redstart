import type { Metadata } from "next";
import { Generator } from "@/components/generator";

export const metadata: Metadata = {
  title: "The Generator",
  description:
    "Paste a contract address and get a best-practices, tested subgraph, verified before you see it. Your AI, your repo, your keys.",
};

export default function GeneratorPage() {
  return <Generator />;
}
