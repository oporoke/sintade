# Golden media fixtures

The files `ProcessTake` is tested against (docs/design.md §18). Media's tests read them from
here; expected properties live next to the tests (`crates/media/src/infra/probe_fixtures.rs`,
and the pipeline goldens from Day 49).

| File | What it stands for | Source |
| --- | --- | --- |
| `vp9_opus_30s.webm` | Baseline Chrome output: VP9/Opus, live WebM (no duration, no cues), 640x360, 30 s | `generate.sh` |
| `vp8_opus_paused.webm` | Pause/resume: VP8/Opus with a 3 s timestamp gap after 5 s | `generate.sh` |
| `h264_aac_safari_20s.mp4` | Safari: fragmented MP4, H.264 baseline/AAC, 20 s — the remux-only path | `generate.sh` |
| `vp9_no_audio.webm` | A silent recording: VP9 only, 10 s | `generate.sh` |
| `truncated_last_chunk.webm` | Crash recovery: a 10 s take whose last cluster is cut off (90 % of the bytes) | `generate.sh` |
| `not_a_video.webm` | Text with a `.webm` name: the ffprobe rejection path | `generate.sh` |
| `real_chrome_vp9_opus_10s.webm` | Real MediaRecorder output: Chrome (fake devices), VP9/Opus 1280x720, ~10 s, 5 chunks concatenated | Recorded 2026-10-04 through `/record` (`recorder-upload.spec.ts`) |
| `real_firefox_vp8_opus_10s.webm` | Real MediaRecorder output: Firefox (fake mic + canvas screen), VP8/Opus 1280x720, ~10 s, 6 chunks | Recorded 2026-10-04 through `/record` |

`vp9_opus_60min.webm` (§18, long-duration budget) is generated on demand for the Day 50
performance pass, never committed.

Regenerate the synthetic ones with `docs/fixtures/generate.sh` (FFmpeg ≥ 6.0). Encoders aren't
bit-exact across FFmpeg versions, so treat a regeneration as a fixture change and re-check the
expectations.
