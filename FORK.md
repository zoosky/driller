# FORK.md

**Driller** is a friendly fork of [fcsonline/drill](https://github.com/fcsonline/drill).

## Relationship to upstream

- **Same license.** GPL-3.0-or-later. The original `LICENSE` file is preserved
  unchanged.
- **Same DSL.** Existing `drill` benchmark YAML files work without modification.
- **Different binary name.** `driller` everywhere -- crate, binary, User-Agent.

## Migration

```bash
cargo uninstall drill
cargo install driller

# benchmark files don't change
driller --benchmark benchmark.yml --stats
```

Replace `drill` with `driller` in scripts and Docker images. Benchmark YAML
does not need changes.

## License

GPL-3.0-or-later. See [LICENSE](./LICENSE).
