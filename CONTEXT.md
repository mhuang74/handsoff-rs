# HandsOff

A menu-bar app that locks keyboard and mouse input until the user types their passphrase, so a machine can be handed off (or left) without unauthorized use.

## Language

**Lock**:
Blocking all keyboard and mouse input until the Passphrase is typed.
_Avoid_: freeze, block, secure

**Passphrase**:
A sequence of at least four physical key presses used to unlock. Layout-independent — what matters is the keys pressed, not the characters they produce.
_Avoid_: password, PIN, key

**Setup Wizard**:
The in-app, first-run window that grants Accessibility permission and captures the Passphrase. The sole setup path (ADR 0004 removed the terminal `--setup` flow with the CLI binary).
_Avoid_: installer, setup CLI, terminal setup

**Permission loss**:
Revoked Accessibility permissions (or a failed event-tap restart) quit the app immediately after one final notification explaining why and how to fix it. Recovery: re-grant the permission in System Settings and relaunch.

**Reset**:
Double-confirmed tray action that wipes the configuration and restarts the Setup Wizard. The recovery path for a forgotten Passphrase.
_Avoid_: reenable
