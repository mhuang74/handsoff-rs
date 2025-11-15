# Potential Bugs with User-Configurable Hotkeys

## 1. **CRITICAL: Duplicate Hotkeys via Manual Config Edit**

**Issue**: Users can manually edit `config.toml` and set both hotkeys to the same letter, bypassing setup validation.

**Impact**:
- Both Lock and Talk would trigger simultaneously when pressing the hotkey
- Spacebar passthrough would activate when trying to lock
- Confusing behavior

**Location**:
- Validation only in setup: `handsoff.rs:128-132`, `handsoff-tray.rs:96-100`
- No runtime validation when loading config

**Fix**: Add validation in both binaries after resolving all precedence (env vars, config file):
```rust
if lock_key == talk_key {
    error!("Lock and Talk hotkeys cannot be the same");
    std::process::exit(1);
}
```

---

## 2. **CRITICAL: Invalid Hotkeys Cause Panic**

**Issue**: If someone manually edits `config.toml` with invalid hotkeys (e.g., `lock_hotkey = "123"` or `lock_hotkey = "AB"`), the app will panic.

**Impact**: App crashes on startup with unhelpful error

**Location**:
- `handsoff.rs:243-251`, `handsoff-tray.rs:215-223`
- Uses `.unwrap()` on `get_lock_key_code()` which can fail

**Current Code**:
```rust
cfg.get_lock_key_code().context("Failed to parse lock hotkey")?
```

**Problem**: The inner `parse_key_string()` returns `Result`, but if it fails, the unwrap inside `unwrap_or_else` will panic.

**Fix**: Better error handling with clear message to run setup again

---

## 3. **HIGH: Env Var Overrides Can Create Duplicate Hotkeys**

**Issue**: Environment variables can override config to create duplicates:
```bash
HANDS_OFF_LOCK_HOTKEY=M HANDS_OFF_TALK_HOTKEY=M handsoff
```

**Impact**: Same as #1 - both hotkeys trigger simultaneously

**Location**: No validation after resolving precedence in both binaries

**Fix**: Validate final resolved keys are different

---

## 4. **MEDIUM: Silent Failure if code_to_keycode Returns None**

**Issue**: If a non-letter Code somehow gets through, `code_to_keycode()` returns None, and `set_hotkey_config()` silently fails to update AppState keycodes.

**Current Code** (lib.rs:75-81):
```rust
if let Some(lock_keycode) = utils::keycode::code_to_keycode(lock_key) {
    self.state.set_lock_keycode(lock_keycode);
}
```

**Impact**:
- Global hotkey registers correctly (e.g., "M")
- But event tap still looks for default "L" (keycode 37)
- Hotkey won't work when locked

**Likelihood**: Low - all validated keys are A-Z which are in the match

**Fix**: Add logging or error if conversion fails

---

## 5. **LOW: Inconsistent Error Handling Between CLI and Tray**

**Issue**: Both binaries have similar but slightly different hotkey loading logic

**Impact**: Maintenance burden, potential for divergence

**Fix**: Extract common hotkey resolution logic into a helper function

---

## 6. **LOW: No Validation of Spacebar Conflict**

**Issue**: If user configures Talk hotkey as something, spacebar is still hardcoded for passthrough.

**Current Code** (input_blocking/mod.rs:56-58):
```rust
if state.is_talk_key_pressed() && keycode == 49 {
    // Keycode 49 is spacebar
```

**Impact**: None currently - spacebar passthrough is the intended behavior

**Note**: This is actually not a bug, but if we wanted to make spacebar configurable in the future, we'd need to update this

---

## Recommendations (Priority Order):

1. **Add duplicate hotkey validation** after resolving all precedence (env vars + config)
2. **Improve error handling** for invalid config file keys
3. **Add logging** in `set_hotkey_config` if `code_to_keycode` returns None
4. **Extract common logic** for hotkey resolution into a helper function
5. **Add integration test** for config file edge cases
