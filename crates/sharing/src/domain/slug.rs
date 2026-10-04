use rand::Rng;

const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 12 base62 characters = 71.4 bits of entropy (docs/design.md §16: slugs ≥ 70 bits).
pub const SLUG_LEN: usize = 12;

/// Bytes at or above this are rejected, so `byte % 62` is uniform (248 = 4 * 62).
const REJECT_FROM: u8 = 248;

/// An unguessable public identifier for a share link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slug(String);

impl Slug {
    /// A fresh slug from the OS-seeded CSPRNG, uniform over the alphabet.
    pub fn generate() -> Self {
        let mut rng = rand::rng();
        let mut slug = String::with_capacity(SLUG_LEN);
        while slug.len() < SLUG_LEN {
            let mut bytes = [0u8; 16];
            rng.fill_bytes(&mut bytes);
            for byte in bytes {
                if byte < REJECT_FROM && slug.len() < SLUG_LEN {
                    slug.push(char::from(ALPHABET[usize::from(byte % 62)]));
                }
            }
        }
        Self(slug)
    }

    /// `None` unless `raw` is exactly [`SLUG_LEN`] base62 characters: a malformed slug can't
    /// match a link, so lookups skip the database.
    pub fn parse(raw: &str) -> Option<Self> {
        (raw.len() == SLUG_LEN && raw.bytes().all(|b| b.is_ascii_alphanumeric()))
            .then(|| Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn a_slug_is_twelve_base62_characters() {
        for _ in 0..200 {
            let slug = Slug::generate();
            assert_eq!(slug.as_str().len(), SLUG_LEN);
            assert!(slug.as_str().bytes().all(|b| ALPHABET.contains(&b)));
            assert_eq!(Slug::parse(slug.as_str()), Some(slug));
        }
    }

    #[test]
    fn the_alphabet_gives_at_least_seventy_bits() {
        let bits = (ALPHABET.len() as f64).log2() * SLUG_LEN as f64;
        assert!(bits >= 70.0, "{bits}");
    }

    #[test]
    fn slugs_do_not_repeat() {
        let slugs: HashSet<String> = (0..20_000)
            .map(|_| Slug::generate().as_str().to_string())
            .collect();
        assert_eq!(slugs.len(), 20_000);
    }

    #[test]
    fn every_character_is_used_roughly_evenly() {
        // 62_000 draws over 62 symbols: each should land near 1_000. A biased `% 62` (no
        // rejection) would put the first 8 symbols ~25% above the rest.
        let mut counts = [0u32; 128];
        for _ in 0..5_167 {
            for byte in Slug::generate().as_str().bytes() {
                counts[usize::from(byte)] += 1;
            }
        }
        let used: Vec<u32> = ALPHABET.iter().map(|b| counts[usize::from(*b)]).collect();
        let (min, max) = (used.iter().min().copied(), used.iter().max().copied());
        assert!(
            min.is_some_and(|m| m > 850) && max.is_some_and(|m| m < 1150),
            "{used:?}"
        );
    }

    #[test]
    fn malformed_slugs_do_not_parse() {
        for raw in [
            "",
            "short",
            "abcdefghijk",
            "abcdefghijklm",
            "abcdefghij-k",
            "abcdefghijk\u{e9}",
        ] {
            assert!(Slug::parse(raw).is_none(), "{raw:?}");
        }
    }
}
