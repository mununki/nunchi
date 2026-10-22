# Third-party code and data

## Code

Araso for Windows is a Rust port of upstream Araso's ArasoCore
(https://github.com/nyttastudio/araso, MIT, Copyright (c) 2026 aimdevlee); its notice is kept in
`LICENSE`. Upstream's kana-kanji converters (macOS's engine, AzooKey) are not part of the port.

## Data

The files in `data/` keep the licenses of what they were made from (`data/README.md` says
which file is which; full notices in `data/licenses/`):

- **Tatoeba** sentences (kana and written-Japanese trigram models, Korean syllable and word
  counts): CC BY 2.0 FR, https://tatoeba.org. Notice: `data/licenses/tatoeba.txt`.
- **SCOWL** English words and frequency levels: a free license. Notice:
  `data/licenses/scowl-Copyright`.
- **hermitdave/FrequencyWords** Korean frequency list (`korean_vocabulary.txt`): CC BY-SA 4.0,
  https://github.com/hermitdave/FrequencyWords. That file is distributed under the same license.
  Notice: `data/licenses/frequencywords.txt`.
- **Webster's Second International** word list (`spelling_words.txt`, from FreeBSD's
  `share/dict/web2`): public domain.

A build that is handed out (an installer, a DLL) has to carry these notices, and those of the Rust
crates it links (windows, serde, serde_json, chrono: MIT or Apache-2.0).
