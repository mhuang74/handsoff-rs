# GitHub Actions Workflows

## Release Workflow

The `release.yml` workflow automatically builds and releases macOS PKG installers and CLI tarballs for HandsOff, for both Apple Silicon (arm64) and Intel (x86_64).

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

1. **Build arm64** (`build-macos-pkg-arm64`): builds and packages the app for Apple Silicon (aarch64-apple-darwin)
2. **Build x86_64** (`build-macos-pkg-x86_64`): builds and packages the app for Intel (x86_64-apple-darwin)
3. **Changelog** (`changelog`): generates changelog entries and updates `CHANGELOG.md` on `main`
4. **Release** (`release`): waits for the above jobs, then creates the GitHub Release with generated notes

Each build job:
1. Builds the Rust project for its target architecture
2. Creates the macOS app bundle using cargo-bundle
3. Fixes the Info.plist to add LSUIElement (menu bar app)
4. Signs the app bundle (if certificates are configured)
5. Creates a PKG installer with launch agent setup
6. Signs the PKG installer (if certificates are configured)
7. Uploads the PKG installer and CLI tarball as workflow artifacts

The release job:
1. Downloads all four build artifacts (per-architecture PKG installers and CLI tarballs)
2. Creates a GitHub Release with generated release notes
3. Uploads all four artifacts to the release

### Code Signing Setup (Optional)

For distribution outside of your development environment, you should set up proper code signing with Apple Developer certificates.

#### Prerequisites

1. **Apple Developer Account**: You need a paid Apple Developer account ($99/year)
2. **Developer ID Certificates**: Two certificates are required:
   - Developer ID Application (for signing the app bundle)
   - Developer ID Installer (for signing the PKG)

#### Creating and Exporting Certificates

1. **Generate certificates in Apple Developer Portal**:
   - Go to https://developer.apple.com/account/resources/certificates
   - Create "Developer ID Application" certificate
   - Create "Developer ID Installer" certificate
   - Download and install both certificates in Keychain Access

2. **Export certificate as P12**:
   - Open Keychain Access
   - Find your "Developer ID Application" certificate
   - Right-click → Export
   - Choose file format: Personal Information Exchange (.p12)
   - Set a strong password (you'll need this for the secret)
   - Save the file

3. **Convert to base64**:
   ```bash
   base64 -i YourCertificate.p12 | pbcopy
   ```
   This copies the base64-encoded certificate to your clipboard

#### GitHub Secrets Configuration

Add the following secrets to your GitHub repository (Settings → Secrets and variables → Actions):

1. **MACOS_CERTIFICATE**
   - The base64-encoded P12 certificate (from step 3 above)
   - Should include both Application and Installer certificates

2. **MACOS_CERTIFICATE_PWD**
   - The password you set when exporting the P12 file

3. **MACOS_KEYCHAIN_PWD**
   - A random password for the temporary keychain (can be any strong password)
   - Example: `openssl rand -base64 32`

#### Without Code Signing

If you don't configure the secrets above, the workflow will still run and create an **unsigned** PKG installer. This is fine for:
- Development testing
- Internal distribution
- Open source projects where users can build from source

Users installing unsigned packages will need to:
- Right-click the PKG and choose "Open" (first time only)
- Confirm they want to install from an unidentified developer

### Workflow Output

The workflow creates:
1. **GitHub Release**: Automatically created with release notes, containing:
   - `HandsOff-v{VERSION}-arm64.pkg` (Apple Silicon PKG installer)
   - `HandsOff-v{VERSION}-x86_64.pkg` (Intel PKG installer)
   - `handsoff-cli-v{VERSION}-arm64.tar.gz` (Apple Silicon CLI)
   - `handsoff-cli-v{VERSION}-x86_64.tar.gz` (Intel CLI)
2. **Workflow artifacts**: each build job uploads its PKG and CLI tarball (retained for 30 days); the release job uploads a combined `release-bundles` artifact containing all four files

### Architecture Support

The workflow builds for both **Apple Silicon (arm64)** and **Intel (x86_64)** in parallel jobs, producing separate per-architecture installers and CLI tarballs. The x86_64 build cross-compiles from the arm64 macOS runner (both targets are Tier 1 for macOS).

### Customization

To customize the workflow:

- **Change target architectures**: Modify the `--target` flags in the build steps of both build jobs
- **Add universal binary support**: Use `lipo` to combine the arm64 and x86_64 binaries
- **Notarization**: Add notarization steps after signing (requires Apple Developer account and app-specific password)
- **Auto-update version**: Sync version numbers between Cargo.toml and git tags

### Testing the Workflow

Before pushing a real release tag, you can test using:
1. Manual workflow dispatch with a test tag name
2. Push to a test branch and temporarily modify the workflow trigger
3. Fork the repository and test in your fork first

### Troubleshooting

**Build fails with "cargo-bundle not found"**:
- The workflow installs cargo-bundle automatically; this shouldn't happen

**Code signing fails**:
- Verify all three secrets are set correctly
- Ensure the P12 file includes both certificates
- Check certificate expiration in Apple Developer Portal

**PKG verification fails**:
- This is expected if no signing certificates are configured
- The unsigned PKG will still work but show warnings to users

**Bundle not found**:
- Check that the bundle rename logic matches your project structure
- Verify cargo-bundle configuration in Cargo.toml

### Related Files

- Makefile: Local build commands (including `make pkg`)
- installer/build-pkg.sh: Local PKG build script
- Cargo.toml: Package metadata and bundle configuration
