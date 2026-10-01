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
The in-app, first-run window that grants Accessibility permission and captures the Passphrase. Replaces the old terminal-based `--setup` flow for average users.
_Avoid_: installer, setup CLI

**Disable**:
Manual suspension of protection from the tray menu. Input is not blocked and the Passphrase is not required.
_Avoid_: pause, turn off

**Reenable**:
Tray action (renamed from "Reset") that ends a stuck Lock and restarts input capture without changing any configuration.
_Avoid_: reset

**Reset**:
Double-confirmed tray action that wipes the configuration and restarts the Setup Wizard. The recovery path for a forgotten Passphrase.
_Avoid_: reenable (that means something else now)
