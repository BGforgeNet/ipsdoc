# Changelog

## 0.3.0

- `lip`, `check` and `info`: translated dialog, `--lang` (cs, fr, de, hu, it, pl, pt, ru, es, sv, vi). Pronunciation
  data from eSpeak NG, downloaded once per language.
- Releases: one file per platform, plus `LICENSE`, `THIRD-PARTY-NOTICES.txt` and `SHA256SUMS`.

## 0.2.0

- Rewritten as one self-contained program: no `snd2acm.exe`, Python or Wine.
- New commands: `convert` (ACM, WAVC, WAV, Ogg Vorbis; reads FLAC, MP3, AAC, ALAC, raw PCM), `info`, `check`, `repair`,
  `lip`.
- ACM and WAVC output is lossless. ACM decoding matches the games.

## 0.1 to 0.1.12

- Python wrapper around `snd2acm.exe`.
