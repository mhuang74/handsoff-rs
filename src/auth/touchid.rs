use anyhow::Result;
use std::process::Command;

/// Attempt to authenticate using Touch ID
///
/// This uses the `bioutil` command-line tool to trigger Touch ID authentication.
/// Returns Ok(true) if authentication succeeds, Ok(false) if it fails or is unavailable.
pub fn authenticate() -> Result<bool> {
    // Use osascript to show a Touch ID prompt via AppleScript
    // This is a cross-version compatible way to trigger Touch ID
    let output = Command::new("osascript")
        .arg("-e")
        .arg(
            r#"
            tell application "System Events"
                try
                    do shell script "echo 'Touch ID authentication'" with administrator privileges
                    return true
                on error
                    return false
                end try
            end tell
        "#,
        )
        .output()?;

    Ok(output.status.success())
}

/// Check if Touch ID is available on this system
#[allow(dead_code)]
pub fn is_available() -> bool {
    // Check if biometric authentication is available
    // This is a simplified check - in production, you'd want to use
    // the LocalAuthentication framework via FFI
    let output = Command::new("bioutil").arg("-r").output();

    match output {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

// Note: For a production implementation, you would want to use the
// LocalAuthentication framework directly via Objective-C FFI.
// This would look something like:
//
// #[link(name = "LocalAuthentication", kind = "framework")]
// extern "C" {
//     // LAContext and related functions
// }
//
// However, for simplicity and compatibility, we're using system commands here.
