#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DisplayNameError {
    #[error("display name must not be empty")]
    Empty,

    #[error("display name must be at most {max} characters")]
    TooLong { max: usize },
}

/// A trimmed, non-empty display name. The 80-character cap is ours, not the design's
/// (TODO: Verify): long enough for real names, short enough to fit UI chrome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayName(String);

impl DisplayName {
    pub const MAX_CHARS: usize = 80;

    pub fn parse(input: &str) -> Result<Self, DisplayNameError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(DisplayNameError::Empty);
        }
        if trimmed.chars().count() > Self::MAX_CHARS {
            return Err(DisplayNameError::TooLong {
                max: Self::MAX_CHARS,
            });
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(
            DisplayName::parse("  Asha Mwangi ").map(|name| name.as_str().to_string()),
            Ok("Asha Mwangi".to_string())
        );
    }

    #[test]
    fn rejects_blank() {
        assert_eq!(DisplayName::parse("   "), Err(DisplayNameError::Empty));
    }

    #[test]
    fn counts_characters_not_bytes() {
        assert!(DisplayName::parse(&"é".repeat(DisplayName::MAX_CHARS)).is_ok());
        assert_eq!(
            DisplayName::parse(&"a".repeat(DisplayName::MAX_CHARS + 1)),
            Err(DisplayNameError::TooLong { max: 80 })
        );
    }
}
