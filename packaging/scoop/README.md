# Scoop packaging

This directory contains the [Scoop](https://scoop.sh) manifest template for roxy
(Windows, amd64 + arm64) and instructions for publishing it to a personal bucket.

## One-time setup

1. **Create a bucket repository** on GitHub named `scoop-bucket`
   (e.g. `github.com/petstack/scoop-bucket`).

2. **Add the manifest** under a `bucket/` directory (Scoop's convention):
   ```bash
   git clone https://github.com/petstack/scoop-bucket
   cd scoop-bucket
   mkdir -p bucket
   cp /path/to/roxy/packaging/scoop/roxy.json bucket/roxy.json
   ```

3. **Fill in the version and SHA256 values** for the first release. Replace every
   `REPLACE_WITH_VERSION` with the version (e.g. `0.2.0`), and each
   `REPLACE_WITH_*_SHA256` with the hash from the published `.zip.sha256` file:
   ```powershell
   (Get-FileHash -Algorithm SHA256 .\roxy-v0.2.0-x86_64-pc-windows-msvc.zip).Hash.ToLower()
   ```

4. **Commit and push**:
   ```bash
   git add bucket/roxy.json
   git commit -m "roxy 0.2.0"
   git push
   ```

## Users install with

```powershell
scoop bucket add petstack https://github.com/petstack/scoop-bucket
scoop install roxy
```

Scoop automatically selects the `64bit` (amd64) or `arm64` artifact for the host.

## Automatic updates on new releases

The main `roxy` release workflow contains a `scoop` job that updates the manifest
automatically on each tagged release. The job:

1. Checks out this template (`packaging/scoop/roxy.json`) and the bucket repo
2. Substitutes the new version into the static URL/`extract_dir`/`version` fields
   (the `autoupdate` block keeps Scoop's own literal `$version` token)
3. Fetches both Windows `*.zip.sha256` files from the just-published GitHub Release
4. Substitutes the two SHA256 values into the template
5. Commits and pushes the updated `bucket/roxy.json` to the bucket repo

This handles both Windows architectures (amd64 + arm64) in a single deterministic
step — no third-party action involved.

### Setup

1. **Create a GitHub Personal Access Token (PAT)** with `contents: write`
   permission on your `petstack/scoop-bucket` repository. A fine-grained token
   scoped only to that repo is recommended.

2. **Add the PAT as a secret** in the `petstack/roxy` repository:
   - Settings → Secrets and variables → Actions → New repository secret
   - Name: `SCOOP_BUCKET_TOKEN`
   - Value: the PAT you created

3. **On the next tagged release**, the workflow commits directly to the bucket
   repo's `main` branch with the updated version and SHA256 values.

If the secret is not set, the auto-update job is skipped cleanly — you can still
maintain the manifest manually.

## winget (future option)

A [winget](https://learn.microsoft.com/windows/package-manager/) manifest is not
shipped yet. Unlike Scoop's self-hosted bucket, winget requires submitting a pull
request into the central `microsoft/winget-pkgs` repository for every release
(typically via [`wingetcreate`](https://github.com/microsoft/winget-create)).
That heavier automation can be layered on later; the portable `.exe` and `.zip`
release assets are sufficient for `wingetcreate` to consume.
