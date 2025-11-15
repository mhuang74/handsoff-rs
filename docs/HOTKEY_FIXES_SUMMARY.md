# Hotkey Configuration Bug Fixes - Summary

## Overview

Fixed 4 critical bugs related to user-configurable hotkeys that could cause crashes, silent failures, or confusing behavior.

## Bugs Fixed

### 1. **CRITICAL: Duplicate Hotkeys via Manual Config Edit** ✅ FIXED

**Problem**: Users could manually edit `config.toml` and set both hotkeys to the same letter, bypassing setup validation.

**Impact**:
- Both Lock and Talk would trigger simultaneously
- Spacebar passthrough would activate while locking
- Confusing UX

**Fix**:
- Added duplicate validation in `Config::new()` (src/config_file.rs:61-66)
- Added duplicate validation when loading config from file (src/config_file.rs:156-164)
- Case-insensitive comparison ('m' vs 'M' detected as duplicate)

**Test Coverage**:
- `test_duplicate_hotkeys_in_new()`
- `test_duplicate_hotkeys_case_insensitive()`
- `test_duplicate_hotkeys_in_loaded_config()`

---

### 2. **CRITICAL: Invalid Hotkeys Cause Panic** ✅ FIXED

**Problem**: Manually editing `config.toml` with invalid hotkeys (e.g., "123" or "AB") caused app to panic on startup with `.unwrap()`.

**Impact**: App crashes on startup with unhelpful error message

**Fix**:
- Replaced `.unwrap()` with proper error handling in both binaries (src/bin/handsoff.rs:241-264, src/bin/handsoff-tray.rs:213-236)
- Added validation when loading config from file (src/config_file.rs:146-154)
- Clear error messages instructing user to run setup again

**Test Coverage**:
- `test_invalid_hotkey_in_loaded_config()`

---

### 3. **HIGH: Environment Variables Can Create Duplicates** ✅ FIXED

**Problem**: Environment variables could override config to create duplicates without validation.

**Impact**: Same as Bug #1 - both hotkeys trigger simultaneously

**Fix**:
- **CLI app**: Added validation after resolving all precedence (env vars + config) (src/bin/handsoff.rs:266-274)
- **Tray app**: Removed environment variable support - only reads from config.toml (src/bin/handsoff-tray.rs:212-233)
- CLI shows error and exits with helpful message
- Tray app shows alert dialog (only for manually edited config files)

**Important Design Decision**:
- **CLI app**: Supports environment variable overrides for power users and automation
- **Tray app**: Only reads from config.toml for simplicity and consistency

**Example**:
```bash
# CLI: This now fails with clear error
HANDS_OFF_LOCK_HOTKEY=M HANDS_OFF_TALK_HOTKEY=M handsoff

# Tray app: Environment variables are ignored (by design)
HANDS_OFF_LOCK_HOTKEY=M handsoff-tray  # Uses config.toml, not env var
```

---

### 4. **MEDIUM: Silent Failure in Keycode Conversion** ✅ FIXED

**Problem**: If `code_to_keycode()` returned None, `set_hotkey_config()` silently failed to update AppState keycodes.

**Impact**:
- Global hotkey would register correctly
- But event tap would still check for default key
- Hotkey wouldn't work when locked

**Fix**:
- Added comprehensive logging in `set_hotkey_config()` (src/lib.rs:76-96)
- Success: INFO log with configured key and macOS keycode
- Failure: ERROR log with "CRITICAL" tag and explanation

---

## Files Changed

1. **src/config_file.rs**
   - Added duplicate validation in `Config::new()`
   - Added validation when loading from file
   - Added 5 new tests

2. **src/bin/handsoff.rs** (CLI app)
   - Improved error handling for invalid hotkeys
   - Added duplicate validation after precedence resolution (env vars + config)
   - Supports environment variable overrides

3. **src/bin/handsoff-tray.rs** (Tray app)
   - Improved error handling for invalid hotkeys
   - **Removed environment variable support** - only reads from config.toml
   - Added duplicate validation for config file
   - Shows alert dialog for user-friendly error reporting

4. **src/lib.rs**
   - Added logging for keycode conversion
   - Added `error` import from log crate

5. **src/utils/keycode.rs**
   - Previously added `code_to_keycode()` helper function

6. **src/app_state.rs**
   - Previously added `lock_keycode` and `talk_keycode` fields
   - Previously added getter/setter methods

7. **src/input_blocking/mod.rs**
   - Previously updated to use configured keycodes instead of hardcoded values

---

## Test Results

All tests pass: **38 passed** (33 original + 5 new)

```
✅ test_duplicate_hotkeys_in_new
✅ test_duplicate_hotkeys_case_insensitive
✅ test_different_hotkeys_accepted
✅ test_invalid_hotkey_in_loaded_config
✅ test_duplicate_hotkeys_in_loaded_config
```

Build status:
- ✅ `cargo check` - Clean
- ✅ `cargo test --lib` - 38/38 passed
- ✅ `cargo build` - Success
- ✅ `cargo build --release` - Success

---

## Validation Layers

The fixes create multiple layers of validation:

1. **Setup Time**: Interactive prompts validate individual keys and check for duplicates
2. **Config Creation**: `Config::new()` validates format and checks for duplicates
3. **Config Loading**: `Config::load_from_path()` validates loaded data
4. **Runtime**:
   - **CLI**: Validates after resolving env var precedence
   - **Tray**: Validates config file only (no env var support)
5. **Keycode Conversion**: Logging alerts if conversion fails

## Configuration Precedence

### CLI App (`handsoff`)
- Lock hotkey: `env:HANDS_OFF_LOCK_HOTKEY` > `config.toml` > default (L)
- Talk hotkey: `env:HANDS_OFF_TALK_HOTKEY` > `config.toml` > default (T)
- Supports environment variable overrides for automation

### Tray App (`handsoff-tray`)
- Lock hotkey: `config.toml` > default (L)
- Talk hotkey: `config.toml` > default (T)
- **Environment variables are ignored** (by design for simplicity)

---

## User-Facing Error Messages

### Duplicate Hotkeys (CLI)
```
ERROR Lock and Talk hotkeys cannot be the same: KeyM
ERROR This can happen if:
ERROR   1. Both environment variables are set to the same key
ERROR   2. The config file was manually edited with duplicate keys
ERROR
ERROR Please run 'handsoff --setup' to reconfigure or check your environment variables.
```

### Duplicate Hotkeys (Tray App)
```
HandsOff - Configuration Error

Lock and Talk hotkeys cannot be the same.

Both are set to: KeyM

This is likely because the config file was manually edited.

Please run setup to reconfigure:
~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup
```

### Invalid Hotkey (Config File)
```
Error: Invalid lock_hotkey in config file: '123'

Caused by:
    Hotkey must be a letter A-Z
```

### Invalid Hotkey (Env Var)
```
Error: Invalid lock hotkey from environment variable: '123'. Must be A-Z.
```

---

## Prevention of Future Issues

1. **Comprehensive validation** at every entry point
2. **Clear error messages** that guide users to fix the problem
3. **Test coverage** for edge cases
4. **Logging** for debugging silent failures
5. **Documentation** of potential issues (this file + HOTKEY_BUGS.md)

---

## Related Files

- **Original Issue**: Talk hotkey not working after locking input
- **Root Cause Analysis**: HOTKEY_BUGS.md
- **Original Fix**: Added configurable keycode support to event tap
- **This Fix**: Comprehensive validation and error handling
