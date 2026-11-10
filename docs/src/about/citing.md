# Citing JETCARP

If JETCARP helps your research, please cite it. The citation metadata is in
[`CITATION.cff`](https://github.com/jprodgers/jetcarp/blob/main/CITATION.cff) in the repository, which
GitHub's *Cite this repository* button reads.

<div class="checked">

**DOI pending.** Each release will be archived on Zenodo with a DOI. The DOI for version 0.4.0 does
not exist yet; it will be added here and to `CITATION.cff` when the release is archived. Until then,
cite the version and the repository.

</div>

## The software

> Rodgers, J. (2026). *JETCARP: Jimmie's Euclidean Tilings and Cellular Automata Research Program*
> (Version 0.4.0) [Computer software]. <https://github.com/jprodgers/jetcarp>

| Field | Value |
| --- | --- |
| Title | JETCARP: Jimmie's Euclidean Tilings and Cellular Automata Research Program |
| Author | Jimmie Rodgers, Independent, ORCID [0009-0003-8775-6483](https://orcid.org/0009-0003-8775-6483) |
| Version | 0.4.0 |
| Licence | Unlicense OR MIT |
| Repository | <https://github.com/jprodgers/jetcarp> |
| DOI | pending (see above) |

In BibTeX (the `@software` entry type needs biblatex; with a classic BibTeX style, use `@misc` with
the same fields):

```bibtex
@software{rodgers2026jetcarp,
  author  = {Rodgers, Jimmie},
  title   = {{JETCARP}: Jimmie's Euclidean Tilings and Cellular Automata Research Program},
  version = {0.4.0},
  year    = {2026},
  url     = {https://github.com/jprodgers/jetcarp},
  license = {Unlicense OR MIT},
  note    = {Zenodo DOI pending}
}
```

A paper describing JETCARP is planned. Until it appears, please cite the software.

## Results made with JETCARP

When you report something you ran or found with JETCARP, the most useful citation also says exactly
what was run:

- **The version**, since a rule or tiling can change between releases before 1.0 (see
  [Reproducibility](../research/reproducibility.md#what-is-not-guaranteed-yet)).
- **The scene**, which lets anyone replay the run, and its state hash, which lets them check they
  got the same result (`jetcarp verify`). A scene file is small enough to publish as supplementary
  material.
- **For catalogued patterns**, the pattern's id, which is computed from how the pattern evolves, not
  from the rule it was found in, together with its tiling and rule range.

If you use one of the results on the [Findings](../research/findings.md) page, please credit it as
"found with JETCARP by Jimmie Rodgers", and check the claims and flags there before calling it new.

## The work JETCARP builds on

Most of what JETCARP runs is other people's work, and a paper that uses it should cite them too: the
rule family's authors, the tiling's discoverers, and the authors of any data. Each encyclopedia page
gives its references, and the [bibliography](../research/bibliography.md) collects them all. Three
cases in particular:

- **James Edmond's** priority tables, Spectre glider and evolutionary glider search
  ([Edmond 2026](../research/bibliography.md#edmond2026gliders)).
- **The hat and spectre tilings**, by David Smith, Joseph Samuel Myers, Craig S. Kaplan and Chaim
  Goodman-Strauss ([Smith et al. 2023](../research/bibliography.md#smith2023aperiodic);
  [2023b](../research/bibliography.md#smith2023chiral)).
- **The k-uniform tilings**, Brian Galebach's catalogue in the encoding of J. E. Soto Sánchez and
  co-authors, which JETCARP uses under the Creative Commons Attribution 4.0 licence; that licence asks
  for attribution when the data are used (see [Licence](licence.md#third-party-work) and
  [k-uniform tilings](../tilings/k-uniform.md)).
