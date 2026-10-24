# What ipsdoc does about the files' quirks

- **Lossless ACM.** ACM and WAVC output stores the samples exactly, in the form the engines' decoders read as any other
  ACM (checked against the Fallout 2 engine's decoder, as decompiled by fallout2-ce, and GemRB's). It is bigger than the
  lossy files Interplay's `snd2acm` makes: 0.8 to 0.9 of a WAV's size. Deeper input (24-bit, float) is dithered to 16
  bits, which is all ACM holds.
- **True headers.** Written files state their real rate and channel count.
- **Channels.** Fallout ignores the ACM channel field, so most of its speech and effects say stereo and are mono. A
  stereo header over an odd number of samples, or over content that reads as one signal interleaved, is read as mono. A
  WAVC whose two headers agree is trusted, as Infinity Engine players trust it, unless its sample count is odd. Known
  misreadings: Fallout's ambient `wind1`, `wind2` (Fallout 2) and `alarm1`, `alarm2` (Fallout 1) read as mono but play
  as stereo (use `--channels 2`); Fallout 2's `engineon` reads as stereo but plays as mono (use `--channels 1`).
- **Overflow.** Lossy ACMs sometimes hold a sample beyond 16 bits. The games wrap it to the other end of the range, an
  audible click; ipsdoc clamps it, and `info` counts them. `--wrap` reproduces the games exactly.
- **Other players.** FFmpeg-based players cannot play most ACMs, lossless ones never; libacm-based tools cannot open
  lossless ones.
