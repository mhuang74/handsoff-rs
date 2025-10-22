use anyhow::{Result, Context};
use keyring::Entry;

const SERVICE_NAME: &str = "com.handsoff.inputlock";
const PASSPHRASE_KEY: &str = "passphrase_hash";
#[allow(dead_code)]
const LOCK_HOTKEY_KEY: &str = "lock_hotkey";
#[allow(dead_code)]
const TALK_HOTKEY_KEY: &str = "talk_hotkey";
const AUTO_LOCK_TIMEOUT_KEY: &str = "auto_lock_timeout";

/// Store passphrase hash in Keychain
pub fn store_passphrase_hash(hash: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, PASSPHRASE_KEY)
        .context("Failed to create keychain entry")?;
    entry.set_password(hash)
        .context("Failed to store passphrase hash in keychain")?;
    Ok(())
}

/// Retrieve passphrase hash from Keychain
pub fn retrieve_passphrase_hash() -> Result<Option<String>> {
    let entry = Entry::new(SERVICE_NAME, PASSPHRASE_KEY)
        .context("Failed to create keychain entry")?;

    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("Failed to retrieve passphrase hash from keychain"),
    }
}

/// Store lock hotkey configuration
#[allow(dead_code)]
pub fn store_lock_hotkey(hotkey: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, LOCK_HOTKEY_KEY)
        .context("Failed to create keychain entry")?;
    entry.set_password(hotkey)
        .context("Failed to store lock hotkey in keychain")?;
    Ok(())
}

/// Retrieve lock hotkey configuration
#[allow(dead_code)]
pub fn retrieve_lock_hotkey() -> Result<Option<String>> {
    let entry = Entry::new(SERVICE_NAME, LOCK_HOTKEY_KEY)
        .context("Failed to create keychain entry")?;

    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("Failed to retrieve lock hotkey from keychain"),
    }
}

/// Store talk hotkey configuration
#[allow(dead_code)]
pub fn store_talk_hotkey(hotkey: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, TALK_HOTKEY_KEY)
        .context("Failed to create keychain entry")?;
    entry.set_password(hotkey)
        .context("Failed to store talk hotkey in keychain")?;
    Ok(())
}

/// Retrieve talk hotkey configuration
#[allow(dead_code)]
pub fn retrieve_talk_hotkey() -> Result<Option<String>> {
    let entry = Entry::new(SERVICE_NAME, TALK_HOTKEY_KEY)
        .context("Failed to create keychain entry")?;

    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("Failed to retrieve talk hotkey from keychain"),
    }
}

/// Store auto-lock timeout
#[allow(dead_code)]
pub fn store_auto_lock_timeout(timeout: u64) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, AUTO_LOCK_TIMEOUT_KEY)
        .context("Failed to create keychain entry")?;
    entry.set_password(&timeout.to_string())
        .context("Failed to store auto-lock timeout in keychain")?;
    Ok(())
}

/// Retrieve auto-lock timeout
pub fn retrieve_auto_lock_timeout() -> Result<Option<u64>> {
    let entry = Entry::new(SERVICE_NAME, AUTO_LOCK_TIMEOUT_KEY)
        .context("Failed to create keychain entry")?;

    match entry.get_password() {
        Ok(password) => {
            let timeout = password.parse::<u64>()
                .context("Failed to parse auto-lock timeout")?;
            Ok(Some(timeout))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("Failed to retrieve auto-lock timeout from keychain"),
    }
}
