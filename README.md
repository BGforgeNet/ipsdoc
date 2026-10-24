## Interplay sound doctor

Converts, inspects, checks and repairs the sound files of Infinity Engine games (Baldur's Gate, Icewind Dale,
Planescape: Torment) and Fallout 1 and 2: ACM, WAVC, and the RIFF WAV and Ogg Vorbis files their mods use. Writes and
checks Fallout's lip-sync files. One self-contained program for Windows, Linux and macOS.

Usage: `ipsdoc sound.wav sound.acm`

### Commands

| Command | Does |
|---|---|
| `ipsdoc IN [OUT]`, `ipsdoc convert IN [OUT]` | Converts. Without `OUT`: an ACM or WAVC becomes a `.wav`, anything else an `.acm`, beside the input (a WAVC already named `.wav` needs an `OUT`). |
| `ipsdoc info FILE...` | Shows the headers, how the file reads, and which header fields are wrong. |
| `ipsdoc check PATH...` | Decodes each file named and, in directories (recursively), every ACM, WAVC, RIFF and Ogg file, and checks the lip files among them; fails if any does not decode, or a lip file is broken or does not fit the ACM beside it. |
| `ipsdoc repair [--dry-run] FILE...` | Fixes wrong ACM and WAVC header fields, and lip files' length fields, in place, without re-encoding. |
| `ipsdoc lip SOUND... [--msg FILE] [-o LIP]` | Writes a Fallout lip-sync file beside each speech sound (or to `-o`, for a single sound), from its line in a dialog `.msg` (or a `.txt` beside it), or from the sound alone; it names each lip file it replaces, and leaves an empty `.txt` (for LIP Editor) beside each lip file that has none. |
| `ipsdoc lip --rescale LIP --for SOUND` | Rescales a lip file to another sound (another rate or channel count of the same recording). |

`convert` options: `--to acm|wavc|wav|ogg` (otherwise from the output's extension), `--rate R` (resample),
`--channels N` and `--input-rate R` (read the input as having these, whatever its header says), `--quality Q`
(Vorbis, -1 to 10, default 5), `--raw` with `--raw-rate`, `--raw-channels`, `--raw-bits` (headerless PCM),
`--wrap` (see below). `ipsdoc --help` and `ipsdoc <command> --help` list everything.

Exit status: 0 when everything succeeded, 1 when a file failed (did not read, decode, check or write), 2 for a
request that cannot be carried out (bad options, an output that would overwrite the input).

### Formats

| Format | Read | Write |
|---|---|---|
| ACM | all of it, as the games decode it | lossless |
| WAVC (Infinity Engine `.wav`) | yes | lossless |
| RIFF WAV | PCM 8 to 32-bit, float, ADPCM | at the source's depth |
| Ogg Vorbis | yes | yes (libvorbis with aoTuV) |
| FLAC, MP3, AAC, ALAC | yes | no |
| Headerless PCM | 8 or 16-bit | no |
| Fallout lip sync (`.lip`) | version 2 | version 2 |

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

### Lip sync

- **Positions.** A lip file's positions are bytes of the decoded sound: twice the sample index, all channels counted.
  So a header fix never moves them; re-encoding at another rate or channel count does (`ipsdoc lip --rescale`).
- **Text.** With the line's text (English), each word's sounds come from the CMU Pronouncing Dictionary, or
  letter-to-sound rules for words it lacks, and are aligned to the speech by loudness. The dialog `.msg` names each
  line's sound: `ipsdoc lip --msg dialog/vccasidy.msg cas0103.acm`. Without text, mouth shapes come from the sound.
- **Checks.** A lip file names the sound the game loads for it, and its length must fit that sound; `check` reports
  either mismatch, phoneme codes the original game misreads (42 and up), and markers out of order. `check`, `info`
  and `repair` know a lip file by content: version 2 and the `VOC` extension field every original file has.

### Installing

Each release has an archive for Linux (x86-64, ARM64), Windows (x86-64, 32-bit x86) and macOS (Apple silicon and
Intel in one program), holding the program and its notices; unpack it anywhere. The Linux and Windows programs are
static: they need no libraries beyond what the system ships.

To build it instead, install Rust (the version is pinned in `rust-toolchain.toml`) and a C compiler (for libvorbis),
then `cargo build --release`. The release builds are cross-built with [Zig](https://ziglang.org/) as the C compiler
and linker: install zig and `cargo-zigbuild` and put them on `PATH`, then
`cargo zigbuild --release --target x86_64-unknown-linux-musl` (the targets are in `.github/workflows/build.yml`).
Development notes, the test oracles and the corpus tests: `docs/development.md`.

### Licence

GNU General Public License, version 3: `LICENSE`. Release archives include the notices of the software it
contains (`THIRD-PARTY-NOTICES.txt`).
