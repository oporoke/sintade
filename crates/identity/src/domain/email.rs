use std::fmt;

use email_address::EmailAddress;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EmailError {
    #[error("invalid email address")]
    Invalid,
}

/// A validated, lowercased email address.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Email(String);

impl Email {
    pub fn parse(input: &str) -> Result<Self, EmailError> {
        let trimmed = input.trim();
        if !EmailAddress::is_valid(trimmed) {
            return Err(EmailError::Invalid);
        }
        Ok(Self(trimmed.to_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Email({})", self.0)
    }
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_well_formed_address() {
        let email = Email::parse("Someone@Example.com").expect("valid email");
        assert_eq!(email.as_str(), "someone@example.com");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let email = Email::parse("  person@example.com  ").expect("valid email");
        assert_eq!(email.as_str(), "person@example.com");
    }

    #[test]
    fn rejects_missing_at_sign() {
        assert_eq!(Email::parse("not-an-email"), Err(EmailError::Invalid));
    }

    #[test]
    fn rejects_missing_domain() {
        assert_eq!(Email::parse("person@"), Err(EmailError::Invalid));
    }

    #[test]
    fn rejects_empty_string() {
        assert_eq!(Email::parse(""), Err(EmailError::Invalid));
    }

    #[test]
    fn rejects_embedded_whitespace() {
        assert_eq!(
            Email::parse("has space@example.com"),
            Err(EmailError::Invalid)
        );
    }
}
