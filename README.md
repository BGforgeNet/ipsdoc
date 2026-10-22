## Interplay sound doctor

Converts, inspects, checks and repairs the sound files of Infinity Engine games (Baldur's Gate, Icewind Dale,
Planescape: Torment) and Fallout 1 and 2: ACM, WAVC, and the RIFF WAV and Ogg Vorbis files their mods use. One
self-contained program for Windows, Linux and macOS.

Usage: `ipsdoc sound.wav sound.acm`

### Commands

| Command | Does |
|---|---|
| `ipsdoc IN [OUT]`, `ipsdoc convert IN [OUT]` | Converts. Without `OUT`: an ACM or WAVC becomes a `.wav`, anything else an `.acm`, beside the input (a WAVC already named `.wav` needs an `OUT`). |
| `ipsdoc info FILE...` | Shows the headers, how the file reads, and which header fields are wrong. |
| `ipsdoc check PATH...` | Decodes every sound (directories recursively); fails if any does not decode. |
| `ipsdoc repair [--dry-run] FILE...` | Fixes wrong ACM and WAVC header fields in place, without re-encoding. |

`convert` options: `--to acm|wavc|wav|ogg` (otherwise from the output's extension), `--rate R` (resample),
`--channels N` and `--input-rate R` (read the input as having these, whatever its header says), `--quality Q`
(Vorbis, -1 to 10, default 5), `--raw` with `--raw-rate`, `--raw-channels`, `--raw-bits` (headerless PCM),
`--wrap` (see below). `ipsdoc --help` and `ipsdoc <command> --help` list everything.

### Formats

| Format | Read | Write |
|---|---|---|
| ACM | all of it, as the games decode it | lossless |
| WAVC (Infinity Engine `.wav`) | yes | lossless |
| RIFF WAV | PCM 8 to 32-bit, float, ADPCM | at the source's depth |
| Ogg Vorbis | yes | yes (libvorbis with aoTuV) |
| FLAC, MP3, AAC, ALAC | yes | no |
| Headerless PCM | 8 or 16-bit | no |

Formats are recognised by content, not by name: an Infinity Engine `.wav` may hold RIFF, WAVC or Ogg Vorbis.

### What it does about the files' quirks

- **Lossless ACM.** ACM and WAVC output stores the samples exactly, in the form the engines' decoders read as any
  other ACM (checked against the Fallout 2 engine's decoder, as decompiled by fallout2-ce, and GemRB's). It is
  bigger than the lossy files Interplay's `snd2acm` makes: 0.8 to 0.9 of a WAV's size. Deeper input (24-bit,
  float) is dithered to 16 bits, which is all ACM holds.
- **True headers.** Written files state their real rate and channel count.
- **Channels.** Fallout ignores the ACM channel field, so most of its speech and effects say stereo and are mono. A
  stereo header over an odd number of samples, or over content that reads as one signal interleaved, is read as
  mono. A WAVC whose two headers agree is trusted, as Infinity Engine players trust it, unless its sample count is
  odd. Known misreadings: Fallout's ambient `wind1`, `wind2` (Fallout 2) and `alarm1`, `alarm2` (Fallout 1) read as
  mono but play as stereo (use `--channels 2`); Fallout 2's `engineon` reads as stereo but plays as mono (use
  `--channels 1`).
- **Rates.** A rate stored byte-swapped (17580 for 44100, as in some music packs) is read as meant; `repair` fixes it.
- **Overflow.** Lossy ACMs sometimes hold a sample beyond 16 bits. The games wrap it to the other end of the range,
  an audible click; ipsdoc clamps it, and `info` counts them. `--wrap` reproduces the games exactly.
- **Other players.** FFmpeg-based players cannot play most ACMs, lossless ones never; libacm-based tools cannot open
  lossless ones.

### Building

Install Rust (the version is pinned in `rust-toolchain.toml`) and a C compiler (for libvorbis), then
`cargo build --release`. Development notes, the test oracles and the corpus tests: `docs/development.md`.

### Licence

GNU General Public License, version 3: `LICENSE`. Release archives include the notices of the software it
contains (`THIRD-PARTY-NOTICES.txt`).
