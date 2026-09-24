use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

const MIN_LENGTH: usize = 10;

static BREACHED_PASSWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    include_str!("../../assets/breached-passwords-top-100k.txt")
        .lines()
        .collect()
});

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PasswordError {
    #[error("password must be at least 10 characters")]
    TooShort,

    #[error("password appears in a known data breach; choose a different one")]
    Breached,
}

/// A password that has passed length and breach-list validation. Never `Debug`/`Display`s its
/// contents (see CLAUDE.md: never log passwords).
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

impl Password {
    pub fn parse(input: &str) -> Result<Self, PasswordError> {
        if input.chars().count() < MIN_LENGTH {
            return Err(PasswordError::TooShort);
        }
        if BREACHED_PASSWORDS.contains(input) {
            return Err(PasswordError::Breached);
        }
        Ok(Self(input.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Password(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_long_unbreached_password() {
        assert!(Password::parse("correct-horse-battery-staple-42").is_ok());
    }

    #[test]
    fn rejects_passwords_under_ten_characters() {
        assert_eq!(Password::parse("short1"), Err(PasswordError::TooShort));
    }

    #[test]
    fn accepts_exactly_ten_characters() {
        assert!(Password::parse("xk7qz2mvbn").is_ok());
    }

    #[test]
    fn rejects_nine_characters() {
        assert_eq!(Password::parse("abcdefghi"), Err(PasswordError::TooShort));
    }

    #[test]
    fn rejects_a_top_of_list_breached_password() {
        // "123456" fails length first; use a >=10-char entry actually in the list.
        assert_eq!(Password::parse("123456789"), Err(PasswordError::TooShort));
        assert_eq!(Password::parse("1234567890"), Err(PasswordError::Breached));
    }

    #[test]
    fn debug_never_reveals_the_password() {
        let password = Password::parse("correct-horse-battery-staple-42").expect("valid");
        assert_eq!(format!("{password:?}"), "Password(REDACTED)");
    }
}
