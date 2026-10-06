mod chapters;
pub use chapters::{Chapter, ChapterError, ChapterList, MAX_CHAPTER_TITLE_CHARS, MAX_CHAPTERS};

/// Longest title accepted, in characters (not bytes).
pub const MAX_TITLE_CHARS: usize = 200;

const DEFAULT_TITLE: &str = "Untitled recording";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TitleError {
    #[error("title must be at most {MAX_TITLE_CHARS} characters")]
    TooLong,
    #[error("title must not contain control characters")]
    ControlCharacters,
}

/// A recording's title: trimmed, at most [`MAX_TITLE_CHARS`], no control characters. Missing
/// or blank means "Untitled recording"; the user renames it later (M6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title(String);

impl Title {
    pub fn parse(raw: Option<&str>) -> Result<Self, TitleError> {
        let trimmed = raw.map(str::trim).unwrap_or_default();
        if trimmed.is_empty() {
            return Ok(Self(DEFAULT_TITLE.to_string()));
        }
        if trimmed.chars().count() > MAX_TITLE_CHARS {
            return Err(TitleError::TooLong);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(TitleError::ControlCharacters);
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
    fn missing_or_blank_titles_get_the_default() {
        for raw in [None, Some(""), Some("   ")] {
            assert_eq!(
                Title::parse(raw).expect("valid").as_str(),
                "Untitled recording"
            );
        }
    }

    #[test]
    fn titles_are_trimmed() {
        assert_eq!(
            Title::parse(Some("  Sprint demo "))
                .expect("valid")
                .as_str(),
            "Sprint demo"
        );
    }

    #[test]
    fn length_is_counted_in_characters_not_bytes() {
        let at_limit = "é".repeat(MAX_TITLE_CHARS);
        assert!(Title::parse(Some(&at_limit)).is_ok());
        let over = "a".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(Title::parse(Some(&over)), Err(TitleError::TooLong));
    }

    #[test]
    fn control_characters_are_rejected() {
        assert_eq!(
            Title::parse(Some("line\nbreak")),
            Err(TitleError::ControlCharacters)
        );
    }
}

/// Recordings per library page (docs/design.md §9: cursor pagination).
pub const LIBRARY_PAGE_SIZE: i64 = 24;

/// Where the next library page starts: after the recording created at `created_at` with `id`
/// (the list is newest first, ties broken by id). Opaque to clients: `"<unix micros>.<uuid>"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub created_at_micros: i64,
    pub id: uuid::Uuid,
}

impl Cursor {
    pub fn encode(self) -> String {
        format!("{}.{}", self.created_at_micros, self.id.simple())
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let (micros, id) = raw.split_once('.')?;
        Some(Self {
            created_at_micros: micros.parse().ok()?,
            id: uuid::Uuid::parse_str(id).ok()?,
        })
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;

    #[test]
    fn a_cursor_round_trips() {
        let cursor = Cursor {
            created_at_micros: 1_790_000_000_123_456,
            id: uuid::Uuid::now_v7(),
        };
        assert_eq!(Cursor::parse(&cursor.encode()), Some(cursor));
    }

    #[test]
    fn garbage_is_not_a_cursor() {
        for raw in [
            "",
            "x",
            "12",
            "12.",
            ".abc",
            "abc.def",
            "12.not-a-uuid",
            "1.2.3",
        ] {
            assert_eq!(Cursor::parse(raw), None, "{raw:?}");
        }
    }
}
