/*
File: crates/ms-actions/src/action.rs

Purpose:
Defines the `ReversibleAction` trait — the self-inverting command contract that
the generic `ActionHistory` engine drives. Modelled on Koharu's `Op`: an action
carries its forward patch AND, after `apply`, the previous state it overwrote, so
`inverse()` is a pure local operation that never touches the domain context.

Key items:
- trait `ReversibleAction`

Notes:
GUI-free and domain-free. Concrete actions (bubble patches, raster diffs) are
implemented by the main crate in later phases; nothing here knows about them.
*/

/// Self-inverting command over a mutable domain context.
///
/// An implementor is a single, one-shot command instance. `apply` mutates the
/// context AND captures into `self` the previous state it overwrote, so that a
/// later `inverse()` can build the reverse command purely from local data,
/// without reading the context again. This mirrors Koharu's `Op` contract
/// (forward patch + captured `prev`).
///
/// # Lifecycle contract
/// For a given instance, `apply` must be called **exactly once** before
/// `inverse()` is called. `inverse()` is only valid after `apply` has populated
/// the captured previous state. Implementors that expose captured fields as
/// `Option` should treat a missing capture as a programming error and surface it
/// through `Err`, never a panic.
pub trait ReversibleAction: Sized {
    /// The mutable domain context this action operates on (models, document,
    /// scene, etc.). Supplied by the caller to `apply`.
    type Ctx;

    /// Error returned when an action cannot be applied to the context.
    type Err;

    /// Apply the action forward, mutating `ctx`, and capture the previous state
    /// it overwrote into `self` (populating the `prev` fields) so a subsequent
    /// `inverse()` is pure.
    ///
    /// Contract: call exactly once per instance before `inverse()`. On `Err`,
    /// the implementor must leave `ctx` unchanged (or document the exact partial
    /// effect); the history engine treats an error as "nothing happened".
    ///
    /// # Errors
    /// Returns `Self::Err` when the action cannot be applied — for example when
    /// its target no longer exists in `ctx` or a precondition is violated.
    fn apply(&mut self, ctx: &mut Self::Ctx) -> Result<(), Self::Err>;

    /// Produce the reverse action.
    ///
    /// Pure and local: only valid **after** `apply` has run on this instance
    /// (its captured previous state is populated). Must not read or mutate any
    /// context. The returned action, when `apply`ed, undoes this one.
    #[must_use]
    fn inverse(&self) -> Self;

    /// Human-readable label for this action, for UI history and logging.
    fn label(&self) -> &str;

    /// Approximate retained size of this action in bytes, for history memory
    /// budgeting by [`crate::ActionHistory`].
    ///
    /// Defaults to `0` for actions with negligible footprint (e.g. small field
    /// snapshots), so existing implementors are unaffected. Actions backed by
    /// large buffers (raster diffs) should override this to return their
    /// compressed payload size so the history engine can bound total undo-stack
    /// memory. The value must be stable for a given instance: the engine tracks
    /// the undo-stack total incrementally by adding this on push and subtracting
    /// the same on eviction/undo.
    fn weight(&self) -> usize {
        0
    }
}
