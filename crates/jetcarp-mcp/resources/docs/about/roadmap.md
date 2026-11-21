# Roadmap

What is planned for JETCARP's next releases, in order. Each release has one large feature, with the
smaller ones that support it; bug fixes go in as soon as they can, in whichever release is next. The
details of each release, and their progress, are in its tracking issue and its milestone on GitHub,
which are kept current; this page is updated with each release.

| Release | The large feature | Tracking |
| --- | --- | --- |
| 0.4.3 | **Accessibility**, led by **the strobe guard**: JETCARP notices flashing and strong stripes in what it draws, and softens, slows or pauses the scene, in every output, exported animations included. Also palettes for colour-vision deficiency, reduced motion, the keyboard and screen readers, a high-contrast theme, and an [accessibility statement](accessibility.md). And every bug fix that can go in. | [#97](https://github.com/jprodgers/jetcarp/issues/97), [milestone](https://github.com/jprodgers/jetcarp/milestone/2) |
| 0.4.4 | **Exchange with Golly and LifeViewer**: rule strings both ways, complete RLE import and export, plaintext, Life 1.05/1.06 and Macrocell, hexagonal and triangular coordinates, Golly's `.rule` files, and an automatic comparison with Golly | [#152](https://github.com/jprodgers/jetcarp/issues/152), [milestone](https://github.com/jprodgers/jetcarp/milestone/3) |
| 0.4.5 | **Printing**: PDFs of any world, paper sizes and physical scale, posters across many pages, contact sheets and tiling paper | [#161](https://github.com/jprodgers/jetcarp/issues/161) |
| 0.4.6 | **Reports**: HTML and PDF reports from scripted templates, and long research runs that write their own | [#162](https://github.com/jprodgers/jetcarp/issues/162) |
| 0.4.7 | **Simple CAs and massive worlds**: one bit per cell, billions of cells, on every periodic tiling, stepped on the GPU | [#163](https://github.com/jprodgers/jetcarp/issues/163) |
| 0.5.0 | **The Python API and notebooks**: `import jetcarp`, with the same results as the command line | [#164](https://github.com/jprodgers/jetcarp/issues/164) |
| 0.5.1 | Python follow-ups: custom rules and tilings from Python, and conda-forge | [#165](https://github.com/jprodgers/jetcarp/issues/165) |
| 0.5.2 | **Scene checks**: goals and assertions in scenes, for research and testing, and the engine of the puzzles that follow | [#166](https://github.com/jprodgers/jetcarp/issues/166) |
| 0.6.0 | **Cellular Automata Puzzles**, a prototype: a puzzle game inside JETCARP that teaches cellular automata and tilings | [#167](https://github.com/jprodgers/jetcarp/issues/167) |
| 0.6.1 | Making and sharing puzzle packs | [#168](https://github.com/jprodgers/jetcarp/issues/168) |
| 0.7.0 | **Networking**, carefully and only where asked for: fetching packs and updates from a URL with their signatures checked, and creating torrents of packs | [#169](https://github.com/jprodgers/jetcarp/issues/169) |
| 0.8.0 | **A built-in BitTorrent client**, for packs that run to many gigabytes | [#170](https://github.com/jprodgers/jetcarp/issues/170) |

Until 0.7.0, JETCARP makes no network connections at all (see
[SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)).

**1.0** will be the release that declares the scene format, the command line, the scripting API and
the MCP tools stable ([Changelog](changelog.md)). What else it must have is not decided yet.

The order can change, and anything here can be discussed: ask in
[Discussions](https://github.com/jprodgers/jetcarp/discussions), or open an issue for a feature you
would like (see [Contributing](contributing.md)).
