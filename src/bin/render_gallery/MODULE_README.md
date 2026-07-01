# Module: src/bin/render_gallery

## Purpose
Deterministic golden-image regression harness for the production text renderer
(`render_next::render_text_to_image`). Renders a fixed feature-matrix of cases to
PNGs so the vector-engine refactor (`render_next/VECTOR_ENGINE_REFACTOR.md`) can
be diffed visually / by tolerance before and after each phase.

This is a test/tooling binary. It renders the REAL engine; it does not copy or
reimplement any rendering logic.

## Architecture
The crate has no `lib.rs`, so the production renderer at
`crate::tabs::typing::render_next` is not reachable from a separate bin crate by
normal `use`. The engine also relies on fully-qualified self-paths
(`crate::tabs::typing::render_next::...`) and on `crate::trace`,
`crate::text_punctuation`, `crate::config`, `crate::tabs::typing::segmentation`.

To exercise the real engine, this bin re-mounts exactly that transitive module
closure at the same crate paths via `#[path]`:

- `trace`, `text_punctuation`, `config`, `bubble_status`, `memory_manager`
  (mounted at crate root in `main.rs`);
- `tabs::typing::{segmentation, render_next}` (mounted through the glue file
  `tabs/typing.rs`, which exists so `#[path]` `..` traversal passes through real
  directories).

Nothing outside this closure is mounted (no egui/app/GUI code).

## Files and submodules
- `main.rs`: engine mount, the fixed `all_cases()` set, PNG writing, the pure
  `rgba_diff`/`DiffStats` compare helper, and self-check `#[test]`s.
- `tabs/typing.rs`: glue that `#[path]`-mounts the real `render_next` and
  `segmentation` modules at their expected crate paths.

## Contracts and invariants
- Fully deterministic: no randomness, no time, fixed text/params/geometry.
- Uses the repo font `test/PanelCleaner/pcleaner/data/LiberationSans-Regular.ttf`
  (Latin + Cyrillic), resolved via `CARGO_MANIFEST_DIR`.
- `main` writes `<argv[1]>/<case>.png` and prints one `name: WxH` line per case.
- `rgba_diff` is pure: equal dimensions required, returns `Result`; never panics.
- The crate-level `#![allow(dead_code, unused_imports)]` covers only the embedded
  engine's unused-in-this-bin surface; harness code stays clippy-clean.

## Editing map
- To add/adjust a golden case, edit `all_cases()` in `main.rs`.
- To change how the engine is reached, edit the `#[path]` mounts in `main.rs`
  and `tabs/typing.rs` (keep them at the exact `crate::...` paths the engine
  expects).
- To change comparison semantics, edit `rgba_diff` / `DiffStats` and their tests.
