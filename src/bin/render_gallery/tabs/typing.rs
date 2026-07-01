/*
File: src/bin/render_gallery/tabs/typing.rs

Purpose:
Glue module for the `render_gallery` harness. Mounts the production
`render_next` renderer and its `segmentation` dependency at the exact crate
paths their source expects (`crate::tabs::typing::render_next` /
`crate::tabs::typing::segmentation`) without copying any engine code.

Notes:
Paths are resolved relative to this file's directory
(`src/bin/render_gallery/tabs/`). Only the harness (`main.rs`) consumes these
re-mounted modules.
*/

#[path = "../../../tabs/typing/segmentation/mod.rs"]
pub mod segmentation;

#[path = "../../../tabs/typing/render_next/mod.rs"]
pub mod render_next;
