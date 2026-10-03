# GitHub Actions Workflows

## AI Code Review Workflow

The `code-review.yml` workflow automatically reviews pull requests using an AI model via OpenRouter.

### Triggering

Runs automatically on pull request events: `opened`, `synchronize`, `reopened`, and `ready_for_review`.

### What the Workflow Does

1. Checks out the PR code
2. Sends the diff to the OpenRouter AI review action (`jonit-dev/openrouter-github-action`, pinned to a full commit SHA because the action's `main` branch no longer ships a built `dist/`)
3. Runs the AI review if the PR carries the `ai-review` label (`review_label` acts as a gate — only labeled PRs are reviewed; the label is not applied to comments)

The review prompt instructs the model to act as a senior engineer — explore the repository, understand intended behavior, inspect callers/tests, and report only actionable defects, limited to 3 critical/high-priority suggestions.

### Model Configuration

The review model is set via `model_id` in the workflow. Current model: **`z-ai/glm-5.3-flash`** (cheap, fast). Commented-out alternatives in the workflow:

- `openai/gpt-4o-mini` — cheap and fast
- `anthropic/claude-sonnet-4.5` — catches nuanced bugs

To change the model, edit the active `model_id` line in `code-review.yml`.

### Required Secrets

1. **OPEN_ROUTER_KEY** — OpenRouter API key (Settings → Secrets and variables → Actions)

`GITHUB_TOKEN` is provided automatically.

### Concurrency

Reviews are grouped per PR; an in-progress review is cancelled when the PR is updated (`cancel-in-progress: true`).

## Release Workflow

The `release.yml` workflow automatically builds and releases macOS DMG disk images (containing the ad-hoc signed HandsOff.app) for HandsOff, for both Apple Silicon (arm64) and Intel (x86_64). The app ships unsigned (ad-hoc signed) per [docs/adr/0001-unsigned-notarization-free-distribution.md](../../docs/adr/0001-unsigned-notarization-free-distribution.md); first-run configuration is handled by the built-in Setup Wizard ([docs/adr/0002-in-app-setup-wizard-replaces-cli-setup.md](../../docs/adr/0002-in-app-setup-wizard-replaces-cli-setup.md)).

### Triggering a Release

The workflow can be triggered in two ways:

1. **Push a Git tag** (recommended):
   ```bash
   git tag v0.4.0
   git push origin v0.4.0
   ```

2. **Manual workflow dispatch**:
   - Go to Actions → Release workflow
   - Click "Run workflow"
   - Enter the tag name (e.g., `v0.4.0`)

### What the Workflow Does

The workflow runs four jobs:

1. **Build arm64** (`build-macos-dmg-arm64`): builds and packages the app for Apple Silicon (aarch64-apple-darwin)
2. **Build x86_64** (`build-macos-dmg-x86_64`): builds and packages the app for Intel (x86_64-apple-darwin)
3. **Changelog** (`changelog`): generates changelog entries and updates `CHANGELOG.md` on `main`
4. **Release** (`release`): waits for the above jobs, then creates the GitHub Release with generated notes

Each build job:
1. Builds the Rust project for its target architecture
2. Creates the macOS app bundle using cargo-bundle
3. Fixes the Info.plist to add LSUIElement (menu bar app)
4. Ad-hoc signs the app bundle (`codesign --deep --force --sign -`; no certificate needed)
5. Packages the app bundle into a DMG with `hdiutil create -format UDZO` (staged with an `/Applications` symlink for drag-and-drop)
6. Uploads the DMG as a workflow artifact

The release job:
1. Downloads the two DMG build artifacts (per-architecture)
2. Creates a GitHub Release with generated release notes
3. Uploads both artifacts to the release

### Signing

No certificate setup is required: the app bundle is ad-hoc signed in CI. On first launch, users right-click the app and choose **Open** to pass Gatekeeper (see the [DMG guide](../../docs/DMG-GUIDE.md) and ADR 0001). Developer ID signing and notarization are intentionally deferred until a paid Apple Developer account exists (ADR 0001).

### Workflow Output

The workflow creates:
1. **GitHub Release**: Automatically created with release notes, containing:
   - `HandsOff-v{VERSION}-arm64.dmg` (Apple Silicon DMG)
   - `HandsOff-v{VERSION}-x86_64.dmg` (Intel DMG)
2. **Workflow artifacts**: each build job uploads its DMG (retained for 30 days); the release job uploads a combined `release-bundles` artifact containing both DMG files

### Architecture Support

The workflow builds for both **Apple Silicon (arm64)** and **Intel (x86_64)** in parallel jobs, producing separate per-architecture DMGs. The x86_64 build cross-compiles from the arm64 macOS runner (both targets are Tier 1 for macOS).

### Customization

To customize the workflow:

- **Change target architectures**: Modify the `--target` flags in the build steps of both build jobs
- **Add universal binary support**: Use `lipo` to combine the arm64 and x86_64 binaries
- **Notarization**: Add notarization steps after signing (requires Apple Developer account and app-specific password; see ADR 0001 for the deferral rationale)
- **Auto-update version**: Sync version numbers between Cargo.toml and git tags

### Testing the Workflow

Before pushing a real release tag, you can test using:
1. Manual workflow dispatch with a test tag name
2. Push to a test branch and temporarily modify the workflow trigger
3. Fork the repository and test in your fork first

### Troubleshooting

**Build fails with "cargo-bundle not found"**:
- The workflow installs cargo-bundle automatically; this shouldn't happen

**Bundle not found**:
- Check that the bundle rename logic matches your project structure
- Verify cargo-bundle configuration in Cargo.toml

**DMG won't mount or app is blocked by Gatekeeper**:
- Verify the ad-hoc signature: `codesign -dv /Applications/HandsOff.app` (expect `Signature=adhoc`)
- First launch requires right-click → Open; see the [DMG guide](../../docs/DMG-GUIDE.md)

### Related Files

- Makefile: Local build commands (including `make dmg`)
- docs/DMG-GUIDE.md: DMG distribution and first-run flow
- Cargo.toml: Package metadata and bundle configuration
