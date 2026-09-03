---
title: Migrating from drill
lead: "How driller relates to fcsonline/drill, what stays the same, and how to switch."
menu:
  order: 5
---
# Migrating from drill

**Driller** is a maintained fork of [fcsonline/drill](https://github.com/fcsonline/drill).

## Relationship to upstream

- **Same license.** GPL-3.0-or-later. The original `LICENSE` file is preserved
  unchanged.
- **Same DSL.** Existing `drill` benchmark YAML files work without modification.
- **Different binary name.** `driller` everywhere -- crate, binary, User-Agent. To prevent name clash with the driller DNS tool.

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

GPL-3.0-or-later. See [LICENSE](https://github.com/zoosky/driller/blob/main/LICENSE).
