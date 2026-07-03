// Contract tests for the `raster_diff` primitive. The entire file is gated on the
// `raster` feature so a default `cargo test -p ms-actions` stays std-only.
#![cfg(feature = "raster")]

use ms_actions::{ApplyDirection, DirtyRect, RasterDiff, RasterDiffError};

/// A tiny deterministic xorshift PRNG so tests are reproducible without an extra
/// dependency.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u8(&mut self) -> u8 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        u8::try_from(x & 0xff).unwrap_or(0)
    }
}

/// Build a random straight-RGBA8 buffer of `w*h*4` bytes.
fn random_rgba(w: u32, h: u32, seed: u64) -> Vec<u8> {
    let len = (w as usize) * (h as usize) * 4;
    let mut rng = Rng::new(seed);
    (0..len).map(|_| rng.next_u8()).collect()
}

/// Index of the first byte of pixel (x, y) in a `w`-wide RGBA8 buffer.
fn px_idx(w: u32, x: u32, y: u32) -> usize {
    ((y as usize) * (w as usize) + (x as usize)) * 4
}

#[test]
fn round_trip_identity_forward_and_reverse() {
    let (w, h) = (64u32, 48u32);
    let before = random_rgba(w, h, 0x1234);
    let mut after = before.clone();
    // Change a scattered set of pixels across several tiles.
    for (x, y) in [(1u32, 1u32), (30, 20), (63, 47), (10, 40), (45, 5)] {
        let idx = px_idx(w, x, y);
        after[idx] = after[idx].wrapping_add(37);
        after[idx + 1] = 200;
        after[idx + 2] = after[idx + 2].wrapping_sub(90);
        after[idx + 3] = 255;
    }

    let diff = RasterDiff::from_rgba(&before, &after, [w, h], 16).expect("build diff");
    assert!(!diff.is_empty());

    // Forward on `before` yields `after`.
    let mut fwd = before.clone();
    let rects = diff
        .apply(&mut fwd, [w, h], ApplyDirection::Forward)
        .expect("forward apply");
    assert_eq!(fwd, after, "forward must reconstruct `after`");
    assert_eq!(rects.len(), diff.tile_count());

    // Reverse on `after` yields `before`.
    let mut rev = after.clone();
    diff.apply(&mut rev, [w, h], ApplyDirection::Reverse)
        .expect("reverse apply");
    assert_eq!(rev, before, "reverse must reconstruct `before`");
}

#[test]
fn no_change_is_empty() {
    let (w, h) = (32u32, 32u32);
    let before = random_rgba(w, h, 7);
    let after = before.clone();
    let diff = RasterDiff::from_rgba(&before, &after, [w, h], 16).expect("build diff");
    assert!(diff.is_empty());
    assert_eq!(diff.tile_count(), 0);
    assert_eq!(diff.compressed_len(), 0);

    // Applying an empty diff is a no-op and reports no dirty rects.
    let mut buf = before.clone();
    let rects = diff
        .apply(&mut buf, [w, h], ApplyDirection::Forward)
        .expect("apply empty");
    assert!(rects.is_empty());
    assert_eq!(buf, before);
}

#[test]
fn single_pixel_change_gives_tight_bbox() {
    let (w, h) = (256u32, 256u32);
    let before = random_rgba(w, h, 99);
    let mut after = before.clone();
    let (cx, cy) = (130u32, 90u32);
    let idx = px_idx(w, cx, cy);
    after[idx] = after[idx].wrapping_add(5);

    let diff = RasterDiff::from_rgba(&before, &after, [w, h], 32).expect("build diff");
    assert_eq!(diff.tile_count(), 1, "exactly one tile changed");
    let tile = &diff.tiles()[0];
    assert_eq!(tile.origin_px(), [cx, cy], "bbox origin at the changed pixel");
    assert_eq!(tile.size_px(), [1, 1], "bbox is a single pixel");

    // Round-trips.
    let mut rev = after.clone();
    let rects = diff
        .apply(&mut rev, [w, h], ApplyDirection::Reverse)
        .expect("reverse");
    assert_eq!(rev, before);
    assert_eq!(
        rects,
        vec![DirtyRect {
            origin_px: [cx, cy],
            size_px: [1, 1],
        }]
    );
}

