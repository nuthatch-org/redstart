import type { NextRequest } from "next/server";

// GitHub OAuth callback (popup flow). Exchanges the code for a user token using
// the server-side client secret, then returns a tiny HTML page that hands the
// token back to the opener window via postMessage and closes. The token is never
// stored: the opener uses it immediately to create the repo, client-side.

export const dynamic = "force-dynamic";

function page(payload: Record<string, unknown>, origin: string): Response {
  // Embed the payload safely and postMessage it to the opener.
  const json = JSON.stringify(payload).replace(/</g, "\\u003c");
  const html = `<!doctype html><meta charset="utf-8"><title>Connecting…</title>
<body style="background:#171614;color:#ece9e3;font-family:system-ui;display:grid;place-items:center;height:100vh;margin:0">
<p>Finishing up… you can close this window.</p>
<script>
  (function () {
    var msg = Object.assign({ type: "redstart-github" }, ${json});
    try { if (window.opener) window.opener.postMessage(msg, ${JSON.stringify(origin)}); } catch (e) {}
    setTimeout(function () { window.close(); }, 400);
  })();
</script></body>`;
  return new Response(html, { headers: { "content-type": "text/html; charset=utf-8" } });
}

export async function GET(request: NextRequest) {
  const origin = request.nextUrl.origin;
  const sp = request.nextUrl.searchParams;
  const code = sp.get("code");
  const state = sp.get("state") ?? "";
  const err = sp.get("error");

  if (err) return page({ error: sp.get("error_description") ?? err, state }, origin);
  if (!code) return page({ error: "No authorization code returned." }, origin);

  const clientId = process.env.NEXT_PUBLIC_GITHUB_CLIENT_ID;
  const clientSecret = process.env.GITHUB_CLIENT_SECRET;
  if (!clientId || !clientSecret) {
    return page({ error: "GitHub is not configured (missing client id/secret)." }, origin);
  }

  try {
    const res = await fetch("https://github.com/login/oauth/access_token", {
      method: "POST",
      headers: { accept: "application/json", "content-type": "application/json" },
      body: JSON.stringify({ client_id: clientId, client_secret: clientSecret, code }),
    });
    const data = await res.json();
    if (data.error || !data.access_token) {
      return page({ error: data.error_description ?? "Token exchange failed.", state }, origin);
    }
    return page({ token: data.access_token, state }, origin);
  } catch (e) {
    return page({ error: `Token exchange error: ${(e as Error).message}`, state }, origin);
  }
}
