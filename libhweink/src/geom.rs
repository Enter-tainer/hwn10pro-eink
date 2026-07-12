//! Geometry: the 90°-clockwise buffer→screen coordinate transform.
//!
//! The native EBC buffer is landscape **2480×1860** (width × height). The physical
//! panel is portrait **1860×2480**. The TCON rotates the buffer **90° clockwise**
//! when scanning out to the panel, so:
//!
//! ```text
//! buffer(x, y)   →  screen(sx, sy) = (BUF_H - 1 - y, x)      // BUF_H = 1860
//! screen(sx, sy) →  buffer(x, y)  = (sy, BUF_H - 1 - sx)
//! ```
//!
//! Edge map: buffer top (y=0) → screen right, buffer bottom (y=1859) → screen left,
//! buffer left (x=0) → screen top, buffer right (x=2479) → screen bottom.
//!
//! This was confirmed on a clean 8bpp `compass` image (the earlier 270° CW conclusion
//! was inferred from a stride-folded image and is void). See `docs/OVERVIEW.md` §2.3.

/// Native (landscape) buffer dimensions on this device: 2480 wide × 1860 tall.
pub const BUF_W: i32 = 2480;
pub const BUF_H: i32 = 1860;

/// Screen (portrait) dimensions the user actually sees: 1860 wide × 2480 tall.
pub const SCREEN_W: i32 = 1860;
pub const SCREEN_H: i32 = 2480;

/// Convert a screen (portrait) coordinate to a native buffer (landscape) coordinate.
#[inline]
pub const fn screen_to_buf(sx: i32, sy: i32) -> (i32, i32) {
    // x = sy, y = BUF_H - 1 - sx
    (sy, BUF_H - 1 - sx)
}

/// Convert a native buffer (landscape) coordinate to a screen (portrait) coordinate.
#[inline]
pub const fn buf_to_screen(x: i32, y: i32) -> (i32, i32) {
    // sx = BUF_H - 1 - y, sy = x
    (BUF_H - 1 - y, x)
}

/// A screen-space (portrait) rectangle. Half-open `[x1, x2) × [y1, y2)`.
///
/// `screen_rect_to_buf` converts it to the native buffer rectangle. Because the 90° CW
/// rotation swaps axes and flips one, the resulting buffer rect may have its corners
/// reordered; we normalize so that `x1 <= x2` and `y1 <= y2`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScreenRect {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl ScreenRect {
    pub const fn new(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        ScreenRect { x1, y1, x2, y2 }
    }
    pub const fn full() -> Self {
        ScreenRect { x1: 0, y1: 0, x2: SCREEN_W, y2: SCREEN_H }
    }
    /// Convert to a native buffer rectangle (normalized, half-open).
    pub fn to_buf(&self) -> crate::ioctl::Rect {
        // Each screen corner (sx, sy) → buffer (sy, BUF_H-1-sx).
        // Map all four corners, then take min/max.
        let (a, b) = screen_to_buf(self.x1, self.y1);
        let (c, d) = screen_to_buf(self.x2, self.y2);
        let bx1 = a.min(c);
        let bx2 = a.max(c);
        let by1 = b.min(d);
        let by2 = b.max(d);
        crate::ioctl::Rect::new(bx1, by1, bx2, by2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_rotate_90cw() {
        // buffer top-left (0,0) → screen top-right (1859, 0)
        assert_eq!(buf_to_screen(0, 0), (BUF_H - 1, 0));
        // buffer top-right (2479, 0) → screen bottom-right (1859, 2479)
        assert_eq!(buf_to_screen(BUF_W - 1, 0), (BUF_H - 1, BUF_W - 1));
        // buffer bottom-left (0, 1859) → screen top-left (0, 0)
        assert_eq!(buf_to_screen(0, BUF_H - 1), (0, 0));
        // buffer bottom-right (2479, 1859) → screen bottom-left (0, 2479)
        assert_eq!(buf_to_screen(BUF_W - 1, BUF_H - 1), (0, BUF_W - 1));
    }

    #[test]
    fn screen_to_buf_roundtrip() {
        for &(sx, sy) in &[(0, 0), (100, 200), (1859, 2479), (929, 1240)] {
            let (x, y) = screen_to_buf(sx, sy);
            assert_eq!(buf_to_screen(x, y), (sx, sy));
        }
    }

    #[test]
    fn full_screen_rect_maps_to_full_buffer() {
        let r = ScreenRect::full().to_buf();
        assert_eq!(r.x1, 0);
        assert_eq!(r.y1, 0);
        assert_eq!(r.x2, BUF_W);
        assert_eq!(r.y2, BUF_H);
    }
}
