# Remove the CLI binary; the tray app with its Setup Wizard is the sole distribution and setup path

The `handsoff` CLI binary, its release tarballs, and the tray's undocumented
`--setup` terminal flow are removed. The Setup Wizard in the tray app is now
the sole setup path: launching with no config (or an invalid one) opens the
wizard automatically, and Reset re-enters it — this completes ADR 0002's
cutover, which had kept the CLI/TUI setup flow alive as a power-user path.
The terminal TUI setup stack (`run_interactive_setup`, the terminal
`capture_passphrase` wrapper, the stdin prompt helpers, and the
`HANDS_OFF_LOCK_HOTKEY` / `HANDS_OFF_TALK_HOTKEY` env-var hotkey overrides
that only that flow honored) is deleted as dead code. The tray runtime never
honored the hotkey env overrides, so the TUI flow's reserved-key logic had
drifted from the tray's actual behavior.

`HANDS_OFF_AUTO_LOCK` and `HANDS_OFF_AUTO_UNLOCK` env overrides remain
supported: the tray runtime resolves them at startup and on Preferences
apply.

With the CLI gone, remote provisioning over SSH is no longer offered: the
wizard and the TCC Accessibility grant both require the GUI console, so the
tray app cannot be set up without physical access to the machine.

Supersedes the CLI-artifact clause of
[ADR 0002](0002-in-app-setup-wizard-replaces-cli-setup.md) and spec stories
21–22 (`specs/in-app-setup-wizard.md`), which remain as historical records.

## Consequences

- `cargo build --release` produces a single binary (`handsoff-tray`);
  release CI packages only the two per-architecture DMGs.
- The `clap` dependency stays: the tray still derives `Parser` for the
  internal `--skip-instance-lock` relaunch flag.
- Removing the CLI closes a latent trap: a CLI setup (with env-var hotkey
  overrides) could produce a config whose reserved-key assumptions the tray
  runtime would not honor.
