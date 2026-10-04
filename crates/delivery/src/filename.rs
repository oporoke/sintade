/// The file name a downloaded recording gets: the title reduced to plain ASCII letters, digits,
/// spaces, dots, dashes and underscores (so it is safe inside a `Content-Disposition` header on
/// every platform), plus `.mp4`. Anything else becomes a space; runs of spaces collapse. An
/// empty result falls back to "recording".
pub fn download_filename(title: &str) -> String {
    const MAX: usize = 80;
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut stem = words.join(" ");
    stem.truncate(MAX);
    let stem = stem.trim_matches(|c: char| c == '.' || c == ' ');
    if stem.is_empty() {
        "recording.mp4".to_string()
    } else {
        format!("{stem}.mp4")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_become_safe_file_names() {
        assert_eq!(download_filename("Sprint demo"), "Sprint demo.mp4");
        assert_eq!(
            download_filename("  Q3 / Q4 : review  "),
            "Q3 Q4 review.mp4"
        );
        assert_eq!(download_filename("a\"b\r\nc"), "a b c.mp4");
        assert_eq!(download_filename("v1.2_final-cut"), "v1.2_final-cut.mp4");
    }

    #[test]
    fn nothing_usable_falls_back() {
        for title in ["", "   ", "///", "....", "日本語"] {
            assert_eq!(download_filename(title), "recording.mp4", "{title:?}");
        }
    }

    #[test]
    fn long_titles_are_cut() {
        let name = download_filename(&"x".repeat(500));
        assert_eq!(name.len(), 80 + ".mp4".len());
    }

    #[test]
    fn never_a_path_or_a_dotfile() {
        assert_eq!(download_filename("../../etc/passwd"), "etc passwd.mp4");
        assert_eq!(download_filename(".hidden"), "hidden.mp4");
    }
}
