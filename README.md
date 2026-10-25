## Interplay sound doctor

Converts, inspects, checks and repairs the sound files of Infinity Engine games and Fallout 1 and 2: ACM, WAVC, as well
as RIFF WAV and Ogg Vorbis files.

Also writes and checks Fallout's lip-sync files.

- [Commands](#commands)
  - [info](#info)
  - [check](#check)
  - [convert](#convert)
  - [repair](#repair)
  - [lip](#lip)
- [Formats](#formats)
- [Installing](#installing)

### Commands

`ipsdoc --help` and `ipsdoc <command> --help` list every option.

#### info

`ipsdoc info FILE...`: shows a file's headers, how it reads, and what in them is wrong.

- `--msg DIALOG`: dialog `.msg` files or directories of them, to judge lip files against each line's words.
- `--lang L`: their language, as for [lip](#lip).

A Fallout 2 speech file whose header claims stereo:

```console
$ ipsdoc info myn1003a.acm
myn1003a.acm: acm
  ACM: 163690 samples, 2 channels, 22050 Hz, levels 7, rows 16
  reads as: 1 channels, 22050 Hz, 16-bit, 163690 frames, 7.424 s
  note: channels: header 2, reads as 1 (content is interleaved mono, score 0.64)
  wrong: ACM channels is 2, should be 1 (ipsdoc repair)
```

What a `.wav` holds: WAVC in a mod, Ogg Vorbis in an Enhanced Edition's own voice files:

```console
$ ipsdoc info shael_07.wav lang/en_US/sounds/bdtpag0.wav
shael_07.wav: wavc
  WAVC: 1 channels, 22050 Hz, 16 bits, sizes 94318 / 23002, ACM at 28, field 0x1A = 0x7ffd
  ACM: 47159 samples, 1 channels, 22050 Hz, levels 6, rows 8
  reads as: 1 channels, 22050 Hz, 16-bit, 47159 frames, 2.139 s
lang/en_US/sounds/bdtpag0.wav: ogg
  reads as: 1 channels, 48000 Hz, float, 32152 frames, 0.670 s
```

#### check

`ipsdoc check PATH...`: decodes sounds and checks lip files, directories recursively.

- `--msg DIALOG`: dialog `.msg` files or directories of them, to judge lip files against each line's words.
- `--lang L`: their language, as for [lip](#lip).

Every sound and lip file in a directory, recursively; every sound in a mod, or in a game's music:

```console
$ ipsdoc check sound/sfx
1361 sounds checked, 0 lip files checked, 0 failed

$ ipsdoc check bg1npc
45 sounds checked, 0 lip files checked, 0 failed

$ ipsdoc check music
375 sounds checked, 0 lip files checked, 0 failed
```

#### convert

`ipsdoc convert IN [OUT]`, or `ipsdoc IN [OUT]`: converts between ACM, WAVC, WAV and Ogg Vorbis.

- `--to acm|wavc|wav|ogg`: the output format; otherwise from the output's extension.
- `--rate R`: resample.
- `--channels N`, `--input-rate R`: read the input as having these, whatever its header says; the samples stay as they
  are.
- `--quality Q`: Vorbis quality, -1 to 10, default 5.
- `--raw` with `--raw-rate`, `--raw-channels`, `--raw-bits`: headerless PCM, instead of `--channels` and `--input-rate`.
- `--wrap`: wrap ACM samples beyond 16 bits as the engines do, instead of clamping them (see
  [the files' quirks](docs/quirks.md)).
- `--full-depth`: a WAV keeps a depth beyond 16 bits; Infinity Engine games read WAV up to 16-bit.

```sh
# ACM to WAV, beside the input: myn1003a.wav
ipsdoc myn1003a.acm

# WAV to ACM (lossless)
ipsdoc myn1003a.wav myn1003a.acm

# Resample to 44100 Hz
ipsdoc --rate 44100 myn1003a.acm myn1003a-44k.acm
```

The classic Infinity Engine games read WAV and WAVC; the Enhanced Editions also read Ogg Vorbis under a `.wav` name. A
mod that ships Ogg Vorbis decodes it for a classic game when it installs:

```console
$ ipsdoc audio/imppp04.ogg override/IMPPP04.wav
override/IMPPP04.wav: 16-bit, from float input: Infinity Engine games read WAV up to 16-bit; add --full-depth to get lossless output
```

```sh
# Or to WAVC, the classic games' compressed .wav
ipsdoc audio/imppp04.ogg --to wavc override/IMPPP04.wav

# A classic game's WAVC to a WAV to edit, and back
ipsdoc shael_07.wav shael_07-pcm.wav
ipsdoc shael_07-pcm.wav --to wavc override/SHAEL_07.wav
```

#### repair

`ipsdoc repair FILE...`: fixes wrong header fields in place, without re-encoding.

- `--rate R`, `--channels N`: store these in each ACM and WAVC header named, over what the content says, as the samples
  cannot tell an ACM's rate; a lip file named with them fails.
- `--dry-run`: show the fixes without writing them.

The speech file `info` showed claiming stereo:

```console
$ ipsdoc repair --dry-run myn1003a.acm
myn1003a.acm: ACM channels 2 -> 1 (dry run)

$ ipsdoc repair myn1003a.acm
myn1003a.acm: ACM channels 2 -> 1
```

#### lip

`ipsdoc lip SOUND...`: writes Fallout lip-sync files, from each line's text or from the sound alone.

`ipsdoc lip --rescale LIP --for SOUND`: fits a lip file to another rate or channel count of its sound.

- `--msg FILE`: the dialog file holding each sound's line; without it, a `.txt` transcript beside the sound, else the
  sound alone.
- `--lang L`: the line's language, English by default. [Other languages](docs/lip-sync.md) download their pronunciation
  data once.
- `-o LIP`: where to write, for a single sound.

```console
$ ipsdoc lip --msg dialog/nhmyron.msg myn1003a.acm
myn1003a.lip: replaced

$ ipsdoc lip --rescale myn1003a.lip --for myn1003a-44k.acm
myn1003a.lip: last marker 327380 -> 654760
```

### Formats

- [What ipsdoc reads and writes, per format](docs/formats.md)
- [What it does about the files' quirks](docs/quirks.md)
- [How lip files are written and checked](docs/lip-sync.md)

### Installing

Download the binary for your system from the [releases page](https://github.com/BGforgeNet/ipsdoc/releases) and run it
from anywhere: `ipsdoc` (Linux x86_64), `ipsdoc-arm64` (Linux ARM64), `ipsdoc.exe` (Windows 64-bit), `ipsdoc-win32.exe`
(Windows 32-bit), `ipsdoc-macos` (macOS). On Linux and macOS, make it executable first: `chmod +x ipsdoc`.

Or [build it from source](docs/building.md).
