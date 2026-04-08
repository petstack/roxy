# Homebrew packaging

This directory contains the Homebrew formula template for roxy and instructions
for publishing it to a personal tap.

## One-time setup

1. **Create a tap repository** on GitHub named `homebrew-tap`
   (e.g. `github.com/petstack/homebrew-tap`). The `homebrew-` prefix is required
   by Homebrew.

2. **Add the formula**:
   ```bash
   git clone https://github.com/petstack/homebrew-tap
   cd homebrew-tap
   mkdir -p Formula
   cp /path/to/roxy/packaging/homebrew/roxy.rb Formula/roxy.rb
   ```

3. **Fill in the SHA256 values** for each archive in the first release.
   For each target, get the hash from the published `.tar.gz.sha256` file,
   or compute it locally:
   ```bash
   curl -sL https://github.com/petstack/roxy/releases/download/v0.1.0/roxy-v0.1.0-aarch64-apple-darwin.tar.gz | shasum -a 256
   ```
   Replace `REPLACE_WITH_*_SHA256` placeholders in `Formula/roxy.rb`.

4. **Commit and push**:
   ```bash
   git add Formula/roxy.rb
   git commit -m "roxy 0.1.0"
   git push
   ```

## Users install with

```bash
brew tap petstack/tap
brew install roxy

# or one-liner:
brew install petstack/tap/roxy
```

## Automatic updates on new releases

The main `roxy` release workflow contains a `homebrew` job that updates the
formula automatically on each tagged release. The job:

1. Checks out this template (`packaging/homebrew/roxy.rb`) and the tap repo
2. Fetches all four `*.sha256` files from the just-published GitHub Release
3. Substitutes the new version + four SHA256 values into the template
4. Commits and pushes the updated `Formula/roxy.rb` to the tap repo

This handles all four architectures (macOS arm64/x86_64, Linux musl arm64/x86_64)
in a single deterministic step — no third-party action involved.

### Setup

1. **Create a GitHub Personal Access Token (PAT)** with `contents: write`
   permission on your `petstack/homebrew-tap` repository. A fine-grained token
   is recommended, scoped only to that repo.

2. **Add the PAT as a secret** in the `petstack/roxy` repository:
   - Settings → Secrets and variables → Actions → New repository secret
   - Name: `HOMEBREW_TAP_TOKEN`
   - Value: the PAT you created

3. **On the next tagged release**, the workflow will commit directly to the tap
   repo's `main` branch with updated version and SHA256 values for all four
   targets.

If the secret is not set, the auto-update job is skipped cleanly — you can
still maintain the formula manually.

## Testing the formula locally

Before pushing changes to the tap, validate the formula:

```bash
brew install --build-from-source ./Formula/roxy.rb
brew audit --strict --online roxy
brew test roxy
```
