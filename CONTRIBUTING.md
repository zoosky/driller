---
title: Contributing
lead: "How to propose a change, the quality checks every pull request must pass, and how releases are cut."
menu:
  order: 7
---
# Contributing

Thank you for considering a contribution to driller.

## Getting Started

1. Fork the repository and clone your fork
2. Create a feature branch: `git checkout -b feature/your-change`
3. Make your changes
4. Run the quality checks (see below)
5. Commit and push to your fork
6. Open a pull request against `main`

## Quality Checks

All PRs must pass these checks before merge:

```bash
cargo fmt --all -- --check
cargo clippy -- -D warnings
cargo test
cargo audit
```

## Code Style

- Follow standard `rustfmt` formatting
- All public items need doc comments (`///`)
- Keep functions focused and short
- Prefer returning `Result` over panicking

## Pull Request Guidelines

- One logical change per PR
- Write a clear title and description explaining *why*, not just *what*
- Reference any related issues (e.g., "Fixes #123")
- Add tests for bug fixes and new features
- Keep commits clean -- squash fixups before requesting review

## Reporting Issues

Use GitHub Issues. Include:

- Driller version (`driller --version`)
- OS and architecture
- The benchmark YAML (minimized if possible)
- Expected vs. actual behavior
- Full error output

## Documentation website

The website at <https://zoosky.github.io/driller> is built from this
repository with [Accent CMS](https://accentcms.dev). The pages under
`/docs` are the markdown files you already edit -- `docs/*.md`, `SYNTAX.md`,
`example/README.md`, `FORK.md`, `CONTRIBUTING.md` -- mounted into the site by
`site/config.yaml`,
so a documentation change needs no second copy. The frontmatter block at the
top of each of those files sets the page title, its one-line summary, and
its `menu.order` in the docs navigation; keep it when editing. The landing
page, the getting-started guide, and the theme live under `site/`.

To preview locally, [install accent](https://docs.accentcms.dev/docs/getting-started)
and run, from the repository root:

```sh
accent serve -c site/config.yaml --no-tls   # http://127.0.0.1:4420/driller
```

The site is served under `/driller` locally as well, mirroring its GitHub
Pages project URL, so links behave exactly as they will once deployed.

To run the same build and link check as CI:

```sh
accent build -c site/config.yaml --clean -o site/output \
  --base-url https://zoosky.github.io/driller --strict-links
accent serve-static --dir site/output --no-tls   # http://127.0.0.1:4403/driller
```

The `Pages` workflow (`.github/workflows/pages.yml`) runs that build on every
push to `main` and deploys the output; pull requests that touch the site or
the mounted files get the build and link check only.

## Releasing

`driller --version` embeds the commit hash so a build can be traced back to its
source. The hash comes from `build.rs`, which reads `git rev-parse` in a normal
checkout. A `cargo install` from crates.io builds from the published tarball,
which has no `.git` -- so the hash must be written into the package at publish
time. When cutting a crates.io release, from the repo root:

```sh
git rev-parse --short HEAD > git-hash   # the release commit's short hash
git add -f git-hash                     # force-stage (the file is gitignored)
cargo publish --allow-dirty             # tarball now carries git-hash
git restore --staged git-hash && rm git-hash   # clean up; never commit it
```

`build.rs` prefers `git-hash` when present, so the installed binary reports the
release commit instead of `unknown`. The GitHub release binaries do not need
this step -- they build from a checkout (or receive `GITHUB_SHA` via
`Cross.toml`) and already embed the hash.

## License

By contributing, you agree that your contributions will be licensed under GPL-3.0, consistent with the project license.
