# Homebrew distribution

`brew install` for `redstart` is served from a **tap** — a separate GitHub repo
named `homebrew-<name>` that contains the formula. The release workflow renders
[`redstart.rb.tmpl`](./redstart.rb.tmpl) into `Formula/redstart.rb` and pushes it
to the tap on every tagged release, so the tap stays in sync automatically.

## One-time setup

1. **Create the tap repo.** A public repo called `homebrew-tap` under the same
   org (default the workflow expects: `nuthatch-org/homebrew-tap`). It can start
   empty — the workflow creates `Formula/redstart.rb` on the first release.

   To point at a different tap, set a repo variable `HOMEBREW_TAP_REPO`
   (e.g. `your-org/homebrew-tap`).

2. **Create a token with write access to the tap.** A fine-grained PAT scoped to
   the tap repo with **Contents: read & write**, or a classic PAT with `repo`.

3. **Add it as a secret** on *this* repo named `HOMEBREW_TAP_TOKEN`
   (Settings → Secrets and variables → Actions → New repository secret).

That's it. The `bump-homebrew` job skips cleanly when the secret is absent, so
binary releases work before the tap is wired up.

## Cutting a release

```sh
git tag v0.1.0
git push origin v0.1.0
```

The workflow builds the cross-compiled binaries, publishes the GitHub Release,
and updates the tap.

## Installing

```sh
brew install nuthatch-org/tap/redstart
# or, after tapping once:
brew tap nuthatch-org/tap
brew install redstart
```

## Bootstrapping the formula by hand (optional)

Before the first automated release you can seed the tap manually: render the
template, filling in the version and the four `sha256` values from the Release's
`*.tar.gz.sha256` assets, and commit it as `Formula/redstart.rb` in the tap.
