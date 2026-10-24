# Formats

| Format                        | Read                                                                                                 | Write                                                                                                           |
| ----------------------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| ACM                           | all of it, as the games decode it                                                                    | lossless, 16-bit                                                                                                |
| WAVC (Infinity Engine `.wav`) | yes                                                                                                  | lossless, 16-bit                                                                                                |
| RIFF WAV                      | PCM 8 to 32-bit, float, ADPCM                                                                        | 8 or 16-bit; deeper input dithered to 16 bits, or kept with `--full-depth` (24 or 32-bit integer, 32-bit float) |
| Ogg Vorbis                    | yes                                                                                                  | at `--quality` -1 to 10 (default 5)                                                                             |
| FLAC, MP3, AAC, ALAC          | yes                                                                                                  | no                                                                                                              |
| Headerless PCM                | 8-bit unsigned or 16-bit signed little-endian; rate and channels from `--raw-rate`, `--raw-channels` | no                                                                                                              |
| Fallout lip sync (`.lip`)     | version 2                                                                                            | version 2                                                                                                       |

Formats are recognised by content, not by name: an Infinity Engine `.wav` may hold RIFF, WAVC or Ogg Vorbis.
