# HandsOff - Quick Start Safety Guide

## ⚠️ WARNING
**This app blocks ALL keyboard and mouse input when locked. Read this guide before running!**

---

## 🚀 Safe Development in 3 Steps

### Step 1: Enable SSH (Your Escape Route)
```bash
# Run this FIRST - it's your safety net!
sudo systemsetup -setremotelogin on

# Get your Mac's network name
hostname

# Test from your phone or another computer:
ssh youruser@your-mac.local
pkill handsoff-tray  # This will save you if locked out
```

**Why?** If you get locked out, you can kill the app remotely.

---

### Step 2: First Run - Safe Mode
```bash
# Copy this command exactly:
HANDS_OFF_AUTO_UNLOCK=60 cargo run --bin handsoff-tray

# What this does:
# - Auto-unlock: the first window opens after 60 s of awake time
#   (60 is the minimum base interval; the window stays open until you
#   lock it again or touch input for auto-lock's grace period)
```

**What to test:**
1. Set a passphrase first (launch the tray app — the Setup Wizard opens
   automatically) — remember it!
2. Lock via the hotkey `Ctrl+Cmd+Shift+L` (or tray menu → Lock Input) — try typing: input is blocked
3. Type your passphrase to unlock
4. For the shortest auto-lock, also use `HANDS_OFF_AUTO_LOCK=20 cargo run --bin handsoff-tray`

---

### Step 3: Progressive Testing
Once the basic cycle works, exercise the full lock → block → unlock path:

```bash
# Lock via the hotkey (Ctrl+Cmd+Shift+L by default)
# → Verify keyboard AND mouse are blocked
# → Type your passphrase to unlock
# → Repeat: lock again, unlock again

# With the shortest auto-lock (20 s):
HANDS_OFF_AUTO_LOCK=20 cargo run --bin handsoff-tray
# → Wait 20 s idle: it re-locks itself
# → Type your passphrase to unlock
```

---

## 🆘 Emergency Recovery

### If You Get Locked Out:

**Method 1: Wait (if auto-unlock is enabled)**
- Run with `HANDS_OFF_AUTO_UNLOCK=60` during testing: the first window opens after 60 s of awake time and input is released (no notification)

**Method 2: SSH Kill (RECOMMENDED)**
```bash
# From phone/another computer:
ssh youruser@your-mac.local
pkill handsoff-tray
```

**Method 3: Force Restart (LAST RESORT)**
- Hold power button for 10 seconds
- Mac will force restart
- You'll lose unsaved work!

---

## 📝 Development Checklist

Before EVERY development session:

- [ ] SSH is enabled and tested
- [ ] I know my passphrase (write it down!)
- [ ] `HANDS_OFF_AUTO_UNLOCK=60` is in my command during testing
- [ ] Another terminal/device ready to kill process
- [ ] Changes committed to git
- [ ] I've read this guide

---

## 🧪 Running Tests (Always Safe)

Unit tests are completely safe and don't block input:

```bash
# Run all tests (inline #[cfg(test)] modules plus tests/ integration-style files)
cargo test

# Run one file's tests, e.g.
cargo test --test auth_tests

# Tests complete in ~1 second
```

**Inline unit tests cover:**
- ✅ Passphrase hashing/verification
- ✅ Keycode conversion
- ✅ Config parsing (config.toml + env overrides)
- ✅ State management (lock state, backoff schedule)

---

## 🔑 Default Hotkeys

Memorize these BEFORE testing:

| Action | Hotkey | Purpose |
|--------|--------|---------|
| Lock | `Ctrl+Cmd+Shift+L` | Enable input lock |
| Talk | `Ctrl+Cmd+Shift+T` | Hold + Spacebar to unmute |

Hotkeys are configurable via the Setup Wizard / Preferences (stored in `config.toml`).

---

## 🎯 What to Test Manually

After running unit tests, manually test:

### Phase 1: Basic Lock/Unlock
1. Set passphrase
2. Lock via menu
3. Verify keyboard blocked
4. Type passphrase to unlock
5. Verify unlock notification

### Phase 2: Hotkeys
1. Lock via `Ctrl+Cmd+Shift+L`
2. Unlock via passphrase
3. Lock again
4. Hold `Ctrl+Cmd+Shift+T` and press Spacebar
5. Lock a third time, unlock again via passphrase

### Phase 3: Auto-Lock
1. Set auto-lock to its minimum via `HANDS_OFF_AUTO_LOCK=20` (or set a short timeout in the Setup Wizard / Preferences); the default is 180 s
2. Wait 20 seconds idle
3. Verify auto-lock triggers
4. Move mouse - verify timer resets

### Phase 4: Video Call (Real Test)
1. Join Zoom/Google Meet
2. Lock input
3. Verify video/audio still works
4. Test Talk hotkey to unmute
5. Unlock and verify notification visible

---

## 📚 Full Documentation

- **Developer guide**: `DEVELOPER.md`
- **Auto-unlock testing**: `docs/TESTING-AUTO-UNLOCK.md`
- **Auto-unlock quick reference**: `docs/AUTO-UNLOCK-QUICK-REFERENCE.md`
- **Original spec**: `specs/handsoff-design.md`

---

## 🐛 Common Issues

### "Accessibility permissions not granted"
```bash
# Grant in: System Settings > Privacy & Security > Accessibility
# Add Terminal (or your IDE)
# Restart the app
```

### "I forgot my passphrase!"
```bash
# From another terminal or SSH:
pkill handsoff-tray

# Delete the config (clears the stored passphrase hash), then relaunch the
# tray app — the Setup Wizard opens automatically.
rm ~/Library/Application\ Support/handsoff/config.toml
```

### "App won't quit"
```bash
# Force quit:
pkill -9 handsoff-tray
```

### "Locked out and can't SSH"
- Force restart Mac (hold power button)
- Next time, enable SSH first!

---

## 💡 Pro Tips

1. **Always have Terminal.app open** in another desktop/space
2. **Keep a text file with your passphrase** during development
3. **Test on a secondary user account** first
4. **Use a VM** for risky testing
5. **Never test in production mode** without SSH ready
6. **Commit your code** before testing (in case of force restart)
7. **Set short timeouts** during testing (`HANDS_OFF_AUTO_LOCK=20`, not the 180 s default)

---

## ✅ Ready to Start?

Run these commands in order:

```bash
# 1. Enable your safety net
sudo systemsetup -setremotelogin on

# 2. Run tests (always safe)
cargo test

# 3. First safe run (auto-unlock window opens after 60 s awake-time)
HANDS_OFF_AUTO_UNLOCK=60 cargo run --bin handsoff-tray

# 4. If that worked, test the shortest auto-lock:
HANDS_OFF_AUTO_LOCK=20 cargo run --bin handsoff-tray
```

---

**Remember**: Better safe than sorry! Always have an escape route. 🚪

**Questions?** See `docs/SAFE-DEVELOPMENT.md` for detailed scenarios.

---

*This tool is powerful. With great power comes great responsibility (and an SSH session).*
