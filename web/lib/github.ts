// Client-side GitHub: connect via OAuth popup, then create a repo and push the
// whole project directly from the browser using the user's token. The token lives
// in memory for the duration of the push and is never stored or sent to us.

const GH_API = "https://api.github.com";

/** Open the GitHub OAuth popup and resolve with the user token. */
export function connectGitHub(clientId: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const state = crypto.randomUUID();
    const redirect = `${location.origin}/api/github/callback`;
    const url =
      `https://github.com/login/oauth/authorize?client_id=${clientId}` +
      // `workflow` lets us commit the generated .github/workflows CI file. If the
      // user declines it we still push everything else (see createSubgraphRepo).
      `&scope=${encodeURIComponent("public_repo workflow")}&redirect_uri=${encodeURIComponent(redirect)}&state=${state}`;

    const popup = window.open(url, "redstart-github", "width=720,height=820");
    if (!popup) {
      reject(new Error("Popup blocked. Allow popups for this site and retry."));
      return;
    }

    const onMessage = (e: MessageEvent) => {
      if (e.origin !== location.origin) return;
      const d = e.data;
      if (!d || d.type !== "redstart-github") return;
      cleanup();
      if (d.error) reject(new Error(d.error));
      else if (d.state && d.state !== state) reject(new Error("State mismatch. Please retry."));
      else if (d.token) resolve(d.token as string);
      else reject(new Error("No token returned."));
    };

    const timer = setInterval(() => {
      if (popup.closed) {
        cleanup();
        reject(new Error("Window closed before authorizing."));
      }
    }, 500);

    function cleanup() {
      clearInterval(timer);
      window.removeEventListener("message", onMessage);
    }
    window.addEventListener("message", onMessage);
  });
}

