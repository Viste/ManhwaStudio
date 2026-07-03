/*
File: crates/ms-actions/src/lib.rs

Purpose:
Crate root of `ms-actions` — the pure, GUI-free generic undo/redo engine for
ManhwaStudio (Phase 0 of the unified action system, see
`docs/unified_action_system.md`). It provides ONLY the generic mechanism: the
`ReversibleAction` trait (Koharu-style self-inverting command) and the in-memory
`ActionHistory` engine.

Main responsibilities:
- expose the `ReversibleAction` command contract;
- expose the generic `ActionHistory<A>` undo/redo engine (apply/undo/redo, limit,
  redo-branch truncation);
- under the optional `raster` feature, expose `raster_diff` — a pure,
  domain-agnostic, reversible tiled RGBA delta primitive (Phase 2a).

Contract:
- The default build has no dependency on the application crate, on any domain
  type, or on GUI/async/image crates. Concrete domain Ops (bubble patches, layer
  edits) are implemented in the main crate in later phases and are deliberately
  NOT part of this crate.
- The `raster` feature is the ONLY thing that pulls an extra dependency (`zstd`);
  it stays fully gated so `cargo check -p ms-actions` (no features) is std-only.
*/

#![warn(clippy::all)]
#![warn(clippy::pedantic)]

pub mod action;
pub mod history;

#[cfg(feature = "raster")]
pub mod raster_diff;

pub use action::ReversibleAction;
pub use history::ActionHistory;

#[cfg(feature = "raster")]
pub use raster_diff::{
    ApplyDirection, DirtyRect, RasterDiff, RasterDiffError, RasterTileDiff,
};
