# Lip sync

- **Positions.** A lip file's positions are bytes of the decoded sound: twice the sample index, all channels counted. So
  a header fix never moves them; re-encoding at another rate or channel count does (`ipsdoc lip --rescale`).
- **Text.** With the line's text (English), each word's sounds come from the CMU Pronouncing Dictionary, or
  letter-to-sound rules for words it lacks, and are aligned to the speech by loudness. The dialog `.msg` names each
  line's sound: `ipsdoc lip --msg dialog/nhmyron.msg myn1003a.acm`. Without text, mouth shapes come from the sound.
- **Checks.** A lip file names the sound the game loads for it, and should end with that sound; `check` reports a name
  mismatch, phoneme codes the original game misreads (42 and up), and markers out of order. A lip file ending off its
  sound by under 50 ms passes, as no player sees it; up to 150 ms is a warning; more, or a length 2 or 4 times off (made
  for another rate or channel count), is an error. `check`, `info` and `repair` know a lip file by content: version 2
  and the `VOC` extension field every original file has.
- **Quality.** `check` also judges how well each lip file follows its sound, from the two alone: a mouth shut through
  more than half the loud speech is an error; one that follows the sound clearly better 150 to 300 ms off is a warning
  naming the offset (on lines of 2 s or more). A directory of 15 or more lip files gets a grade, the medians of its
  files' alignment (the chance that an open-mouth frame of speech is louder than a shut one: 0.5 is chance) and of their
  shut-while-loud share, with a warning below 0.6 or above 35%. Fallout 2's own speech directories grade 0.64 to 0.82
  and 6 to 25%. `info` shows each file's figures.
- **Against the words.** Given the lines' text (`check --msg dialog/`, a dialog file or a directory of them, English),
  `check` also counts the closed-lips shapes against each line's m, b and p, a run of them (the m b of "remember")
  counted once: Interplay's files close the lips once for each. A lip file closing them under 0.67 or over 1.5 times as
  often, with under a quarter of the m, b and p met by a closure near where they fall, gets a warning. That warns on 3%
  of Fallout 2's own files, 2% of `ipsdoc lip --msg`'s, 28% of `ipsdoc lip`'s from the sound alone (which cannot know
  the words), and 46% of lip files made for another line. The directory grade adds the median closures per m, b and p,
  with a warning outside 0.5 to 1.6 (Fallout 2's directories: 0.75 to 1.50).
