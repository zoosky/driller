# FORK.md

**Driller** is a friendly fork of [fcsonline/drill](https://github.com/fcsonline/drill),
a small, fast, Rust-native HTTP load testing tool with an Ansible-inspired
YAML DSL.

## Why fork?

Upstream `drill` is healthy in its architecture but **bursty-to-absent in
maintenance**: multi-year activity gaps, several reported bugs with mergeable
fixes that have sat unreviewed for 3+ years (notably the histogram panic on
responses >3.6 s — issues #151/#165/#174/#195/#201/#216, fix proposed in PR
#223), unaddressed RUSTSEC advisories, and an unresolved name collision with
Apache Drill (issue #181). Another community contributor publicly proposed a
fork in March 2025 (issue #200). We're picking up that thread.

The goal of this fork is to **be a credible, actively maintained successor**:
clear the open PR backlog, ship the known-good fixes, modernize the dependency
tree, and commit to a predictable release cadence.

## Relationship to upstream

- **Same license.** Driller stays GPL-3.0-or-later. The original `LICENSE`
  file is preserved unchanged and continues to carry Ferran Basora's copyright
  on the existing code.
- **Same architecture and DSL.** Existing `drill` benchmark YAML files run on
  `driller` without modification. The CLI surface is compatible.
- **Different binary name.** The binary is `driller`, the crate is `driller`,
  the HTTP `User-Agent` is `driller`. If you're migrating, the only thing that
  changes in your CI scripts is the command name.
- **We intend to upstream fixes.** Where it's reasonable, fixes landed here
  will be opened as PRs against `fcsonline/drill` as well. We are not trying
  to compete with the upstream; we're trying to provide an available
  alternative for users who need predictable maintenance.

## Migration

For users moving from `drill` to `driller`:

```bash
# remove upstream
cargo uninstall drill

# install the fork
cargo install driller

# benchmark files don't change
driller --benchmark benchmark.yml --stats
```

If you depend on the binary by path in scripts or Docker images, replace
`drill` with `driller`. Configuration files and benchmark YAML do not need
changes.

## Governance

This fork is maintained by **Andreas Kapp**. We commit to:

- Monthly point releases, at minimum, for the first 12 months.
- Weekly issue/PR triage.
- A documented release process and publicly stated maintainers.
- Friendly coordination with upstream — we will not block or compete on
  attention.

## Credits

The original `drill` was written by [Ferran Basora](https://github.com/fcsonline)
and ~29 contributors over multiple years. This fork stands on their work.
If you appreciate the upstream tool, consider supporting Ferran via his
[Buy Me a Coffee](https://www.buymeacoffee.com/fcsonline) page.

## License

`driller` is distributed under the GNU General Public License v3.0 or later
(`GPL-3.0-or-later`), the same license as upstream `drill`. See `LICENSE` for
the full text.
