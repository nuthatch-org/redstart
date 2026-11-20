import type { Metadata } from "next";
import localFont from "next/font/local";
import "./globals.css";
import { Nav } from "@/components/nav";
import { Footer } from "@/components/footer";
import { Cosmos } from "@/components/cosmos";
import { VersionWatcher } from "@/components/version-watcher";

const sans = localFont({
  src: "./fonts/space-grotesk.woff2",
  variable: "--font-space-grotesk",
  weight: "300 700",
});

const mono = localFont({
  src: "./fonts/jetbrains-mono.woff2",
  variable: "--font-jetbrains-mono",
  weight: "400",
});

const SITE = "https://redstart-lang.com";
const TAGLINE = "Redstart — the best language for building The Graph subgraphs";
const DESCRIPTION =
  "The most performant and secure language for authoring The Graph subgraphs. One typed source for schema, manifest, and mappings — compiled to AssemblyScript that's faster and safer than any human would hand-write. If it compiles, it works.";

export const metadata: Metadata = {
  metadataBase: new URL(SITE),
  title: {
    default: TAGLINE,
    template: "%s · Redstart",
  },
  description: DESCRIPTION,
  openGraph: {
    title: TAGLINE,
    description: DESCRIPTION,
    url: SITE,
    siteName: "Redstart",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: TAGLINE,
    description: DESCRIPTION,
  },
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html
      lang="en"
      className={`${sans.variable} ${mono.variable} h-full antialiased`}
    >
      <body className="relative flex min-h-full flex-col bg-bg text-text">
        <Cosmos />
        <Nav />
        <main className="relative flex-1">{children}</main>
        <Footer />
        <VersionWatcher />
      </body>
    </html>
  );
}