#[test]
fn changes_in_two_separated_tiles_yield_two_diffs() {
    let (w, h) = (128u32, 128u32);
    let ts = 32u32;
    let before = random_rgba(w, h, 555);
    let mut after = before.clone();
    // Tile (0,0) and tile (3,3) — well separated so no other tile is touched.
    for (x, y) in [(5u32, 6u32), (100, 110)] {
        let idx = px_idx(w, x, y);
        after[idx + 2] = after[idx + 2].wrapping_add(64);
    }

    let diff = RasterDiff::from_rgba(&before, &after, [w, h], ts).expect("build diff");
    assert_eq!(diff.tile_count(), 2);
    let origins: Vec<[u32; 2]> = diff.tiles().iter().map(|t| t.origin_px()).collect();
    assert!(origins.contains(&[5, 6]));
    assert!(origins.contains(&[100, 110]));

    // Round-trip both directions.
    let mut fwd = before.clone();
    diff.apply(&mut fwd, [w, h], ApplyDirection::Forward)
        .expect("forward");
    assert_eq!(fwd, after);
}

#[test]
fn non_multiple_image_size_round_trips() {
    // 100x70 is not a multiple of tile_side 32 -> boundary tiles are clamped.
    let (w, h) = (100u32, 70u32);
    let before = random_rgba(w, h, 0xABCD);
    let mut after = before.clone();
    // Touch pixels in the clamped last column/row tiles too.
    for (x, y) in [(0u32, 0u32), (99, 69), (96, 64), (33, 40)] {
        let idx = px_idx(w, x, y);
        after[idx] = after[idx].wrapping_add(120);
        after[idx + 3] = after[idx + 3].wrapping_sub(15);
    }

    let diff = RasterDiff::from_rgba(&before, &after, [w, h], 32).expect("build diff");
    assert!(!diff.is_empty());

    let mut rev = after.clone();
    diff.apply(&mut rev, [w, h], ApplyDirection::Reverse)
        .expect("reverse");
    assert_eq!(rev, before);

    let mut fwd = before.clone();
    diff.apply(&mut fwd, [w, h], ApplyDirection::Forward)
        .expect("forward");
    assert_eq!(fwd, after);
}

#[test]
fn clamping_on_overflow_and_underflow() {
    // A 1x1 image whose delta is +200 on the red channel (before=0, after=200).
    let before = [0u8, 0, 0, 0];
    let after = [200u8, 0, 0, 0];
    let diff = RasterDiff::from_rgba(&before, &after, [1, 1], 8).expect("build diff");

    // Forward (+200) applied to a base of 200 overflows -> clamps to 255.
    let mut over = [200u8, 10, 20, 30];
    diff.apply(&mut over, [1, 1], ApplyDirection::Forward)
        .expect("forward");
    assert_eq!(over[0], 255, "over-255 clamps to 255");
    // Other channels have zero delta and are untouched.
    assert_eq!(&over[1..], &[10, 20, 30]);

    // Reverse (-200) applied to a base of 50 underflows -> clamps to 0.
    let mut under = [50u8, 0, 0, 0];
    diff.apply(&mut under, [1, 1], ApplyDirection::Reverse)
        .expect("reverse");
    assert_eq!(under[0], 0, "below-0 clamps to 0");
}

#[test]
fn wrong_buffer_length_on_build_errors() {
    let before = vec![0u8; 10 * 10 * 4];
    let short_after = vec![0u8; 10 * 10 * 4 - 4];
    let err = RasterDiff::from_rgba(&before, &short_after, [10, 10], 8).unwrap_err();
    assert_eq!(
        err,
        RasterDiffError::BufferLengthMismatch {
            expected: 10 * 10 * 4,
            got: 10 * 10 * 4 - 4,
        }
    );

    // Wrong `before` length is also rejected.
    let short_before = vec![0u8; 4];
    let err2 = RasterDiff::from_rgba(&short_before, &before, [10, 10], 8).unwrap_err();
    assert!(matches!(
        err2,
        RasterDiffError::BufferLengthMismatch { .. }
    ));
}

#[test]
fn zero_tile_side_errors() {
    let before = vec![0u8; 4];
    let after = vec![1u8; 4];
    let err = RasterDiff::from_rgba(&before, &after, [1, 1], 0).unwrap_err();
    assert_eq!(err, RasterDiffError::InvalidTileSide);
}

