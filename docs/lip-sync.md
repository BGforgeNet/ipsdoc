# Lip sync

- **Positions.** A lip file's positions are bytes of the decoded sound: twice the sample index, all channels counted. So
  a header fix never moves them; re-encoding at another rate or channel count does (`ipsdoc lip --rescale`).
- **Text.** With the line's text (English), each word's sounds come from the CMU Pronouncing Dictionary, or
  letter-to-sound rules for words it lacks, and are aligned to the speech by loudness. The dialog `.msg` names each
  line's sound: `ipsdoc lip --msg dialog/vccasidy.msg cas0103.acm`. Without text, mouth shapes come from the sound.
- **Checks.** A lip file names the sound the game loads for it, and its length must fit that sound; `check` reports
  either mismatch, phoneme codes the original game misreads (42 and up), and markers out of order. `check`, `info` and
  `repair` know a lip file by content: version 2 and the `VOC` extension field every original file has.
