# The fonts of the pages JETCARP writes

The HTML pages JETCARP writes are set in **Atkinson Hyperlegible Next** and, for code and other
monospaced text, **Atkinson Hyperlegible Mono**, as the app and the book are: fonts whose letters
are easier to tell apart, designed by the Braille Institute for low-vision readers.
`src/web_fonts.rs` embeds the files below, and each page carries them inside itself (as `data:`
addresses in its style sheet), so it works as one file opened offline and fetches nothing from
another site. The pages: the clip-rating page (`jetcarp jobs clips --page`, Next and Mono) and
the website export's page (`jetcarp site export`, Next).

Each file is the copyright holders' own WOFF2 file, whole and unmodified, from the families'
official repositories on GitHub, kept by Google Fonts for the Braille Institute: the same files
as the book's (`docs/theme/fonts/`). Both families are licensed under the SIL Open Font License,
Version 1.1; `OFL-AtkinsonHyperlegibleNext.txt` and `OFL-AtkinsonHyperlegibleMono.txt` are each
family's `OFL.txt`, exactly as its repository gives it (renamed only so both fit in one folder).
Neither names a Reserved Font Name. A test (`web_fonts::tests`) checks that each embedded file's
SHA-256 is the one here.

| File | SHA-256 | Source |
| --- | --- | --- |
| `AtkinsonHyperlegibleNext-Regular.woff2` | `378aea0f5c1d179f4e0b5382c06bfc87571b98cfcc4fd1352bc979e2e2259c54` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/webfonts/AtkinsonHyperlegibleNext-Regular.woff2> |
| `AtkinsonHyperlegibleNext-Bold.woff2` | `dda449f0f556a595cffd0a9ce479bb1210beba286cb4c2f5aeca6975f9c85a3b` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/webfonts/AtkinsonHyperlegibleNext-Bold.woff2> |
| `AtkinsonHyperlegibleMono-Regular.woff2` | `3b916de5c5247c5fa9736aad6c2673f1b0b0381ab306dcddfcee4a184a842a51` | <https://github.com/googlefonts/atkinson-hyperlegible-next-mono/blob/154d50362016cc3e873eb21d242cd0772384c8f9/fonts/webfonts/AtkinsonHyperlegibleMono-Regular.woff2> |
| `OFL-AtkinsonHyperlegibleNext.txt` | `aca6a428580965d2297d1b718042dd427c2a9443ece3b0d02d758e161e0c4030` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/OFL.txt> |
| `OFL-AtkinsonHyperlegibleMono.txt` | `1ebb31cf7393164f20d10c1d48406cddb5314feff8465531cf1e4ba37e9dd740` | <https://github.com/googlefonts/atkinson-hyperlegible-next-mono/blob/154d50362016cc3e873eb21d242cd0772384c8f9/OFL.txt> |

The fonts say of themselves: version 2.001; "Copyright 2020-2024 The Atkinson Hyperlegible Next
Project Authors" (and "... Mono Project Authors"); licensed under the SIL Open Font License,
Version 1.1. The app embeds the same families as TTF files from the same commits
(`crates/jetcarp-display/fonts/`).

## Updating

Take the new WOFF2 files from the repositories' `fonts/webfonts/` folders, whole, with the
licence files; read the licences again; and put the new SHA-256 and the commit's address in the
table above, as for the book's and the app's copies.
