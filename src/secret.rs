//! Secrets that never leak: masked `Debug`/`Display`, and a central redaction filter
//! (constitution, principe VI).

use std::fmt;

/// Mask printed in place of any secret.
pub const MASK: &str = "••••";

/// A secret value (API key). `Debug` and `Display` never show it.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Secret(s.into())
    }

    /// The raw value. Only the HTTP client and the redactor may call this.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() { "\"\"" } else { MASK })
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() { "" } else { MASK })
    }
}

impl<'de> serde::Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Secret)
    }
}

/// Replaces every known secret by [`MASK`] in any text leaving cmd_ai.
#[derive(Clone, Default, Debug)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    /// Secrets shorter than 4 characters are ignored (too many false positives).
    pub fn new(secrets: &[&str]) -> Self {
        let mut secrets: Vec<String> = secrets
            .iter()
            .filter(|s| s.chars().count() >= 4)
            .map(|s| (*s).to_string())
            .collect();
        // Longest first so that a secret containing another one is fully masked.
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets.dedup();
        Redactor { secrets }
    }

    pub fn redact(&self, s: &str) -> String {
        let mut out = s.to_string();
        for secret in &self.secrets {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), MASK);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_are_masked() {
        let s = Secret::new("sk-very-secret-key");
        assert_eq!(format!("{s:?}"), MASK);
        assert_eq!(format!("{s}"), MASK);
        assert_eq!(s.expose(), "sk-very-secret-key");
    }

    #[test]
    fn empty_secret_is_visibly_empty() {
        let s = Secret::default();
        assert!(s.is_empty());
        assert_eq!(format!("{s}"), "");
    }

    #[test]
    fn redactor_masks_every_occurrence() {
        let r = Redactor::new(&["abcd1234"]);
        assert_eq!(r.redact("key=abcd1234; again abcd1234"), format!("key={MASK}; again {MASK}"));
    }

    #[test]
    fn redactor_ignores_short_secrets_and_handles_nesting() {
        let r = Redactor::new(&["ab", "secret", "my-secret-long"]);
        assert_eq!(r.redact("ab my-secret-long secret"), format!("ab {MASK} {MASK}"));
    }

    #[test]
    fn deserializes_from_json_string() {
        let s: Secret = serde_json::from_str("\"k3y-value\"").unwrap();
        assert_eq!(s.expose(), "k3y-value");
    }
}
