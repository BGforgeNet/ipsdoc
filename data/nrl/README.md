The English letter-to-sound rules of NRL Report 7948 (Elovitz, Johnson, McHugh and Shore, 1976, "Automatic Translation
of English Text to Phonetics by Means of Letter-to-Sound Rules", Table 6), as transcribed by
https://github.com/greg-kennedy/p5-NRL-TextToPhoneme (`rules/eng_to_ipa.json`, commit
`f99c64ac84cf483b5ad51422e05277a3a383d869`, released into the public domain: `LICENSE`).

`rules.txt` is the letter groups A to Z of that file, one rule per line, in order; the punctuation and digit groups are
left out (`src/g2p` normalises those itself). Spaces are significant: they mark word boundaries. The A and O groups (81
rules) were checked against the report's scanned Table 6, with no difference.

`src/g2p/nrl.rs` reads the rules.
