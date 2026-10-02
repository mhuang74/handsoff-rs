# Agent skills

### Issue tracker

Issues live in GitHub Issues for `mhuang74/handsoff-rs`; use the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### GUI testing limits

macOS TCC blocks agent-driven GUI verification here: no window screenshots (`screencapture` of app windows is denied — desktop-only), no synthetic clicks or menu automation (no assistive access for osascript/System Events). Verify UI changes by launching with `RUST_LOG=debug` and reading logs, numeric frame logging, or asking the user for a screenshot. See `docs/agents/gui-testing-limits.md`.
