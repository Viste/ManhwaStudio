/*
File: crates/ms-actions/src/raster_diff.rs

Purpose:
Pure, domain-agnostic, reversible RASTER DELTA primitive for the unified action
system (Phase 2a; see `docs/unified_action_system.md` sections 3.2 and 5). It is
the "diff, not full copy" building block for large webtoon-ribbon rasters where a
full RGBA snapshot per edit (a page can be ~800x19000 px ~= 58 MB) is
unaffordable. Feature-gated behind the `raster` cargo feature.

This GENERALIZES the single-bbox signed `[i16;4]` RGBA delta already used by
`CleanOverlaysModel` (`color_delta`/`apply_color_delta`): instead of one bbox it
TILES the image and stores only the changed tiles, each with its own tight bbox
plus a zstd-compressed signed delta.

Key structures:
- `RasterDiff`      — a tiled, compressed, reversible RGBA delta over one image.
- `RasterTileDiff`  — one changed tile: tight bbox + zstd(signed [i16;4]) payload.
- `DirtyRect`       — a changed rectangle reported back to the caller after apply.
- `ApplyDirection`  — Forward (redo, adds delta) / Reverse (undo, subtracts delta).
- `RasterDiffError` — typed, no-panic error surface.

Key functions:
- `RasterDiff::from_rgba`          — build a diff from full-image before/after
  straight-RGBA8 buffers.
- `RasterDiff::from_region_pixels` — build a diff from region-local before/after
  buffers, storing tile origins in image coordinates so it `apply`s against the
  full image identically to `from_rgba`.
- `RasterDiff::apply`              — apply the delta to a buffer in a given
  direction.

Notes:
- Operates on raw straight-alpha RGBA8 byte buffers (`&[u8]`, length == w*h*4).
- No egui/image/domain types. All heavy work (bbox scan + zstd) is synchronous
  here; the caller is responsible for scheduling it off the GUI thread.
- Reversibility invariant: for any before/after, `apply(after, Reverse) == before`
  and `apply(before, Forward) == after` (per-channel clamped to [0,255]).
- No panics on bad input: buffer length, image size, tile side, and payload
  integrity are validated up front and reported as `RasterDiffError`.
*/

//! Tiled, zstd-compressed, reversible RGBA8 delta primitive (`raster` feature).

use std::fmt;

/// Bytes per straight-alpha RGBA8 pixel.
const BYTES_PER_PIXEL: usize = 4;
/// Bytes per serialized signed delta pixel: 4 channels * `i16` (2 bytes each).
const DELTA_BYTES_PER_PIXEL: usize = 8;
/// zstd compression level for delta payloads. A modest level: signed deltas are
/// usually low-magnitude/sparse and compress well without heavy CPU cost.
const ZSTD_LEVEL: i32 = 3;

/// Direction of application.
///
/// `Forward` adds the stored delta (redo); `Reverse` subtracts it (undo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyDirection {
    /// Add the stored delta to the target buffer (redo).
    Forward,
    /// Subtract the stored delta from the target buffer (undo).
    Reverse,
}

/// A changed rectangle reported back to the caller after [`RasterDiff::apply`].
///
/// `origin_px` is the top-left in image coordinates; `size_px` is `[width,
/// height]` in pixels (both components > 0). One is produced per applied tile so
/// the caller can sync a mirror buffer or mark dirty regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRect {
    /// Top-left of the changed rectangle in image coordinates.
    pub origin_px: [u32; 2],
    /// `[width, height]` of the changed rectangle in pixels (> 0).
    pub size_px: [u32; 2],
}

/// A tight-bbox, zstd-compressed signed RGBA delta for one tile.
///
/// The payload is `zstd(signed [i16;4] per pixel)`, row-major over the bbox,
/// little-endian, interleaved RGBA. `origin_px`/`size_px` describe the tight bbox
/// of changed pixels in IMAGE coordinates. Fields are private; instances are only
/// produced by [`RasterDiff::from_rgba`], so they are always consistent with the
/// owning diff's `image_size`.
#[derive(Debug, Clone)]
pub struct RasterTileDiff {
    /// Tight bbox top-left in image coordinates.
    origin_px: [u32; 2],
    /// Tight bbox `[width, height]` in pixels (both > 0).
    size_px: [u32; 2],
    /// zstd-compressed signed delta bytes (interleaved LE `i16` RGBA, row-major).
    payload: Vec<u8>,
}