#[test]
fn apply_rejects_wrong_buffer_length_and_size() {
    let (w, h) = (16u32, 16u32);
    let before = random_rgba(w, h, 3);
    let mut after = before.clone();
    let idx = px_idx(w, 3, 3);
    after[idx] = after[idx].wrapping_add(10);
    let diff = RasterDiff::from_rgba(&before, &after, [w, h], 8).expect("build diff");

    // Wrong buffer length -> BufferLengthMismatch, no panic.
    let mut short = vec![0u8; (w as usize) * (h as usize) * 4 - 4];
    let err = diff
        .apply(&mut short, [w, h], ApplyDirection::Forward)
        .unwrap_err();
    assert!(matches!(err, RasterDiffError::BufferLengthMismatch { .. }));

    // Wrong image size -> ImageSizeMismatch, no panic.
    let mut buf = before.clone();
    let err2 = diff
        .apply(&mut buf, [w + 1, h], ApplyDirection::Forward)
        .unwrap_err();
    assert_eq!(
        err2,
        RasterDiffError::ImageSizeMismatch {
            diff: [w, h],
            target: [w + 1, h],
        }
    );
}

/// Copy the region rect `[rox,roy] + [rw,rh]` out of a full-image RGBA8 buffer
/// into a fresh region-local, row-major buffer.
fn extract_region(img: &[u8], iw: u32, rox: u32, roy: u32, rw: u32, rh: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((rw as usize) * (rh as usize) * 4);
    for y in 0..rh {
        for x in 0..rw {
            let idx = px_idx(iw, rox + x, roy + y);
            out.extend_from_slice(&img[idx..idx + 4]);
        }
    }
    out
}

#[test]
fn from_region_pixels_round_trips_against_full_image() {
    let (w, h) = (128u32, 96u32);
    let full_before = random_rgba(w, h, 0xBEEF);
    // Region well inside the image.
    let (rox, roy, rw, rh) = (40u32, 30u32, 32u32, 24u32);
    let region_before = extract_region(&full_before, w, rox, roy, rw, rh);

    // Change a scattered set of region-local pixels across several region tiles.
    let mut region_after = region_before.clone();
    for (lx, ly) in [(0u32, 0u32), (31, 23), (10, 5), (20, 18)] {
        let idx = px_idx(rw, lx, ly);
        region_after[idx] = region_after[idx].wrapping_add(50);
        region_after[idx + 1] = 123;
        region_after[idx + 2] = region_after[idx + 2].wrapping_sub(40);
        region_after[idx + 3] = 255;
    }

    // Reconstruct the full `after` image by writing the region back.
    let mut full_after = full_before.clone();
    for y in 0..rh {
        for x in 0..rw {
            let src = px_idx(rw, x, y);
            let dst = px_idx(w, rox + x, roy + y);
            full_after[dst..dst + 4].copy_from_slice(&region_after[src..src + 4]);
        }
    }

    let diff = RasterDiff::from_region_pixels(
        &region_before,
        &region_after,
        [rox, roy],
        [rw, rh],
        [w, h],
        16,
    )
    .expect("build region diff");
    assert!(!diff.is_empty());
    assert_eq!(diff.image_size(), [w, h]);
    // All stored tile origins must lie inside the region (image coordinates).
    for t in diff.tiles() {
        let [ox, oy] = t.origin_px();
        assert!(ox >= rox && ox < rox + rw, "tile origin x in region");
        assert!(oy >= roy && oy < roy + rh, "tile origin y in region");
    }

    // Forward on the full `before` reconstructs the full `after`.
    let mut fwd = full_before.clone();
    diff.apply(&mut fwd, [w, h], ApplyDirection::Forward)
        .expect("forward apply against full image");
    assert_eq!(fwd, full_after, "forward reconstructs full after");

    // Reverse on the full `after` reconstructs the full `before`.
    let mut rev = full_after.clone();
    diff.apply(&mut rev, [w, h], ApplyDirection::Reverse)
        .expect("reverse apply against full image");
    assert_eq!(rev, full_before, "reverse reconstructs full before");
}

