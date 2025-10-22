# Debugging Accessibility Permission Issues

## Quick Start

The app bundle has been enhanced with detailed logging. Follow these steps to debug permission issues:

### 1. Run the Debug Script

```bash
./debug_permissions.sh
```

This script will:
- Check if the app bundle exists
- Verify the bundle identifier
- Check code signature
- Reset accessibility permissions
- Open System Settings for you
- Run the app with full debug output

### 2. Manual Debugging Steps

If you prefer to debug manually:

#### Step 1: Run App from Terminal to See Logs

```bash
/Users/mhuang/Projects/Development/handsoff-rs/target/release/bundle/osx/HandsOff.app/Contents/MacOS/handsoff
```

This will show all log output, including:
- `AXIsProcessTrusted check: true/false` - Shows if macOS recognizes the permission
- `Event tap creation check: true/false` - Shows if event tap can be created
- Detailed error messages if permission is denied

#### Step 2: Check macOS System Logs

Open Console.app and filter for "handsoff":

```bash
# Option 1: Use Console.app GUI
open -a Console
# Then enter "handsoff" in the search box

# Option 2: Stream logs in terminal
log stream --predicate 'process == "handsoff"' --level debug

# Option 3: Search recent logs
log show --predicate 'process == "handsoff"' --last 5m
```

#### Step 3: Reset Permissions

If permissions appear to be granted but the app still fails:

```bash
# Reset permissions for the app
tccutil reset Accessibility com.handsoff.inputlock

# Then re-grant in System Settings
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
```

#### Step 4: Verify Bundle Identity

Check that the bundle identifier matches:

```bash
# Check Info.plist
defaults read /Users/mhuang/Projects/Development/handsoff-rs/target/release/bundle/osx/HandsOff.app/Contents/Info CFBundleIdentifier

# Should output: com.handsoff.inputlock
```

#### Step 5: Check Code Signature

```bash
codesign -dv /Users/mhuang/Projects/Development/handsoff-rs/target/release/bundle/osx/HandsOff.app
```

## Common Issues

### Issue 1: "Accessibility permissions not granted" but I already granted them

**Cause**: You may have granted permissions to:
- The command-line binary (`target/release/handsoff`)
- An older version of the app bundle
- The app with a different bundle identifier

**Solution**:
1. Reset permissions: `tccutil reset Accessibility com.handsoff.inputlock`
2. Remove the app from Accessibility list in System Settings
3. Run the app again - it will request permissions fresh
4. Grant permissions to the new request

### Issue 2: Permission dialog doesn't appear

**Cause**: macOS only shows the permission dialog once per app

**Solution**:
```bash
# Reset the permission prompt
tccutil reset Accessibility com.handsoff.inputlock

# Delete the app from System Settings > Privacy & Security > Accessibility if present
# Then restart the app
```

### Issue 3: App quits immediately with no error

**Cause**: Permission check fails before logger initializes, or app crashes

**Solution**: Run from terminal to see all output:
```bash
/Users/mhuang/Projects/Development/handsoff-rs/target/release/bundle/osx/HandsOff.app/Contents/MacOS/handsoff
```

### Issue 4: Different behavior between command-line and app bundle

**Cause**: macOS treats these as separate applications with separate permission grants

**Solution**: Always use the app bundle. Grant permissions specifically to:
`HandsOff.app` (com.handsoff.inputlock), not the CLI binary.

## Understanding the Permission Checks

The app now performs TWO permission checks:

1. **`AXIsProcessTrusted()`** - macOS Accessibility API check
   - Most reliable indicator of permission status
   - Returns `true` if app is in Accessibility list and enabled

2. **Event Tap Creation** - Attempts to create a CGEventTap
   - Tests if the app can actually intercept events
   - May fail even if AXIsProcessTrusted succeeds (in rare cases)

Both checks must pass for the app to start.

## Debugging Output

When you run the app from terminal, you'll see output like:

```
[INFO  handsoff] Starting HandsOff Input Lock
[INFO  handsoff::input_blocking] AXIsProcessTrusted check: false
[INFO  handsoff::input_blocking] Event tap creation check: false
[ERROR handsoff::input_blocking] Accessibility permission check failed:
[ERROR handsoff::input_blocking]   - AXIsProcessTrusted: false
[ERROR handsoff::input_blocking]   - Event tap created: false
[ERROR handsoff::input_blocking]   - Bundle ID should be: com.handsoff.inputlock
[ERROR handsoff::input_blocking]   - Please check System Settings > Privacy & Security > Accessibility
[ERROR handsoff] Accessibility permissions not granted
```

This tells you exactly which check failed and what to do next.

## Still Not Working?

If you've tried all the above and it still doesn't work:

1. Check macOS version compatibility (requires 10.11+)
2. Check if other accessibility apps work (to rule out system issues)
3. Try creating a new user account and testing there
4. Check Console.app for TCC (Transparency, Consent, and Control) errors
5. Reboot (macOS sometimes needs this for TCC changes to take effect)

## Additional Resources

- [macOS TCC Documentation](https://developer.apple.com/documentation/bundleresources/information_property_list/protected_resources)
- [Accessibility Programming Guide](https://developer.apple.com/library/archive/documentation/Accessibility/Conceptual/AccessibilityMacOSX/)
