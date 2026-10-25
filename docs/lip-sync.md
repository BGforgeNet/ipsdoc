# Lip sync

- **Positions.** A lip file's positions are bytes of the decoded sound: twice the sample index, all channels counted. So
  a header fix never moves them; re-encoding at another rate or channel count does (`ipsdoc lip --rescale`).
- **Text.** With the line's text (English), each word's sounds come from the CMU Pronouncing Dictionary, or
  letter-to-sound rules for words it lacks, and are placed on the speech by a forced alignment against a model of how
  each sound sounds and how long it lasts, fitted to Fallout 2's own voiced lines. The dialog `.msg` names each line's
  sound: `ipsdoc lip --msg dialog/nhmyron.msg myn1003a.acm`. Without text, mouth shapes come from the sound.
- **Checks.** A lip file names the sound the game loads for it, and should end with that sound; `check` reports a name
  mismatch, phoneme codes the original game misreads (42 and up), and markers out of order. A lip file ending off its
  sound by under 50 ms passes, as no player sees it; up to 150 ms is a warning; more, or a length 2 or 4 times off (made
  for another rate or channel count), is an error. `check`, `info` and `repair` know a lip file by content: version 2
  and the `VOC` extension field every original file has.
- **Quality.** `check` also judges how well each lip file follows its sound, from the two alone: a mouth shut (at rest,
  or lips closed) through more than half the loud speech is an error. Warnings: a mouth that follows the sound clearly
  better 60 to 300 ms late or 130 to 300 ms early (on lines of 2 s or more; a player sees a mouth late sooner than one
  early), or by a wider margin 310 to 600 ms off (3 s or more), naming the offset, when it also beats what shuffled
  copies of the file manage; a mouth open through more than half the quiet away from speech; a mouth changing shape
  under twice or over 20 times a second of speech; one shape held through over 650 ms of unbroken speech. A directory of
  15 or more lip files gets a grade, the medians of its files' figures: alignment (the chance that an open-mouth frame
  of speech is louder than one at rest: 0.5 is chance; closed lips count on neither side, as an m is voiced), width (the
  rank correlation of how wide the mouth opens with the loudness: 0 is chance), the shares of the loud speech shown shut
  (and how much of it closed lips) and of the quiet shown open, and shape changes a second. It warns below 0.6 alignment
  or 0.16 width, above 35% shut or 25% open, or outside 4 to 12 changes. Fallout 2's own speech directories grade 0.67
  to 0.96, 0.23 to 0.51, 6 to 25%, 0 to 6% and 6.1 to 9.1. How wide each head frame opens is ranked by the loudness of
  the speech it shows in Fallout 2's own files. `info` shows each file's figures, and judges no lip file made for
  another length.
- **Against the words.** Given the lines' text (`check --msg dialog/` or `info --msg`, a dialog file or a directory of
  them, English), `check` also counts the closed-lips shapes against each line's m, b and p, a run of them (the m b of
  "remember") counted once: Interplay's files close the lips once for each. A lip file closing them under 0.67 or over
  1.5 times as often, with under a quarter of the m, b and p met by a closure near where they fall, gets a warning. That
  warns on 3% of Fallout 2's own files, none of `ipsdoc lip --msg`'s, 26% of `ipsdoc lip`'s from the sound alone (which
  cannot know the words), and 41% of lip files made for another line. The directory grade adds the median closures per
  m, b and p, with a warning outside 0.5 to 1.6 (Fallout 2's directories: 0.75 to 1.50). A line whose text draws a word
  out ("Riiiight...", three or more of a letter) may hold a shape up to 1.5 s and change shape once a second.
