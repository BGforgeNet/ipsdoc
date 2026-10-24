# Formats in detail

| Format         | Read                                                                                                 | Write                                                                                                           |
| -------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| ACM            | all of it, as the games decode it                                                                    | lossless, 16-bit                                                                                                |
| WAVC           | yes                                                                                                  | lossless, 16-bit                                                                                                |
| RIFF WAV       | PCM 8 to 32-bit, float, ADPCM                                                                        | 8 or 16-bit; deeper input dithered to 16 bits, or kept with `--full-depth` (24 or 32-bit integer, 32-bit float) |
| Headerless PCM | 8-bit unsigned or 16-bit signed little-endian; rate and channels from `--raw-rate`, `--raw-channels` | no                                                                                                              |
