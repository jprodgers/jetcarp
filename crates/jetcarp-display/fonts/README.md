# The fonts JETCARP ships

JETCARP draws its words in **Atkinson Hyperlegible Next** and, for code and other monospaced
text, **Atkinson Hyperlegible Mono**: fonts whose letters are easier to tell apart, designed by
the Braille Institute for low-vision readers. `src/fonts.rs` embeds the files below; the app and
the caption on the desktop background and the screensaver draw with them.

Each file is the copyright holders' own, whole and unmodified (not subset, not converted), taken
from the families' official repositories on GitHub, kept by Google Fonts for the Braille
Institute. Both families are licensed under the SIL Open Font License, Version 1.1; each folder
holds its family's `OFL.txt` exactly as the repository gives it. Neither licence file names a
Reserved Font Name. A test (`fonts::tests`) checks that each embedded file's SHA-256 is the one
here.

| File | SHA-256 | Source |
| --- | --- | --- |
| `atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Regular.ttf` | `88ed5c31a71584c7772963b02d04bef1eb7e3d2e9c8b9cb204339b1f82cf432c` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/ttf/AtkinsonHyperlegibleNext-Regular.ttf> |
| `atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Bold.ttf` | `994414047df66bb4998d01c1cb1eeb4a2ddc4622d1aa56bbb8adbeca7645b041` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/ttf/AtkinsonHyperlegibleNext-Bold.ttf> |
| `atkinson-hyperlegible-next/OFL.txt` | `aca6a428580965d2297d1b718042dd427c2a9443ece3b0d02d758e161e0c4030` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/OFL.txt> |
| `atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Regular.ttf` | `2c9a7b9fabebd5c746a808d8a1ef2f52e618850caad6ec4fd9f5c48f56e1c1af` | <https://github.com/googlefonts/atkinson-hyperlegible-next-mono/blob/154d50362016cc3e873eb21d242cd0772384c8f9/fonts/ttf/AtkinsonHyperlegibleMono-Regular.ttf> |
| `atkinson-hyperlegible-mono/OFL.txt` | `1ebb31cf7393164f20d10c1d48406cddb5314feff8465531cf1e4ba37e9dd740` | <https://github.com/googlefonts/atkinson-hyperlegible-next-mono/blob/154d50362016cc3e873eb21d242cd0772384c8f9/OFL.txt> |

The fonts say of themselves (their `name` tables): version 2.001; "Copyright 2020-2024 The Atkinson
Hyperlegible Next Project Authors" (and "... Mono Project Authors"); designers Elliott Scott,
Megan Eiswerth, Linus Boman, Theodore Petrosky and Letters from Sweden; made by Applied Design
Works and Letters from Sweden; licensed under the SIL Open Font License, Version 1.1.

The book bundles the same families as WOFF2 files from the same commits (`docs/theme/fonts/`).

## Updating

Take the new static TTF files from the repositories' `fonts/ttf/` folders, whole, with the
licence file; read the licence again; put the new SHA-256 and the commit's address in the table
above; and draw the book's pictures of the app again.
