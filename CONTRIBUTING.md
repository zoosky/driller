# Contributing to Driller

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

## License

By contributing, you agree that your contributions will be licensed under GPL-3.0, consistent with the project license.
