## Interplay sound doctor

Converts, inspects, checks and repairs the sound files of Infinity Engine games and Fallout 1 and 2: ACM, WAVC, and the
RIFF WAV and Ogg Vorbis files their mods use.

Also writes and checks Fallout's lip-sync files.

### Commands

| Command                                      | Does                                                                                                                                                                                                                         |
| -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ipsdoc IN [OUT]`, `ipsdoc convert IN [OUT]` | Converts. Without `OUT`: an ACM or WAVC becomes a `.wav`, anything else an `.acm`, beside the input (a WAVC already named `.wav` needs an `OUT`). It names an output it replaces.                                            |
| `ipsdoc info FILE...`                        | Shows the headers, how the file reads, and which header fields are wrong.                                                                                                                                                    |
| `ipsdoc check PATH...`                       | Decodes each file named and, in directories (recursively), every ACM, WAVC, RIFF and Ogg file, and checks the lip files among them; fails if any does not decode, or a lip file is broken or does not fit the ACM beside it. |
| `ipsdoc repair [--dry-run] FILE...`          | Fixes wrong ACM and WAVC header fields, and lip files' length fields, in place, without re-encoding.                                                                                                                         |
| `ipsdoc lip SOUND... [--msg FILE] [-o LIP]`  | Writes a Fallout lip-sync file beside each speech sound (or to `-o`, for a single sound), from its line in a dialog `.msg` (or a `.txt` beside it), or from the sound alone; it names each lip file it replaces.             |
| `ipsdoc lip --rescale LIP --for SOUND`       | Rescales a lip file to another sound (another rate or channel count of the same recording).                                                                                                                                  |

`convert` options: `--to acm|wavc|wav|ogg` (otherwise from the output's extension), `--rate R` (resample),
`--channels N` and `--input-rate R` (read the input as having these, whatever its header says), `--quality Q` (Vorbis,
-1 to 10, default 5), `--raw` with `--raw-rate`, `--raw-channels`, `--raw-bits` (headerless PCM), `--wrap` (see
`docs/quirks.md`), `--full-depth` (a WAV keeps a depth beyond 16 bits, which GemRB and Near Infinity cannot read).
`ipsdoc --help` and `ipsdoc <command> --help` list everything.

Exit status: 0 when everything succeeded, 1 when a file failed (did not read, decode, check or write), 2 for a request
that cannot be carried out (bad options, an output that would overwrite the input).

### Examples

- [Convert](#convert)
- [Inspect and repair](#inspect-and-repair)
- [Check a directory](#check-a-directory)
- [Lip sync](#lip-sync)

#### Convert

```sh
# ACM to WAV, beside the input: myn1003a.wav
ipsdoc myn1003a.acm

# WAV to ACM (lossless)
ipsdoc myn1003a.wav myn1003a.acm

# To Ogg Vorbis, at a higher quality than the default 5
ipsdoc --quality 8 myn1003a.acm myn1003a.ogg

# For an Infinity Engine game: WAVC, under the .wav name the game loads
ipsdoc myn1003a.ogg --to wavc override/MYN1003A.wav

# Resample to 44100 Hz
ipsdoc --rate 44100 myn1003a.acm myn1003a-44k.acm
```

#### Inspect and repair

A Fallout 2 speech file whose header claims stereo:

```console
$ ipsdoc info myn1003a.acm
myn1003a.acm: acm
  ACM: 163690 samples, 2 channels, 22050 Hz, levels 7, rows 16
  reads as: 1 channels, 22050 Hz, 16-bit, 163690 frames, 7.424 s
  note: channels: header 2, reads as 1 (content is interleaved mono, score 0.64)
  wrong: ACM channels is 2, should be 1 (ipsdoc repair)

$ ipsdoc repair --dry-run myn1003a.acm
myn1003a.acm: ACM channels 2 -> 1 (dry run)

$ ipsdoc repair myn1003a.acm
myn1003a.acm: ACM channels 2 -> 1
```

#### Check a directory

Every sound and lip file in it, recursively:

```console
$ ipsdoc check sound/sfx
1361 sounds checked, 0 lip files checked, 0 failed
```

#### Lip sync

```console
$ ipsdoc lip --msg dialog/nhmyron.msg myn1003a.acm
myn1003a.lip: replaced

$ ipsdoc lip --rescale myn1003a.lip --for myn1003a-44k.acm
myn1003a.lip: last marker 327380 -> 654760
```

### Formats

| Format                        | Read | Write |
| ----------------------------- | ---- | ----- |
| ACM                           | yes  | yes   |
| WAVC (Infinity Engine `.wav`) | yes  | yes   |
| RIFF WAV                      | yes  | yes   |
| Ogg Vorbis                    | yes  | yes   |
| FLAC, MP3, AAC, ALAC          | yes  | no    |
| Headerless PCM                | yes  | no    |
| Fallout lip sync (`.lip`)     | yes  | yes   |

Formats are recognised by content, not by name: an Infinity Engine `.wav` may hold RIFF, WAVC or Ogg Vorbis.

Details per format: `docs/formats.md`. What ipsdoc does about the files' quirks: `docs/quirks.md`.

How lip files are written and checked: `docs/lip-sync.md`.

### Installing

Download the binary from the releases page and run it from anywhere.

To build it instead: `docs/building.md`.

### Licence

GNU General Public License, version 3: `LICENSE`. Release archives include the notices of the software it contains
(`THIRD-PARTY-NOTICES.txt`).
