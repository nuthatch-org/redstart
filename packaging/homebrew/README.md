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

2. **Give the release workflow write access to the tap**, scoped to the tap alone:

   ```sh
   ssh-keygen -t ed25519 -N '' -C redstart-release -f tapkey
   gh repo deploy-key add tapkey.pub -R nuthatch-org/homebrew-tap -t "redstart release" -w
   gh secret set HOMEBREW_TAP_DEPLOY_KEY -R nuthatch-org/redstart < tapkey
   rm tapkey tapkey.pub
   ```

   A PAT with write access to the tap, stored as `HOMEBREW_TAP_TOKEN`, also
   works; the deploy key is preferred because it can reach nothing else.

That's it. The `bump-homebrew` job skips cleanly when neither secret is set, so
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
