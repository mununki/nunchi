# Third-party code and data

## Code

Nunchi is a Rust port of upstream Araso's ArasoCore
(https://github.com/nyttastudio/araso, MIT, Copyright (c) 2026 aimdevlee); its notice is kept in
`LICENSE`. Upstream's kana-kanji converters (macOS's engine, AzooKey) are not part of the port.

The programs link Rust crates, all under permissive licenses:

- **The input method** (`nunchi_tsf.dll`, 64- and 32-bit): windows, windows-core, serde,
  serde_json, chrono and what they use, MIT OR Apache-2.0 (memchr: Unlicense OR MIT, zmij: MIT).
- **The settings window** (`nunchi-settings.exe`): eframe and egui (MIT OR Apache-2.0) and what
  they use to draw a window: winit (Apache-2.0), glow and wgpu, AccessKit, arboard, image and
  others. Most are MIT OR Apache-2.0. A few have other permissive licenses:
  - Zlib, BSD-2-Clause, BSD-3-Clause, ISC, 0BSD or Unlicense
  - BSL-1.0 (clipboard-win, error-code)
  - Unicode-3.0 (the ICU4X crates url uses: icu_*, zerovec and others)
  - self_cell, offered as Apache-2.0 OR GPL-2.0-only, used under Apache-2.0
- **Fonts egui puts in the settings window** (epaint_default_fonts):
  - Hack (MIT; Bitstream Vera License for the Vera glyphs)
  - Ubuntu Light (Ubuntu Font Licence 1.0)
  - Noto Emoji (SIL Open Font License 1.1)
  - emoji-icon-font (MIT)

  Hangul is drawn with Windows' Malgun Gothic, read from the system and not shipped.

Every crate with its version and license, and the license texts the crates carry, are generated
from `Cargo.lock` by `tools/crate-licenses.ps1` into `target/licenses/rust-crates.txt`;
`scripts/build-installer.ps1` runs it, and the installer puts it in `licenses\`.

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

A build that is handed out (an installer, a DLL) has to carry these notices. The installer puts
this file (`THIRD-PARTY.txt`), `LICENSE.txt` and `licenses\` (the data's notices and
`rust-crates.txt`) beside the programs.
