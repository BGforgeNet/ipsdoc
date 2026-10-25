# Formats

| Format                        | Read                                                                                                                                       | Write                                                                                                                          |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| ACM                           | as the games decode it, a sample past 16 bits clamped (`--wrap` wraps it as they do); a stream they would decode from undefined data fails | 16-bit: lossless from 16-bit input or less, deeper input dithered; from ACM or WAVC, samples unchanged: its stream, new header |
| WAVC (Infinity Engine `.wav`) | yes                                                                                                                                        | as ACM                                                                                                                         |
| RIFF WAV                      | PCM 8 to 32-bit, float, ADPCM                                                                                                              | 8 or 16-bit; deeper input dithered to 16 bits, or kept with `--full-depth` (24 or 32-bit integer, 32-bit float)                |
| Ogg Vorbis                    | yes                                                                                                                                        | at `--quality` -1 to 10 (default 5)                                                                                            |
| FLAC, MP3, AAC, ALAC          | yes                                                                                                                                        | no                                                                                                                             |
| Headerless PCM                | 8-bit unsigned or 16-bit signed little-endian; rate and channels from `--raw-rate`, `--raw-channels`                                       | no                                                                                                                             |
| Fallout lip sync (`.lip`)     | version 2                                                                                                                                  | version 2                                                                                                                      |

Formats are recognised by content, not by name: an Infinity Engine `.wav` may hold RIFF, WAVC or Ogg Vorbis.

A sound is read whole, so a RIFF, Ogg Vorbis, FLAC, MP3, AAC or ALAC sound of more than 2^30 samples (all channels
together: some 3.4 hours of 44.1 kHz stereo) is not read.
