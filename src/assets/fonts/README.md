# Vendored fonts

Self-hosted so the app renders identically offline on every machine.
Latin subset, variable weight axis only (upright — no italic file is shipped;
italic is synthesized off, see `font-synthesis: none` in styles/base.css).

| File | Source | Version | License |
| --- | --- | --- | --- |
| `inter-latin-wght-normal.woff2` | `@fontsource-variable/inter` | 5.3.0 | SIL OFL 1.1 |
| `jetbrains-mono-latin-wght-normal.woff2` | `@fontsource-variable/jetbrains-mono` | 5.3.0 | SIL OFL 1.1 |

To update: `npm i -D @fontsource-variable/inter @fontsource-variable/jetbrains-mono`,
copy `files/*-latin-wght-normal.woff2` here, then remove the packages again.
