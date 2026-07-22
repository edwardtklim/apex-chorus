//! API credentials backed by the operating system's secure credential store.

const SERVICE: &str = "APEX Velox";

fn entry(provider: &str) -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, &format!("provider:{}", provider.to_lowercase()))
}

/// Store a provider secret in Windows Credential Manager (or the native store).
pub fn set(provider: &str, secret: &str) -> Result<(), String> {
    if secret.trim().is_empty() {
        return Err("empty credential".into());
    }
    entry(provider)
        .and_then(|e| e.set_password(secret.trim()))
        .map_err(|e| e.to_string())
}

/// Read a secret for use in-memory. Callers must never log the returned value.
pub fn get(provider: &str) -> Option<String> {
    entry(provider).ok()?.get_password().ok()
}

pub fn has(provider: &str) -> bool {
    get(provider).is_some_and(|s| !s.is_empty())
}

pub fn delete(provider: &str) -> Result<(), String> {
    entry(provider)
        .and_then(|e| e.delete_credential())
        .map_err(|e| e.to_string())
}

/// Move legacy dotenv API keys into the native vault. A line is removed only
/// after the secure write succeeds; unrelated dotenv settings are preserved.
pub fn migrate_dotenv(path: &std::path::Path) -> usize {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return 0;
    };
    let mappings = [
        ("ANTHROPIC_API_KEY", "claude"),
        ("OPENAI_API_KEY", "gpt"),
        ("GEMINI_API_KEY", "gemini"),
        ("GROK_API_KEY", "grok"),
    ];
    let mut migrated = 0;
    let mut kept = Vec::new();
    for line in contents.lines() {
        let parsed = mappings.iter().find_map(|(name, provider)| {
            line.trim_start()
                .strip_prefix(&format!("{name}="))
                .map(|secret| (*provider, secret.trim()))
        });
        match parsed {
            Some((provider, secret)) if !secret.is_empty() && set(provider, secret).is_ok() => {
                migrated += 1;
            }
            _ => kept.push(line),
        }
    }
    if migrated > 0 {
        let output = if kept.is_empty() {
            String::new()
        } else {
            kept.join("\n") + "\n"
        };
        let _ = std::fs::write(path, output);
    }
    migrated
}
