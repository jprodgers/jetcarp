## What and why

<!-- What this changes, and why. Link the issue it fixes, if there is one: "Fixes #123". -->

## Checklist

- [ ] `cargo fmt --all` and `cargo test --workspace --release` pass
- [ ] New behaviour has tests (a new rule, tiling or search: one showing its result doesn't depend on the thread count)
- [ ] No determinism pin re-pinned, or the commit message says which pins changed and why
- [ ] The book is updated if behaviour changed (`python3 docs/tools/check-links.py`; `python3 docs/tools/gen-cli.py --check` for help text)
- [ ] A changelog line under the unreleased version, if a user would notice the change
- [ ] Any material from elsewhere has its licence and a notice in `THIRD_PARTY_NOTICES.md`
- [ ] If an AI tool helped, I say so here, and I have run and understand every change

By opening this pull request, I offer my contribution under JETCARP's terms: the Unlicense, or the MIT licence where that is not recognised.