#[test]
fn region_and_full_image_diffs_are_apply_equivalent() {
    // The same single-pixel change built two ways must apply identically, and the
    // region-built tile origin must be in image coordinates.
    let (w, h) = (200u32, 150u32);
    let full_before = random_rgba(w, h, 0x0FF1CE);
    let (cx, cy) = (137u32, 88u32);

    // Full-image `after`: bump one pixel.
    let mut full_after = full_before.clone();
    let idx = px_idx(w, cx, cy);
    full_after[idx + 2] = full_after[idx + 2].wrapping_add(77);

    // Region enclosing that pixel.
    let (rox, roy, rw, rh) = (128u32, 80u32, 16u32, 16u32);
    let region_before = extract_region(&full_before, w, rox, roy, rw, rh);
    let mut region_after = region_before.clone();
    let lidx = px_idx(rw, cx - rox, cy - roy);
    region_after[lidx + 2] = region_after[lidx + 2].wrapping_add(77);

    let full_diff = RasterDiff::from_rgba(&full_before, &full_after, [w, h], 32).expect("full diff");
    let region_diff = RasterDiff::from_region_pixels(
        &region_before,
        &region_after,
        [rox, roy],
        [rw, rh],
        [w, h],
        32,
    )
    .expect("region diff");

    // Both must be a single tile whose origin is the changed pixel in IMAGE
    // coordinates, sized 1x1.
    assert_eq!(full_diff.tile_count(), 1);
    assert_eq!(region_diff.tile_count(), 1);
    assert_eq!(full_diff.tiles()[0].origin_px(), [cx, cy]);
    assert_eq!(region_diff.tiles()[0].origin_px(), [cx, cy]);
    assert_eq!(region_diff.tiles()[0].size_px(), [1, 1]);

    // Applying either diff to the same base yields the same result.
    let mut from_full = full_before.clone();
    full_diff
        .apply(&mut from_full, [w, h], ApplyDirection::Forward)
        .expect("apply full");
    let mut from_region = full_before.clone();
    region_diff
        .apply(&mut from_region, [w, h], ApplyDirection::Forward)
        .expect("apply region");
    assert_eq!(from_full, from_region, "both diffs apply to the same image");
    assert_eq!(from_region, full_after);
}

#[test]
fn region_not_fitting_in_image_errors() {
    let (w, h) = (64u32, 64u32);
    // Region origin + size exceeds the image on the x axis.
    let (rox, roy, rw, rh) = (60u32, 10u32, 16u32, 16u32);
    let region = vec![0u8; (rw as usize) * (rh as usize) * 4];
    let err =
        RasterDiff::from_region_pixels(&region, &region, [rox, roy], [rw, rh], [w, h], 8).unwrap_err();
    assert_eq!(err, RasterDiffError::TileOutOfBounds, "must not panic; reports OOB");
}

#[test]
fn region_wrong_buffer_length_errors() {
    let (w, h) = (64u32, 64u32);
    let (rox, roy, rw, rh) = (0u32, 0u32, 16u32, 16u32);
    let good = vec![0u8; (rw as usize) * (rh as usize) * 4];
    let short = vec![0u8; (rw as usize) * (rh as usize) * 4 - 4];
    let err =
        RasterDiff::from_region_pixels(&good, &short, [rox, roy], [rw, rh], [w, h], 8).unwrap_err();
    assert_eq!(
        err,
        RasterDiffError::BufferLengthMismatch {
            expected: (rw as usize) * (rh as usize) * 4,
            got: (rw as usize) * (rh as usize) * 4 - 4,
        }
    );
}

#[test]
fn region_with_no_change_is_empty() {
    let (w, h) = (48u32, 48u32);
    let (rox, roy, rw, rh) = (8u32, 8u32, 16u32, 16u32);
    let region = vec![7u8; (rw as usize) * (rh as usize) * 4];
    let diff = RasterDiff::from_region_pixels(&region, &region, [rox, roy], [rw, rh], [w, h], 8)
        .expect("build region diff");
    assert!(diff.is_empty());
    assert_eq!(diff.tile_count(), 0);
}

#[test]
fn compression_shrinks_a_realistic_changed_region() {
    // A large image with a big block changed by a constant per-channel delta:
    // the repetitive signed-delta stream compresses well below its raw size.
    let (w, h) = (256u32, 256u32);
    let before = vec![0u8; (w as usize) * (h as usize) * 4];
    let mut after = before.clone();
    for y in 20..180u32 {
        for x in 20..180u32 {
            let idx = px_idx(w, x, y);
            after[idx] = 10;
            after[idx + 1] = 10;
            after[idx + 2] = 10;
            after[idx + 3] = 255;
        }
    }

    let diff = RasterDiff::from_rgba(&before, &after, [w, h], 64).expect("build diff");
    assert!(!diff.is_empty());
    let compressed = diff.compressed_len();
    assert!(compressed > 0, "a changed region must produce payload bytes");

    // Uncompressed size = sum over tiles of bbox_w * bbox_h * 8.
    let uncompressed: usize = diff
        .tiles()
        .iter()
        .map(|t| {
            (t.size_px()[0] as usize) * (t.size_px()[1] as usize) * 8
        })
        .sum();
    assert!(
        compressed < uncompressed,
        "compressed ({compressed}) must be smaller than uncompressed ({uncompressed})"
    );

    // And it still round-trips.
    let mut rev = after.clone();
    diff.apply(&mut rev, [w, h], ApplyDirection::Reverse)
        .expect("reverse");
    assert_eq!(rev, before);
}