async function gh(
  token: string,
  path: string,
  body?: unknown,
  method?: string,
  label?: string,
) {
  const res = await fetch(`${GH_API}${path}`, {
    method: method ?? (body ? "POST" : "GET"),
    headers: {
      authorization: `Bearer ${token}`,
      accept: "application/vnd.github+json",
      "x-github-api-version": "2022-11-28",
      ...(body ? { "content-type": "application/json" } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    // GitHub's top-level `message` is often generic ("Repository creation failed.");
    // the real reason is in `errors[]`. Surface both so failures are actionable.
    const detail = Array.isArray(data?.errors)
      ? data.errors
          .map((e: { message?: string; field?: string; code?: string }) =>
            e.message ?? [e.field, e.code].filter(Boolean).join(" "),
          )
          .filter(Boolean)
          .join("; ")
      : "";
    // A 404/403 here is almost always a missing/cached scope. Surface which step
    // failed and what scopes the token actually carries, so it's actionable.
    const scopes = res.headers.get("x-oauth-scopes");
    const hint =
      (res.status === 404 || res.status === 403) && !hasRepoScope(scopes)
        ? `: your GitHub token is missing repo access (scopes: ${scopes || "none"}). Revoke Redstart at github.com/settings/applications, then reconnect and approve the repository permission.`
        : "";
    const where = label ? `${label}: ` : "";
    const msg =
      where +
      ([data?.message, detail].filter(Boolean).join(": ") || `GitHub API ${res.status}`) +
      hint;
    throw new Error(msg);
  }
  return data;
}

/** Whether an X-OAuth-Scopes header grants repo creation (`repo` or `public_repo`). */
function hasRepoScope(scopes: string | null): boolean {
  if (!scopes) return false;
  const set = scopes.split(",").map((s) => s.trim());
  return set.includes("repo") || set.includes("public_repo");
}

/** Whether the token may commit workflow files (`.github/workflows/*`). */
function hasWorkflowScope(scopes: string | null): boolean {
  if (!scopes) return false;
  return scopes.split(",").map((s) => s.trim()).includes("workflow");
}

/**
 * Like `gh`, but retries on a 404. The git-data endpoints (blobs/trees/commits/
 * refs) can 404 for a beat right after `auto_init` while the git backend settles;
 * a browser is fast enough to race it. Retries only on "not found"/404.
 */
async function ghRetry(
  token: string,
  path: string,
  body?: unknown,
  method?: string,
  label?: string,
  tries = 4,
) {
  for (let i = 0; ; i++) {
    try {
      return await gh(token, path, body, method, label);
    } catch (e) {
      const m = (e as Error).message || "";
      if (i >= tries - 1 || !/not found|404/i.test(m)) throw e;
      await new Promise((r) => setTimeout(r, 800));
    }
  }
}

export interface CreatedRepo {
  url: string;
  fullName: string;
  /** True if the CI workflow file was omitted because `workflow` scope was declined. */
  workflowSkipped: boolean;
}

/** Create a public repo and push the whole project as a single initial commit. */
export async function createSubgraphRepo(
  token: string,
  repoName: string,
  files: Record<string, string>,
  description: string,
): Promise<CreatedRepo> {
  // 0. Preflight: confirm the token is live and actually carries repo scope. A
  // cached OAuth grant can hand back a token missing `public_repo`, which makes
  // `POST /user/repos` return a bare "404 Not Found", so check up front so the
  // error is "reconnect and approve repo access", not a mystery.
  const meRes = await fetch(`${GH_API}/user`, {
    headers: {
      authorization: `Bearer ${token}`,
      accept: "application/vnd.github+json",
      "x-github-api-version": "2022-11-28",
    },
  });
  if (!meRes.ok) {
    throw new Error(
      `auth check: GitHub ${meRes.status}. The connection didn't stick. Disconnect and reconnect GitHub.`,
    );
  }
  const scopes = meRes.headers.get("x-oauth-scopes");
  if (!hasRepoScope(scopes)) {
    throw new Error(
      `Your GitHub token can't create repositories (scopes: ${scopes || "none"}). Revoke Redstart at github.com/settings/applications, then reconnect and approve the repository permission.`,
    );
  }
  // Whether we may push the CI workflow file. If the user didn't grant `workflow`,
  // we skip it (below) rather than failing the whole push.
  const canWorkflow = hasWorkflowScope(scopes);

  // 1. Create the repo with an initial commit. `auto_init: true` is required:
  // the Git Data API can't create blobs/trees on a bare repo ("Git Repository is
  // empty"), so GitHub must lay down a first commit to initialise the git backend.
  const repo = await gh(
    token,
    "/user/repos",
    { name: repoName, description, private: false, auto_init: true },
    "POST",
    "create repo",
  );
  const owner: string = repo.owner.login;
  const name: string = repo.name;
  const branch: string = repo.default_branch || "main";

  // 2. The base commit + its tree (to layer our files on top). All git-data calls
  // go through ghRetry: a fresh auto_init repo can 404 them for a moment.
  const ref = await ghRetry(
    token,
    `/repos/${owner}/${name}/git/ref/heads/${branch}`,
    undefined,
    undefined,
    "read branch",
  );
  const baseSha: string = ref.object.sha;
  const baseCommit = await ghRetry(
    token,
    `/repos/${owner}/${name}/git/commits/${baseSha}`,
    undefined,
    undefined,
    "read base commit",
  );

  // 3. Blob per file. Skip `.github/workflows/*` only if the user didn't grant the
  // `workflow` scope, because pushing those files requires it; the CI file always ships
  // in the .zip regardless.
  let workflowSkipped = false;
  const tree = [];
  for (const [path, content] of Object.entries(files)) {
    if (path.startsWith(".github/workflows/") && !canWorkflow) {
      workflowSkipped = true;
      continue;
    }
    const blob = await ghRetry(
      token,
      `/repos/${owner}/${name}/git/blobs`,
      { content, encoding: "utf-8" },
      "POST",
      "write blob",
    );
    tree.push({ path, mode: "100644", type: "blob", sha: blob.sha });
  }

  // 4. Tree (on the base tree) → 5. commit (on the initial commit) → 6. move the branch.
  const treeObj = await ghRetry(
    token,
    `/repos/${owner}/${name}/git/trees`,
    { base_tree: baseCommit.tree.sha, tree },
    "POST",
    "build tree",
  );
  const commit = await ghRetry(
    token,
    `/repos/${owner}/${name}/git/commits`,
    { message: "Add subgraph, generated by The Generator (redstart-lang.com)", tree: treeObj.sha, parents: [baseSha] },
    "POST",
    "write commit",
  );
  await ghRetry(
    token,
    `/repos/${owner}/${name}/git/refs/heads/${branch}`,
    { sha: commit.sha, force: true },
    "PATCH",
    "update branch",
  );

  return { url: repo.html_url, fullName: repo.full_name, workflowSkipped };
}
