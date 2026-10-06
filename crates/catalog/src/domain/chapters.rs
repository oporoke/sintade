//! Owner-defined chapters (docs/design.md §4.6): a titled start time in a recording. Pure.

/// Most chapters a recording can have.
pub const MAX_CHAPTERS: usize = 100;
/// Longest chapter title, in characters.
pub const MAX_CHAPTER_TITLE_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChapterError {
    #[error("a recording can have at most {MAX_CHAPTERS} chapters")]
    TooMany,
    #[error("a chapter title must be 1 to {MAX_CHAPTER_TITLE_CHARS} characters")]
    BadTitle,
    #[error("a chapter title must not contain control characters")]
    ControlCharacters,
    #[error("two chapters start at the same time")]
    DuplicateStart,
    #[error("a chapter starts after the end of the recording")]
    PastTheEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub start_ms: u32,
    pub title: String,
}

/// A valid chapter list: sorted by start, unique starts, titles trimmed, none past the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterList(Vec<Chapter>);

impl ChapterList {
    /// Validates `raw`. `duration_ms` (when known) bounds the start times.
    pub fn parse(raw: Vec<Chapter>, duration_ms: Option<u32>) -> Result<Self, ChapterError> {
        if raw.len() > MAX_CHAPTERS {
            return Err(ChapterError::TooMany);
        }
        let mut chapters = Vec::with_capacity(raw.len());
        for chapter in raw {
            let title = chapter.title.trim();
            let length = title.chars().count();
            if length == 0 || length > MAX_CHAPTER_TITLE_CHARS {
                return Err(ChapterError::BadTitle);
            }
            if title.chars().any(char::is_control) {
                return Err(ChapterError::ControlCharacters);
            }
            if duration_ms.is_some_and(|end| chapter.start_ms > end) {
                return Err(ChapterError::PastTheEnd);
            }
            chapters.push(Chapter {
                start_ms: chapter.start_ms,
                title: title.to_string(),
            });
        }
        chapters.sort_by_key(|chapter| chapter.start_ms);
        if chapters
            .windows(2)
            .any(|pair| pair[0].start_ms == pair[1].start_ms)
        {
            return Err(ChapterError::DuplicateStart);
        }
        Ok(Self(chapters))
    }

    pub fn as_slice(&self) -> &[Chapter] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(start_ms: u32, title: &str) -> Chapter {
        Chapter {
            start_ms,
            title: title.to_string(),
        }
    }

    #[test]
    fn chapters_are_sorted_and_titles_trimmed() {
        let list = ChapterList::parse(
            vec![chapter(30_000, " Demo "), chapter(0, "Intro")],
            Some(60_000),
        )
        .expect("valid");
        assert_eq!(
            list.as_slice(),
            [chapter(0, "Intro"), chapter(30_000, "Demo")]
        );
    }

    #[test]
    fn an_empty_list_is_valid() {
        assert!(
            ChapterList::parse(vec![], Some(10))
                .expect("valid")
                .as_slice()
                .is_empty()
        );
    }

    #[test]
    fn rejects_duplicates_bad_titles_and_times_past_the_end() {
        let dup = vec![chapter(5, "a"), chapter(5, "b")];
        assert_eq!(
            ChapterList::parse(dup, None),
            Err(ChapterError::DuplicateStart)
        );
        for title in ["", "   "] {
            assert_eq!(
                ChapterList::parse(vec![chapter(0, title)], None),
                Err(ChapterError::BadTitle)
            );
        }
        assert_eq!(
            ChapterList::parse(vec![chapter(0, &"x".repeat(121))], None),
            Err(ChapterError::BadTitle)
        );
        assert_eq!(
            ChapterList::parse(vec![chapter(0, "a\nb")], None),
            Err(ChapterError::ControlCharacters)
        );
        assert_eq!(
            ChapterList::parse(vec![chapter(10_001, "a")], Some(10_000)),
            Err(ChapterError::PastTheEnd)
        );
        assert!(ChapterList::parse(vec![chapter(10_000, "a")], Some(10_000)).is_ok());
    }

    #[test]
    fn rejects_too_many() {
        let many = (0..=MAX_CHAPTERS as u32).map(|i| chapter(i, "c")).collect();
        assert_eq!(ChapterList::parse(many, None), Err(ChapterError::TooMany));
    }
}
