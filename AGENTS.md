# Agent skills

### Issue tracker

Issues live in GitHub Issues for `mhuang74/handsoff-rs`; use the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### GUI testing limits

macOS TCC blocks agent-driven GUI verification here: no window screenshots (`screencapture` of app windows is denied — desktop-only), no synthetic clicks or menu automation (no assistive access for osascript/System Events). Verify UI changes by launching with `RUST_LOG=debug` and reading logs, numeric frame logging, or asking the user for a screenshot. See `docs/agents/gui-testing-limits.md`.

### Re-grant flow path requirement

TCC Accessibility grants are bound to the resolved binary path; the re-grant flow (and any grant) only works when the binary runs at the TCC-approved path — `/Applications/HandsOff.app/Contents/MacOS/handsoff-tray`. A debug build run from `target/debug` cannot be re-granted in-place; to smoke-test flows that hit re-grant, build release, copy the binary into a locally-built `/Applications/HandsOff.app` bundle (bundle id `handsoff-tray.handsoff` unchanged), and run from there.
