# Module: crates/ms-text-util

## Purpose
Config-free text utilities shared by the app binary and the text renderer
(`ms-text-render`): the global hanging-punctuation set and language-aware line
segmentation. Extracted from `src/` so the renderer can be its own crate without
reaching back into the application crate.

## Architecture
Two independent modules. Neither reads config or touches the filesystem.

- `text_punctuation`: a process-global, generation-counted set of "hanging"
  punctuation characters with a thread-local snapshot cache for the hot path
  (`is_hanging_punctuation`, called per char in tight render/wrap loops).
- `segmentation`: splits text into layout units and offers hyphenation-aware
  segmentation (via the `hyphenation` crate), consumed by the renderer's wrap path.

## Files and submodules
- `src/lib.rs`: crate root; re-exports both modules.
- `src/text_punctuation.rs`: `is_hanging_punctuation`, `set_hanging_punctuation`,
  `hanging_punctuation_string`, `DEFAULT_HANGING_PUNCTUATION`.
- `src/segmentation/`: `base` (segmenter + conservatism), `ru` (Russian rules), `mod`.

## Contracts and invariants
- No dependency on the app crate or `config`. The hanging-punctuation set defaults
  to `DEFAULT_HANGING_PUNCTUATION`; the **app** seeds the user value at startup via
  `set_hanging_punctuation` (`main.rs::seed_hanging_punctuation_from_config`).
- `segmentation` depends on `text_punctuation` within this crate (`crate::text_punctuation`).

## Editing map
- To change which characters hang, edit `DEFAULT_HANGING_PUNCTUATION` (and the app's
  config default) — see `text_punctuation.rs`.
- To change segmentation/hyphenation behavior, see `segmentation/`.
