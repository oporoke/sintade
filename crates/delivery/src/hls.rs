//! A rung's playlist with its init segment and media segments swapped for signed URLs. The
//! master playlist needs no rewriting: it names each rung relatively, so the player resolves
//! it against the (API) URL it was fetched from.

/// The rungs the ladder can have (docs/design.md §10 Process).
pub const RUNGS: [&str; 3] = ["360p", "720p", "1080p"];

/// Whether `file` is a plain file name: what a playlist may reference. Anything with a path in
/// it is refused, so a stored playlist can't point a signature at another recording's object.
pub(crate) fn is_plain_file(file: &str) -> bool {
    !file.is_empty()
        && !file.starts_with('.')
        && file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// The file names a rung playlist references, in order: the `#EXT-X-MAP` init segment and every
/// media segment line. `None` if any is not a plain file name.
pub fn referenced_files(playlist: &str) -> Option<Vec<String>> {
    let mut files = Vec::new();
    for line in playlist.lines() {
        let line = line.trim();
        let file = if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            map_uri(rest)
        } else if line.is_empty() || line.starts_with('#') {
            continue;
        } else {
            line
        };
        if !is_plain_file(file) {
            return None;
        }
        files.push(file.to_string());
    }
    Some(files)
}

fn map_uri(attributes: &str) -> &str {
    attributes
        .split(',')
        .find_map(|attribute| attribute.trim().strip_prefix("URI="))
        .map_or("", |uri| uri.trim_matches('"'))
}

/// `playlist` with each referenced file replaced by `urls[file]`. `urls` must have an entry for
/// every file `referenced_files` returned.
pub fn rewrite(playlist: &str, urls: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::with_capacity(playlist.len() * 2);
    for line in playlist.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("#EXT-X-MAP:") {
            let file = map_uri(rest);
            match urls.get(file) {
                Some(url) => out.push_str(&rest_with_uri("#EXT-X-MAP:", rest, file, url)),
                None => out.push_str(line),
            }
        } else if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push_str(line);
        } else {
            out.push_str(urls.get(trimmed).map_or(line, String::as_str));
        }
        out.push('\n');
    }
    out
}

fn rest_with_uri(prefix: &str, rest: &str, file: &str, url: &str) -> String {
    format!(
        "{prefix}{}",
        rest.replacen(&format!("\"{file}\""), &format!("\"{url}\""), 1)
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const RUNG: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\nseg_0000.m4s\n#EXTINF:1.5,\nseg_0001.m4s\n#EXT-X-ENDLIST\n";

    #[test]
    fn it_lists_the_init_and_every_segment() {
        assert_eq!(
            referenced_files(RUNG).expect("plain"),
            ["init.mp4", "seg_0000.m4s", "seg_0001.m4s"]
        );
    }

    #[test]
    fn a_path_in_a_playlist_is_refused() {
        for bad in [
            "#EXTM3U\n../../other/seg.m4s\n",
            "#EXTM3U\n/etc/passwd\n",
            "#EXTM3U\nhttps://evil.example/seg.m4s\n",
            "#EXTM3U\n#EXT-X-MAP:URI=\"../init.mp4\"\n",
        ] {
            assert_eq!(referenced_files(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_reference_becomes_its_signed_url() {
        let urls: HashMap<String, String> = referenced_files(RUNG)
            .expect("plain")
            .into_iter()
            .map(|file| {
                let url = format!("https://s/{file}?sig=1");
                (file, url)
            })
            .collect();
        let out = rewrite(RUNG, &urls);
        assert!(
            out.contains("#EXT-X-MAP:URI=\"https://s/init.mp4?sig=1\"\n"),
            "{out}"
        );
        assert!(out.contains("\nhttps://s/seg_0000.m4s?sig=1\n"), "{out}");
        assert!(out.contains("\nhttps://s/seg_0001.m4s?sig=1\n"), "{out}");
        assert!(out.contains("#EXTINF:4.0,\n") && out.ends_with("#EXT-X-ENDLIST\n"));
    }
}

/// `sprite.vtt` with each cue's sheet (`sprite_0.jpg#xywh=...`) replaced by `signed(sheet)` plus
/// the same fragment. `None` if a cue names anything but a plain file.
pub fn sprite_files(vtt: &str) -> Option<Vec<String>> {
    let mut files = Vec::new();
    for line in vtt.lines() {
        if let Some((file, _)) = line.split_once("#xywh=") {
            if !is_plain_file(file) {
                return None;
            }
            files.push(file.to_string());
        }
    }
    Some(files)
}

pub fn rewrite_sprite(vtt: &str, urls: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::with_capacity(vtt.len() * 3);
    for line in vtt.lines() {
        match line.split_once("#xywh=") {
            Some((file, rect)) if urls.contains_key(file) => {
                out.push_str(&format!("{}#xywh={rect}", urls[file]));
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod sprite_tests {
    use std::collections::HashMap;

    use super::*;

    const VTT: &str = "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nsprite_0.jpg#xywh=0,0,160,90\n\n00:00:01.000 --> 00:00:02.000\nsprite_0.jpg#xywh=160,0,160,90\n";

    #[test]
    fn each_cue_gets_its_sheet_signed_and_keeps_its_rectangle() {
        assert_eq!(
            sprite_files(VTT).expect("plain"),
            ["sprite_0.jpg", "sprite_0.jpg"]
        );
        let urls = HashMap::from([("sprite_0.jpg".to_string(), "https://s/a?sig=1".to_string())]);
        let out = rewrite_sprite(VTT, &urls);
        assert!(
            out.contains("\nhttps://s/a?sig=1#xywh=160,0,160,90\n"),
            "{out}"
        );
        assert!(out.starts_with("WEBVTT\n\n00:00:00.000 --> 00:00:01.000\n"));
    }

    #[test]
    fn a_sheet_with_a_path_is_refused() {
        assert_eq!(
            sprite_files("WEBVTT\n\n0 --> 1\n../x.jpg#xywh=0,0,1,1\n"),
            None
        );
    }
}
