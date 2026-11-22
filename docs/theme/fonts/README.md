# The book's fonts

The book's text is set in **Atkinson Hyperlegible Next** and its code in **Atkinson Hyperlegible
Mono**, fonts whose letters are easier to tell apart, designed by the Braille Institute for
low-vision readers. `fonts.css` loads them from here, so a reader's browser fetches them from the
book itself and never from another site; mdBook copies this folder to the book's `fonts/`.

Each file is the copyright holders' own WOFF2 file, whole and unmodified, from the families'
official repositories on GitHub, kept by Google Fonts for the Braille Institute. Both families are
licensed under the SIL Open Font License, Version 1.1; `OFL-AtkinsonHyperlegibleNext.txt` and
`OFL-AtkinsonHyperlegibleMono.txt` are each family's `OFL.txt`, exactly as its repository gives
it (renamed only so both fit in one folder).

| File | SHA-256 | Source |
| --- | --- | --- |
| `AtkinsonHyperlegibleNext-Regular.woff2` | `378aea0f5c1d179f4e0b5382c06bfc87571b98cfcc4fd1352bc979e2e2259c54` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/webfonts/AtkinsonHyperlegibleNext-Regular.woff2> |
| `AtkinsonHyperlegibleNext-Bold.woff2` | `dda449f0f556a595cffd0a9ce479bb1210beba286cb4c2f5aeca6975f9c85a3b` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/webfonts/AtkinsonHyperlegibleNext-Bold.woff2> |
| `AtkinsonHyperlegibleNext-Italic.woff2` | `97f9cffe4912f3ceaa41583dca26ae4874b4847243a826c87203071d8ad601ad` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/webfonts/AtkinsonHyperlegibleNext-Italic.woff2> |
| `AtkinsonHyperlegibleNext-BoldItalic.woff2` | `d73949a5c24b5f91e64185034ea0af96902f27ce84b3b413aab30be5d0cb371e` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/webfonts/AtkinsonHyperlegibleNext-BoldItalic.woff2> |
| `AtkinsonHyperlegibleMono-Regular.woff2` | `3b916de5c5247c5fa9736aad6c2673f1b0b0381ab306dcddfcee4a184a842a51` | <https://github.com/googlefonts/atkinson-hyperlegible-next-mono/blob/154d50362016cc3e873eb21d242cd0772384c8f9/fonts/webfonts/AtkinsonHyperlegibleMono-Regular.woff2> |
| `AtkinsonHyperlegibleMono-Bold.woff2` | `039936edaa268366426a7f0ca8014da637b8bf8da24859afbd85a5b313e9315e` | <https://github.com/googlefonts/atkinson-hyperlegible-next-mono/blob/154d50362016cc3e873eb21d242cd0772384c8f9/fonts/webfonts/AtkinsonHyperlegibleMono-Bold.woff2> |
| `AtkinsonHyperlegibleMono-Italic.woff2` | `298a41e6e49f031f28d5e256db8cd3ab57e66048619744189e972f56fe612883` | <https://github.com/googlefonts/atkinson-hyperlegible-next-mono/blob/154d50362016cc3e873eb21d242cd0772384c8f9/fonts/webfonts/AtkinsonHyperlegibleMono-Italic.woff2> |
| `OFL-AtkinsonHyperlegibleNext.txt` | `aca6a428580965d2297d1b718042dd427c2a9443ece3b0d02d758e161e0c4030` | <https://github.com/googlefonts/atkinson-hyperlegible-next/blob/7925f50f649b3813257faf2f4c0b381011f434f1/OFL.txt> |
| `OFL-AtkinsonHyperlegibleMono.txt` | `1ebb31cf7393164f20d10c1d48406cddb5314feff8465531cf1e4ba37e9dd740` | <https://github.com/googlefonts/atkinson-hyperlegible-next-mono/blob/154d50362016cc3e873eb21d242cd0772384c8f9/OFL.txt> |

The app embeds the same families as TTF files from the same commits
(`crates/jetcarp-display/fonts/`).
