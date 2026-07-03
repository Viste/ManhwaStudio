/*
File: crates/ms-text-util/src/lib.rs

Purpose:
Crate root of `ms-text-util` — config-free text utilities shared between the
ManhwaStudio binary and the text renderer (`ms-text-render`).

Modules:
- `text_punctuation`: the global hanging-punctuation set (seeded by the app).
- `segmentation`: language-aware line/unit segmentation used by wrapping.

Contract:
- No dependency on the application crate or its config. The hanging-punctuation
  set defaults to `DEFAULT_HANGING_PUNCTUATION`; the app seeds the user value via
  `text_punctuation::set_hanging_punctuation` at startup.
*/

pub mod segmentation;
pub mod text_punctuation;