impl RasterTileDiff {
    /// Tight bbox top-left in image coordinates.
    #[must_use]
    pub fn origin_px(&self) -> [u32; 2] {
        self.origin_px
    }

    /// Tight bbox `[width, height]` in pixels.
    #[must_use]
    pub fn size_px(&self) -> [u32; 2] {
        self.size_px
    }

    /// Length in bytes of this tile's compressed payload.
    #[must_use]
    pub fn compressed_len(&self) -> usize {
        self.payload.len()
    }
}

/// A tiled, compressed, reversible RGBA8 delta over a full image.
///
/// Splits the image into `tile_side`-sized tiles and stores only the tiles that
/// changed, each as a [`RasterTileDiff`]. Applying is reversible from live pixels
/// alone (no second snapshot): `Forward` adds the delta, `Reverse` subtracts it,
/// per-channel clamped to `[0,255]`.
#[derive(Debug, Clone)]
pub struct RasterDiff {
    /// `[width, height]` of the image this diff was built against, in pixels.
    image_size: [u32; 2],
    /// Tile edge length in pixels used to partition the image (> 0).
    tile_side: u32,
    /// Changed tiles only; unchanged tiles are omitted.
    tiles: Vec<RasterTileDiff>,
}

/// Typed, no-panic error surface for [`RasterDiff`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RasterDiffError {
    /// A supplied RGBA buffer length did not equal `width * height * 4`.
    BufferLengthMismatch {
        /// The required length (`width * height * 4`).
        expected: usize,
        /// The actual buffer length that was supplied.
        got: usize,
    },
    /// The `image_size` passed to [`RasterDiff::apply`] does not match the size
    /// the diff was built against.
    ImageSizeMismatch {
        /// The image size stored in the diff.
        diff: [u32; 2],
        /// The image size supplied by the caller.
        target: [u32; 2],
    },
    /// `tile_side` was zero (an image cannot be partitioned into 0-sized tiles).
    InvalidTileSide,
    /// Image dimensions overflow addressable memory (`width * height * 4`, or a
    /// tile delta length, does not fit in `usize`). Unreachable for realistic
    /// inputs on supported 64-bit targets.
    DimensionOverflow,
    /// A tile's bbox falls outside the target buffer. Indicates a corrupt or
    /// mismatched diff; reported instead of panicking.
    TileOutOfBounds,
    /// A delta payload could not be zstd-compressed.
    Compression,
    /// A delta payload could not be decompressed or had an unexpected length
    /// (corrupt/undecodable payload).
    Decode,
}

impl fmt::Display for RasterDiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RasterDiffError::BufferLengthMismatch { expected, got } => write!(
                f,
                "RGBA buffer length mismatch: expected {expected} bytes (width*height*4), got {got}"
            ),
            RasterDiffError::ImageSizeMismatch { diff, target } => write!(
                f,
                "image size mismatch: diff was built for {}x{}, target is {}x{}",
                diff[0], diff[1], target[0], target[1]
            ),
            RasterDiffError::InvalidTileSide => {
                write!(f, "tile_side must be greater than zero")
            }
            RasterDiffError::DimensionOverflow => {
                write!(f, "image dimensions overflow addressable memory")
            }
            RasterDiffError::TileOutOfBounds => {
                write!(f, "a tile bbox falls outside the target buffer (corrupt diff)")
            }
            RasterDiffError::Compression => {
                write!(f, "failed to compress a raster delta payload")
            }
            RasterDiffError::Decode => {
                write!(f, "failed to decode a raster delta payload (corrupt or truncated)")
            }
        }
    }
}

impl std::error::Error for RasterDiffError {}

