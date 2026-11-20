import { ImageResponse } from "next/og";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

export const alt = "Redstart: one typed language for The Graph subgraphs";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

const BG = "#171614";
const TEXT = "#ece9e3";
const MUTED = "#9c978c";
const ACCENT = "#d0704f";
const ACCENT_INK = "#f2cbbb";

function Bird({ s, color }: { s: number; color: string }) {
  return (
    <svg width={s} height={s} viewBox="0 0 32 32">
      <path d="M15.6 21 L13.9 28.4 L19.4 26.8 L18.4 20.4 Z" fill={ACCENT_INK} />
      <circle cx="13.4" cy="11" r="4.7" fill={color} />
      <path
        d="M9.4 14 C8.9 18.4 11.4 22.4 15.4 22.4 C19.7 22.4 21.2 17.9 19.8 14.1 C18.7 11.5 15.8 10.3 13.4 11.3 Z"
        fill={color}
      />
      <path d="M9.2 10.2 L4.4 9.7 L9.4 12.3 Z" fill={color} />
      <circle cx="11.5" cy="10" r="1.15" fill={BG} />
    </svg>
  );
}

export default async function Image() {
  const [display, mono] = await Promise.all([
    readFile(join(process.cwd(), "assets/SpaceGrotesk-Medium.woff")),
    readFile(join(process.cwd(), "assets/JetBrainsMono-Regular.ttf")),
  ]);

  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          background: BG,
          padding: "72px 80px",
          fontFamily: "Space Grotesk",
          position: "relative",
        }}
      >
        {/* wordmark */}
        <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
          <Bird s={36} color={ACCENT} />
          <span style={{ fontFamily: "Space Grotesk", fontSize: 32, letterSpacing: "-0.06em", color: TEXT }}>redstart</span>
        </div>

        {/* tagline */}
        <div style={{ display: "flex", flexDirection: "column", fontFamily: "Space Grotesk" }}>
          <span style={{ fontSize: 88, fontWeight: 500, lineHeight: 1.0, color: TEXT, letterSpacing: "-0.03em" }}>
            Write the subgraph once.
          </span>
          <span style={{ fontSize: 88, fontWeight: 500, lineHeight: 1.0, color: ACCENT, letterSpacing: "-0.03em" }}>
            Properly.
          </span>
        </div>

        {/* footer */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            borderTop: "1px solid rgba(255,255,255,0.1)",
            paddingTop: 28,
            fontFamily: "JetBrains Mono",
            fontSize: 23,
          }}
        >
          <span style={{ color: MUTED }}>One typed language for The Graph subgraphs</span>
          <span style={{ color: TEXT }}>redstart-lang.com</span>
        </div>
      </div>
    ),
    {
      ...size,
      fonts: [
        { name: "Space Grotesk", data: display, style: "normal", weight: 500 },
        { name: "JetBrains Mono", data: mono, style: "normal", weight: 400 },
      ],
    },
  );
}
