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
pkill handsoff  # This will save you if locked out
```

**Why?** If you get locked out, you can kill the app remotely.

---

### Step 2: First Run - Safe Mode
```bash
# Copy this command exactly:
HANDSOFF_DEV_MODE=1 HANDSOFF_DRY_RUN=1 cargo run

# What this does:
# - DEV_MODE: Auto-unlocks after 10 seconds
# - DRY_RUN: Logs what would be blocked but doesn't actually block
```

**What to test:**
1. Set a passphrase (remember it!)
2. Click "Enable Lock"
3. Try typing - it should still work
4. Watch logs to see what would be blocked
5. Type your passphrase to unlock

---

### Step 3: Progressive Testing
Once dry-run works, test incrementally:

```bash
# Test 1: Block mouse only (keyboard still works!)
HANDSOFF_DEV_MODE=1 BLOCK_MOUSE=1 cargo run
# → Try moving mouse, verify you can still type passphrase

# Test 2: Block keyboard only
HANDSOFF_DEV_MODE=1 BLOCK_KEYBOARD=1 cargo run
# → Try typing, verify you can still use mouse to quit

# Test 3: Full blocking with 10-second auto-unlock
HANDSOFF_DEV_MODE=1 BLOCK_KEYBOARD=1 BLOCK_MOUSE=1 cargo run
# → Lock it, wait 10 seconds, it auto-unlocks
```

---

## 🆘 Emergency Recovery

### If You Get Locked Out:

**Method 1: Wait (if in dev mode)**
- Dev mode auto-unlocks after 10 seconds
- Just wait it out!

**Method 2: SSH Kill (RECOMMENDED)**
```bash
# From phone/another computer:
ssh youruser@your-mac.local
pkill handsoff
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
- [ ] HANDSOFF_DEV_MODE=1 is in my command
- [ ] Another terminal/device ready to kill process
- [ ] Changes committed to git
- [ ] I've read this guide

---

## 🧪 Running Tests (Always Safe)

Unit tests are completely safe and don't block input:

```bash
# Run all tests
cargo test

# Run specific test file
cargo test --test auth_tests

# Tests complete in ~1 second
```

**32 tests covering:**
- ✅ Passphrase hashing/verification
- ✅ State management
- ✅ Keycode conversion
- ✅ Thread safety

---

## 🔑 Default Hotkeys

Memorize these BEFORE testing:

| Action | Hotkey | Purpose |
|--------|--------|---------|
| Lock | `Ctrl+Cmd+Shift+L` | Enable input lock |
| Talk | `Ctrl+Cmd+Shift+T` | Hold + Spacebar to unmute |
| Touch ID | `Ctrl+Cmd+Shift+U` | Trigger Touch ID unlock |

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
5. Unlock via `Ctrl+Cmd+Shift+U` (Touch ID)

### Phase 3: Auto-Lock
1. Set timeout to 30 seconds in code
2. Wait 30 seconds idle
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

- **Complete safety guide**: `docs/SAFE-DEVELOPMENT.md`
- **Test details**: `docs/TESTING-SUMMARY.md`
- **Phase 2 plan**: `specs/phase-2.md`
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
pkill handsoff

# Or delete keychain entry:
security delete-generic-password -s com.handsoff.inputlock -a passphrase_hash
```

### "App won't quit"
```bash
# Force quit:
pkill -9 handsoff
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
7. **Set short timeouts** during testing (10 seconds, not 3 minutes)

---

## ✅ Ready to Start?

Run these commands in order:

```bash
# 1. Enable your safety net
sudo systemsetup -setremotelogin on

# 2. Run tests (always safe)
cargo test

# 3. First safe run
HANDSOFF_DEV_MODE=1 HANDSOFF_DRY_RUN=1 cargo run

# 4. If that worked, try:
HANDSOFF_DEV_MODE=1 BLOCK_MOUSE=1 cargo run
```

---

**Remember**: Better safe than sorry! Always have an escape route. 🚪

**Questions?** See `docs/SAFE-DEVELOPMENT.md` for detailed scenarios.

---

*This tool is powerful. With great power comes great responsibility (and an SSH session).*
