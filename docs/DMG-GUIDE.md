# HandsOff DMG Distribution Guide

How HandsOff is packaged and distributed as a macOS app: an unsigned
(ad-hoc signed) `.app` bundle inside a mountable DMG, with first-run
configuration handled by the built-in Setup Wizard.

## The Approach: DMG + Setup Wizard

HandsOff ships as `HandsOff-v{VERSION}-<arch>.dmg`:

1. **Mount the DMG** and drag **HandsOff.app** to `/Applications`
   (the DMG window includes an Applications symlink for drag-and-drop).
2. **First launch**: because the app is unsigned (ad-hoc signed only),
   macOS Gatekeeper may block it. Right-click (or Control-click)
   HandsOff.app and choose **Open**, then confirm **Open** in the dialog.
   You only need to do this once. See
   [docs/adr/0001-unsigned-notarization-free-distribution.md](adr/0001-unsigned-notarization-free-distribution.md)
   for why HandsOff ships unsigned and notarization-free.
3. **Setup Wizard**: on first run with no valid config, the app
   automatically opens the in-app Setup Wizard — it grants Accessibility
   permission (to HandsOff itself, not a terminal), captures your secret
   Passphrase as physical key presses, and offers hotkey/timeout
   settings plus a login item. See
   [docs/adr/0002-in-app-setup-wizard-replaces-cli-setup.md](adr/0002-in-app-setup-wizard-replaces-cli-setup.md)
   for why the wizard replaces the old terminal `--setup` flow.

The wizard writes `~/Library/Application Support/handsoff/config.toml`
(SHA-256 hashed Passphrase, 600 permissions). Reconfigure any time via
**Preferences** / **Change Passphrase** in the tray menu.

## Building the DMG

On a Mac (requires macOS tools: `cargo-bundle`, `plutil`, `codesign`,
`hdiutil`):

```bash
make dmg
```

Output: `dist/HandsOff-v{VERSION}-<arch>.dmg` where `<arch>` is the
build machine's architecture (`arm64` or `x86_64`).

What the target does:

1. `cargo build --release` — builds both binaries (`handsoff`,
   `handsoff-tray`) from the same crate.
2. `cargo bundle --release --bin handsoff-tray` — creates the `.app`
   bundle and renames it to `HandsOff.app`.
3. `plutil` — inserts `LSUIElement` (menu bar only, no Dock icon).
4. `codesign --deep --force --sign -` — ad-hoc signature (no identity
   needed; satisfies nothing beyond local run, per ADR 0001).
5. Stages `HandsOff.app` plus an `/Applications` symlink and runs
   `hdiutil create -format UDZO` to produce the compressed DMG.

The CLI binary (`target/release/handsoff`) is built by the same step and
is distributed separately (e.g. zipped or tarballed); it is not part of
the DMG.

## Testing the DMG

```bash
make dmg
hdiutil attach dist/HandsOff-v{VERSION}-<arch>.dmg
# Drag HandsOff.app to /Applications, launch it (right-click → Open on
# first launch), complete the Setup Wizard.
hdiutil detach "/Volumes/HandsOff"
```

Verify the ad-hoc signature:

```bash
codesign -dv /Applications/HandsOff.app 2>&1 | grep -E 'Signature|flags'
# Expected: Signature=adhoc
```

## Updating / Reinstalling

Each release has a different ad-hoc signature (CDHash), so macOS treats
every build of HandsOff as a distinct app. When you replace
`/Applications/HandsOff.app` with a new version, the old Accessibility
entry no longer matches — the permission looks granted but is stale, and
input blocking will not work.

Before reinstalling, delete the old permission entry first:

```bash
tccutil reset Accessibility handsoff-tray.handsoff
```

Then quit the running app, replace the bundle, launch the new one, and
grant the Accessibility permission again (System Settings → Privacy &
Security → Accessibility, or via the in-app Setup Wizard's permission
step). Re-adding without deleting the old entry first can leave both a
stale and a fresh entry, and the stale one may keep winning.

Alternatively, if you reinstall without the reset and the app reports a
permission problem, use the tray menu's
**Fix Accessibility Permission…** — it detects the stale grant and
performs the reset + relaunch for you.

## Uninstalling

Quit the app from the tray menu, then:

```bash
rm -rf /Applications/HandsOff.app
rm -rf "$HOME/Library/Application Support/handsoff"
```

(If you enabled the login item, it is removed automatically when the
app is deleted; or toggle it off via
System Settings → General → Login Items first.)

## References

- [BUILD.md](../BUILD.md) - full build instructions
- [docs/adr/0001-unsigned-notarization-free-distribution.md](adr/0001-unsigned-notarization-free-distribution.md)
- [docs/adr/0002-in-app-setup-wizard-replaces-cli-setup.md](adr/0002-in-app-setup-wizard-replaces-cli-setup.md)
- [Makefile](../Makefile) - all available build targets
