# data

The word lists and models of the decision engine. `crates/araso-core/build.rs` turns them into
compact tables built into the binary (`src/tables.rs`), so nothing here is read at run time and
the input method needs no data folder; only `text_trigrams.txt`, for reranking kanji conversions,
would be read from a file. All but `spelling_words.txt`
are copied unchanged from upstream Araso (https://github.com/nyttastudio/araso,
`Sources/ArasoCore/Data`) at commit 22e7d3d, the commit the Rust port follows. Each keeps the
license of what it was made from; the notices are in `licenses/` (copied from upstream's
`scripts/third-party`).

| File | What | Made from | License |
|---|---|---|---|
| `english_words.txt` | English words with their SCOWL level (10 most common … 60) | SCOWL | SCOWL's notice, `licenses/scowl-Copyright` |
| `korean_vocabulary.txt` | Korean words of two or more syllables with counts | hermitdave/FrequencyWords | **CC BY-SA 4.0**, `licenses/frequencywords.txt`; the file is distributed under the same license |
| `korean_words.txt` | Korean words seen in sentences | Tatoeba | CC BY 2.0 FR, `licenses/tatoeba.txt` |
| `korean_syllables.txt` | Korean syllable counts | Tatoeba | CC BY 2.0 FR |
| `kana_trigrams.txt` | Kana trigram counts (Japanese detection) | Tatoeba | CC BY 2.0 FR |
| `text_trigrams.txt` | Trigrams of written Japanese (for reranking kanji conversions) | Tatoeba | CC BY 2.0 FR |
| `spelling_words.txt` | 236,008 English words the letter trigrams are trained on; upstream reads the same list from macOS's `/usr/share/dict/words` | FreeBSD `share/dict/web2` (Webster's Second International, 1934) | Public domain ("The 1934 copyright has lapsed", FreeBSD's `share/dict/README`) |

The files are stored byte for byte as upstream ships them, with LF line endings
(`.gitattributes` keeps Git from converting them). To follow a newer upstream, copy its
`Sources/ArasoCore/Data` here again and note the commit above.
