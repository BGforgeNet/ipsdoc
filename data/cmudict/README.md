The CMU Pronouncing Dictionary, as published at https://github.com/cmusphinx/cmudict, commit
`74790861f652b15e4ac49015a90074ad62a27690` (2025-10-24): `cmudict.dict` and `LICENSE`, unchanged. `build.rs`
compresses it into the binary; `src/g2p/dict.rs` reads it.

To update, fetch both files from a newer commit, name it here, and run the tests (the word count in
`src/g2p/dict.rs` changes with the file).
