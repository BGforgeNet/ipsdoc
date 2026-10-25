# Lip sync

How `ipsdoc` writes Fallout's lip-sync files (`.lip`) and how it checks them.

## Positions

A lip file's positions are bytes of the decoded sound: twice the sample index, all channels counted. So a header fix
never moves them; re-encoding at another rate or channel count does (`ipsdoc lip --rescale`).

## Writing (`ipsdoc lip`)

- **With the line's text** (English): each word's sounds come from the CMU Pronouncing Dictionary, or letter-to-sound
  rules for words it lacks. A forced alignment places them on the speech, against a model of how each sound sounds and
  how long it lasts, fitted to Fallout 2's own voiced lines.
- **The text's source:** the dialog `.msg` names each line's sound: `ipsdoc lip --msg dialog/nhmyron.msg myn1003a.acm`.
- **Without text**, the same model picks the likeliest run of sounds from the sound alone. It does so too, and says so,
  when the text has more sounds than its speech has room for.

The model judges each 10 ms by its loudness and its spectrum (mel-frequency cepstral coefficients).

## Checks (`check`, `info`, `repair`)

`check`, `info` and `repair` know a lip file by content: version 2 and the `VOC` extension field every original file
has.

### The file itself

| Problem                                              | Reported as |
| ---------------------------------------------------- | ----------- |
| A name field other than the file's own name          | error       |
| Phoneme codes the original game misreads (42 and up) | error       |
| Markers out of order                                 | error       |
| A marker count other than one more than the phonemes | error       |
| Flags set                                            | warning     |
| A first marker other than (1, 0)                     | warning     |
| Five markers or fewer                                | warning     |
| A length field other than the last marker            | warning     |

A lip file should end with its sound:

| Ends off its sound by                                                       | Reported as                |
| --------------------------------------------------------------------------- | -------------------------- |
| under 50 ms                                                                 | nothing: no player sees it |
| up to 150 ms                                                                | warning                    |
| more, or a length 2 or 4 times off (made for another rate or channel count) | error                      |

### How well it follows its sound

`check` judges this from the lip file and its sound alone. `info` shows each file's figures, and judges no lip file made
for another length.

A mouth shut (at rest, or lips closed) through more than half the loud speech is an error. Warnings:

| Warning       | When                                                                                                                                                                                                                                                                                                      |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Offset        | The mouth follows the sound clearly better 60 to 300 ms late or 130 to 300 ms early (lines of 2 s or more), or by a wider margin 310 to 600 ms off (3 s or more), and also beats what shuffled copies of the file manage. The warning names the offset. A player sees a mouth late sooner than one early. |
| Open in quiet | The mouth is open through more than half the quiet away from speech.                                                                                                                                                                                                                                      |
| Movement      | The mouth changes shape under twice or over 20 times a second of speech.                                                                                                                                                                                                                                  |
| Held shape    | One shape is held through over 650 ms of unbroken speech.                                                                                                                                                                                                                                                 |

### Against the words

Given the lines' text (`check --msg dialog/` or `info --msg`: a dialog file or a directory of them, English), `check`
also counts the closed-lips shapes against each line's m, b and p, a run of them (the m b of "remember") counted once:
Interplay's files close the lips once for each. A lip file closing them under 0.67 or over 1.5 times as often, with
under a quarter of the m, b and p met by a closure near where they fall, gets a warning. That warns on:

| Lip files                                                      | Warned |
| -------------------------------------------------------------- | ------ |
| Fallout 2's own                                                | 3%     |
| `ipsdoc lip --msg`'s                                           | 0.1%   |
| `ipsdoc lip`'s from the sound alone (it cannot know the words) | 18%    |
| made for another line                                          | 41%    |

A line whose text draws a word out ("Riiiight...", three or more of a letter) may hold a shape up to 1.5 s and change
shape once a second.

### A directory's grade

A directory of 15 or more lip files `check` could judge (a sound beside each, of the length the file was made for, codes
the game knows, and speech in it) gets a grade: the medians of its files' figures.

| Figure    | What it measures                                                                                                                               | Warns              | Fallout 2's directories |
| --------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ | ----------------------- |
| Alignment | The chance that an open-mouth frame of speech is louder than one at rest: 0.5 is chance. Closed lips count on neither side, as an m is voiced. | below 0.6          | 0.67 to 0.96            |
| Width     | The rank correlation of how wide the mouth opens with the loudness: 0 is chance.                                                               | below 0.16         | 0.23 to 0.51            |
| Shut      | The share of the loud speech shown shut, and how much of it closed lips.                                                                       | above 35%          | 6 to 25%                |
| Open      | The share of the quiet shown open.                                                                                                             | above 25%          | 0 to 6%                 |
| Movement  | Shape changes a second.                                                                                                                        | outside 4 to 12    | 6.1 to 9.1              |
| Closures  | Closures per m, b and p, given the text.                                                                                                       | outside 0.5 to 1.6 | 0.75 to 1.50            |

How wide each head frame opens is ranked by the loudness of the speech it shows in Fallout 2's own files.