/// Compute the required RGBA8 buffer length (`w * h * 4`) as `usize`, guarding
/// against `usize` overflow for absurd dimensions.
///
/// # Errors
/// Returns [`RasterDiffError::DimensionOverflow`] if the product does not fit in
/// `usize`.
fn rgba_len(image_size: [u32; 2]) -> Result<usize, RasterDiffError> {
    // u32 -> usize is lossless on supported 64-bit targets; try_from keeps this
    // free of lossy `as` casts and reports the impossible case as an error rather
    // than panicking.
    let w = usize::try_from(image_size[0]).map_err(|_| RasterDiffError::DimensionOverflow)?;
    let h = usize::try_from(image_size[1]).map_err(|_| RasterDiffError::DimensionOverflow)?;
    w.checked_mul(h)
        .and_then(|px| px.checked_mul(BYTES_PER_PIXEL))
        .ok_or(RasterDiffError::DimensionOverflow)
}

impl RasterDiff {
    /// Build a diff from `before` -> `after`, both straight-alpha RGBA8 of exactly
    /// `image_size[0] * image_size[1] * 4` bytes.
    ///
    /// The image is split into `tile_side`-sized tiles (the last row/column is
    /// clamped to the image bounds; image dimensions need NOT be a multiple of
    /// `tile_side`). For each tile the tight bbox of changed pixels is computed
    /// and the zstd-compressed signed `after - before` delta is stored. Tiles with
    /// no change are omitted; if nothing changed the result is empty
    /// ([`Self::is_empty`] is `true`).
    ///
    /// # Errors
    /// - [`RasterDiffError::InvalidTileSide`] if `tile_side == 0`.
    /// - [`RasterDiffError::BufferLengthMismatch`] if either buffer length is not
    ///   `width * height * 4`.
    /// - [`RasterDiffError::DimensionOverflow`] for dimensions that overflow
    ///   addressable memory.
    /// - [`RasterDiffError::Compression`] if a delta payload cannot be compressed.
    pub fn from_rgba(
        before: &[u8],
        after: &[u8],
        image_size: [u32; 2],
        tile_side: u32,
    ) -> Result<Self, RasterDiffError> {
        if tile_side == 0 {
            return Err(RasterDiffError::InvalidTileSide);
        }
        let expected = rgba_len(image_size)?;
        if before.len() != expected {
            return Err(RasterDiffError::BufferLengthMismatch {
                expected,
                got: before.len(),
            });
        }
        if after.len() != expected {
            return Err(RasterDiffError::BufferLengthMismatch {
                expected,
                got: after.len(),
            });
        }

        let w = usize::try_from(image_size[0]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let h = usize::try_from(image_size[1]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let ts = usize::try_from(tile_side).map_err(|_| RasterDiffError::DimensionOverflow)?;

        let mut tiles = Vec::new();
        // Iterate the tile grid; div_ceil gives one extra (clamped) tile when the
        // image is not a multiple of the tile side.
        let tiles_y = h.div_ceil(ts);
        let tiles_x = w.div_ceil(ts);
        for tj in 0..tiles_y {
            let ty0 = tj * ts;
            let ty1 = (ty0 + ts).min(h);
            for ti in 0..tiles_x {
                let tx0 = ti * ts;
                let tx1 = (tx0 + ts).min(w);
                // Full-image scan: buffer width is the image width and tile
                // coordinates are already image coordinates (zero origin offset).
                if let Some(tile) =
                    Self::build_tile(before, after, w, [0, 0], [tx0, tx1, ty0, ty1])?
                {
                    tiles.push(tile);
                }
            }
        }

        Ok(Self {
            image_size,
            tile_side,
            tiles,
        })
    }

    /// Build a diff from region-local `before`/`after` buffers, avoiding a
    /// full-page scan when only a small region changed (e.g. a brush stroke).
    ///
    /// Both buffers are straight-alpha RGBA8, row-major over the region rect, of
    /// exactly `region_size[0] * region_size[1] * 4` bytes. The region must lie
    /// within `image_size` (`region_origin + region_size <= image_size`, per
    /// axis). The REGION is tiled by `tile_side`; each changed sub-tile is stored
    /// with its origin expressed in IMAGE coordinates (`region_origin` + local
    /// offset) and the same per-tile tight-bbox + interleaved LE `i16` delta +
    /// zstd payload as [`Self::from_rgba`]. The resulting diff therefore `apply`s
    /// against a FULL-image buffer identically to one produced by `from_rgba`
    /// with the same effective change. An empty region (or one with no changed
    /// pixels) yields an empty diff.
    ///
    /// # Errors
    /// - [`RasterDiffError::InvalidTileSide`] if `tile_side == 0`.
    /// - [`RasterDiffError::BufferLengthMismatch`] if either buffer length is not
    ///   `region_size[0] * region_size[1] * 4`.
    /// - [`RasterDiffError::TileOutOfBounds`] if the region does not fit within
    ///   `image_size`.
    /// - [`RasterDiffError::DimensionOverflow`] for dimensions that overflow
    ///   addressable memory.
    /// - [`RasterDiffError::Compression`] if a delta payload cannot be compressed.
    pub fn from_region_pixels(
        before: &[u8],
        after: &[u8],
        region_origin: [u32; 2],
        region_size: [u32; 2],
        image_size: [u32; 2],
        tile_side: u32,
    ) -> Result<Self, RasterDiffError> {
        if tile_side == 0 {
            return Err(RasterDiffError::InvalidTileSide);
        }
        // Region buffers are sized to the region area, not the image area.
        let expected = rgba_len(region_size)?;
        if before.len() != expected {
            return Err(RasterDiffError::BufferLengthMismatch {
                expected,
                got: before.len(),
            });
        }
        if after.len() != expected {
            return Err(RasterDiffError::BufferLengthMismatch {
                expected,
                got: after.len(),
            });
        }

        // Validate the region fits within the image. Compute in u64 so large
        // ribbon dimensions cannot overflow the addition (u32 + u32 always fits
        // in u64); a region straddling the image edge is a corrupt request.
        let fits_x =
            u64::from(region_origin[0]) + u64::from(region_size[0]) <= u64::from(image_size[0]);
        let fits_y =
            u64::from(region_origin[1]) + u64::from(region_size[1]) <= u64::from(image_size[1]);
        if !fits_x || !fits_y {
            return Err(RasterDiffError::TileOutOfBounds);
        }

        let rw = usize::try_from(region_size[0]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let rh = usize::try_from(region_size[1]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let ts = usize::try_from(tile_side).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let rox =
            usize::try_from(region_origin[0]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let roy =
            usize::try_from(region_origin[1]).map_err(|_| RasterDiffError::DimensionOverflow)?;

        let mut tiles = Vec::new();
        // Tile the REGION (not the image); local coordinates run over the region
        // rect and are translated to image space via the `[rox, roy]` offset.
        let tiles_y = rh.div_ceil(ts);
        let tiles_x = rw.div_ceil(ts);
        for tj in 0..tiles_y {
            let ty0 = tj * ts;
            let ty1 = (ty0 + ts).min(rh);
            for ti in 0..tiles_x {
                let tx0 = ti * ts;
                let tx1 = (tx0 + ts).min(rw);
                if let Some(tile) =
                    Self::build_tile(before, after, rw, [rox, roy], [tx0, tx1, ty0, ty1])?
                {
                    tiles.push(tile);
                }
            }
        }

        Ok(Self {
            image_size,
            tile_side,
            tiles,
        })
    }

    /// Scan one tile's pixel range for the tight bbox of changes and, if any,
    /// build its compressed [`RasterTileDiff`]. Returns `Ok(None)` for an
    /// unchanged tile.
    ///
    /// `buf_w` is the width in pixels of the buffers being scanned (the image
    /// width for a full-image scan, or the region width for a region scan).
    /// `tile_range` is `[tx0, tx1, ty0, ty1]`, the tile's half-open pixel range
    /// `[tx0,tx1) x [ty0,ty1)` in BUFFER-LOCAL coordinates (already clamped to the
    /// buffer bounds). `origin_offset` is added to the local tight-bbox top-left
    /// to produce the stored origin in IMAGE coordinates; it is `[0, 0]` for a
    /// full-image scan and `region_origin` for a region scan. The serialized delta
    /// payload is independent of `origin_offset` (it reads the same pixels either
    /// way), so a region-built tile is byte-identical to the equivalent full-image
    /// tile.
    fn build_tile(
        before: &[u8],
        after: &[u8],
        buf_w: usize,
        origin_offset: [usize; 2],
        tile_range: [usize; 4],
    ) -> Result<Option<RasterTileDiff>, RasterDiffError> {
        let [tx0, tx1, ty0, ty1] = tile_range;
        let w = buf_w;
        let mut min_x = usize::MAX;
        let mut min_y = usize::MAX;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        let mut changed = false;

        for y in ty0..ty1 {
            let row = y * w;
            for x in tx0..tx1 {
                let idx = (row + x) * BYTES_PER_PIXEL;
                // Both slices are exactly `expected` long (validated by the
                // caller), so `idx..idx+4` is always in range.
                if before[idx..idx + BYTES_PER_PIXEL] != after[idx..idx + BYTES_PER_PIXEL] {
                    changed = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }

        if !changed {
            return Ok(None);
        }

        let bw = max_x - min_x + 1;
        let bh = max_y - min_y + 1;
        // Serialize the signed delta row-major over the tight bbox: per pixel, 4
        // channels as little-endian i16 (after - before), interleaved RGBA.
        let raw_len = bw
            .checked_mul(bh)
            .and_then(|px| px.checked_mul(DELTA_BYTES_PER_PIXEL))
            .ok_or(RasterDiffError::DimensionOverflow)?;
        let mut raw = Vec::with_capacity(raw_len);
        for y in min_y..=max_y {
            let row = y * w;
            for x in min_x..=max_x {
                let idx = (row + x) * BYTES_PER_PIXEL;
                let before_px = &before[idx..idx + BYTES_PER_PIXEL];
                let after_px = &after[idx..idx + BYTES_PER_PIXEL];
                for (b, a) in before_px.iter().zip(after_px.iter()) {
                    // i16::from(u8) is lossless; the difference is in [-255, 255].
                    let delta = i16::from(*a) - i16::from(*b);
                    raw.extend_from_slice(&delta.to_le_bytes());
                }
            }
        }

        let payload =
            zstd::encode_all(raw.as_slice(), ZSTD_LEVEL).map_err(|_| RasterDiffError::Compression)?;

        // Translate the local tight-bbox top-left into image coordinates by
        // adding the region origin offset (zero for a full-image scan). bbox
        // coordinates originate from u32 dimensions, so the image-space result
        // fits back into u32; checked_add + try_from keep this overflow-safe and
        // lossless-cast-clean.
        let image_x = min_x
            .checked_add(origin_offset[0])
            .ok_or(RasterDiffError::DimensionOverflow)?;
        let image_y = min_y
            .checked_add(origin_offset[1])
            .ok_or(RasterDiffError::DimensionOverflow)?;
        let origin_px = [
            u32::try_from(image_x).map_err(|_| RasterDiffError::DimensionOverflow)?,
            u32::try_from(image_y).map_err(|_| RasterDiffError::DimensionOverflow)?,
        ];
        let size_px = [
            u32::try_from(bw).map_err(|_| RasterDiffError::DimensionOverflow)?,
            u32::try_from(bh).map_err(|_| RasterDiffError::DimensionOverflow)?,
        ];

        Ok(Some(RasterTileDiff {
            origin_px,
            size_px,
            payload,
        }))
    }

    /// Whether this diff carries no changed tiles.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// The `[width, height]` of the image this diff was built against.
    #[must_use]
    pub fn image_size(&self) -> [u32; 2] {
        self.image_size
    }

    /// The tile edge length in pixels used to partition the image.
    #[must_use]
    pub fn tile_side(&self) -> u32 {
        self.tile_side
    }

    /// Total compressed payload bytes across all tiles — for memory budgeting.
    #[must_use]
    pub fn compressed_len(&self) -> usize {
        self.tiles.iter().map(RasterTileDiff::compressed_len).sum()
    }

    /// Number of changed tiles stored in this diff.
    #[must_use]
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Read-only view of the stored tiles.
    #[must_use]
    pub fn tiles(&self) -> &[RasterTileDiff] {
        &self.tiles
    }

    /// Apply the delta to `buf` (straight-alpha RGBA8, must match `image_size`).
    ///
    /// `Forward` adds the delta, `Reverse` subtracts it; each channel result is
    /// clamped to `[0,255]`. Returns one [`DirtyRect`] per applied tile so the
    /// caller can sync a mirror buffer / mark dirty regions.
    ///
    /// # Errors
    /// - [`RasterDiffError::ImageSizeMismatch`] if `image_size` differs from the
    ///   size this diff was built against.
    /// - [`RasterDiffError::BufferLengthMismatch`] if `buf.len() != width*height*4`.
    /// - [`RasterDiffError::Decode`] if a payload cannot be decompressed or has an
    ///   unexpected length.
    /// - [`RasterDiffError::TileOutOfBounds`] if a tile bbox falls outside `buf`
    ///   (corrupt diff).
    pub fn apply(
        &self,
        buf: &mut [u8],
        image_size: [u32; 2],
        dir: ApplyDirection,
    ) -> Result<Vec<DirtyRect>, RasterDiffError> {
        if image_size != self.image_size {
            return Err(RasterDiffError::ImageSizeMismatch {
                diff: self.image_size,
                target: image_size,
            });
        }
        let expected = rgba_len(image_size)?;
        if buf.len() != expected {
            return Err(RasterDiffError::BufferLengthMismatch {
                expected,
                got: buf.len(),
            });
        }

        let w = usize::try_from(image_size[0]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        // Reverse subtracts the stored delta; Forward adds it. Applied per channel
        // as `current + sign * delta`, then clamped.
        let sign: i16 = match dir {
            ApplyDirection::Forward => 1,
            ApplyDirection::Reverse => -1,
        };

        let mut dirty = Vec::with_capacity(self.tiles.len());
        for tile in &self.tiles {
            Self::apply_tile(buf, w, tile, sign)?;
            dirty.push(DirtyRect {
                origin_px: tile.origin_px,
                size_px: tile.size_px,
            });
        }
        Ok(dirty)
    }

    /// Decompress one tile's payload and apply it to `buf` in place.
    ///
    /// `w` is the image width in pixels; `sign` is +1 (Forward) or -1 (Reverse).
    fn apply_tile(
        buf: &mut [u8],
        w: usize,
        tile: &RasterTileDiff,
        sign: i16,
    ) -> Result<(), RasterDiffError> {
        let ox = usize::try_from(tile.origin_px[0]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let oy = usize::try_from(tile.origin_px[1]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let bw = usize::try_from(tile.size_px[0]).map_err(|_| RasterDiffError::DimensionOverflow)?;
        let bh = usize::try_from(tile.size_px[1]).map_err(|_| RasterDiffError::DimensionOverflow)?;

        let raw = zstd::decode_all(tile.payload.as_slice()).map_err(|_| RasterDiffError::Decode)?;
        let expected_raw = bw
            .checked_mul(bh)
            .and_then(|px| px.checked_mul(DELTA_BYTES_PER_PIXEL))
            .ok_or(RasterDiffError::DimensionOverflow)?;
        if raw.len() != expected_raw {
            return Err(RasterDiffError::Decode);
        }

        let mut ri = 0usize;
        for y in oy..oy + bh {
            let row = y * w;
            for x in ox..ox + bw {
                let idx = (row + x) * BYTES_PER_PIXEL;
                // Report a corrupt/mismatched tile instead of panicking on a bad
                // index. For diffs built by `from_rgba` against a matching image
                // size this always succeeds.
                let px = buf
                    .get_mut(idx..idx + BYTES_PER_PIXEL)
                    .ok_or(RasterDiffError::TileOutOfBounds)?;
                for ch in px.iter_mut() {
                    // `ri` walks exactly `bw*bh*8` bytes (validated above), so both
                    // indices are always in range.
                    let delta = i16::from_le_bytes([raw[ri], raw[ri + 1]]);
                    ri += 2;
                    // i16::from(u8) is lossless; `saturating_mul` guards the sign
                    // flip; `clamp` bounds to [0,255] so `u8::try_from` never fails.
                    let value = (i16::from(*ch) + delta.saturating_mul(sign)).clamp(0, 255);
                    *ch = u8::try_from(value).unwrap_or(u8::MAX);
                }
            }
        }
        Ok(())
    }
}
