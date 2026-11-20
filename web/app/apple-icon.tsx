import { ImageResponse } from "next/og";

export const size = { width: 180, height: 180 };
export const contentType = "image/png";

// Apple touch icon — the Redstart bird, rendered to PNG at build time.
export default function AppleIcon() {
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "#171614",
        }}
      >
        <svg width="132" height="132" viewBox="0 0 32 32">
          <path d="M15.6 21 L13.9 28.4 L19.4 26.8 L18.4 20.4 Z" fill="#f2cbbb" />
          <circle cx="13.4" cy="11" r="4.7" fill="#d0704f" />
          <path
            d="M9.4 14 C8.9 18.4 11.4 22.4 15.4 22.4 C19.7 22.4 21.2 17.9 19.8 14.1 C18.7 11.5 15.8 10.3 13.4 11.3 Z"
            fill="#d0704f"
          />
          <path d="M9.2 10.2 L4.4 9.7 L9.4 12.3 Z" fill="#d0704f" />
          <circle cx="11.5" cy="10" r="1.15" fill="#171614" />
        </svg>
      </div>
    ),
    { ...size },
  );
}
