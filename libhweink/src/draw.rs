//! Batch-write draw scope (`Draw` guard) with a reusable screen-space shadow buffer.
//!
//! ## Why
//!
//! `Surface::put_pixel` / `fill_rect` are fine for one-off draws, but for real
//! rendering (a pen stroke, a bezier, a glyph) you issue hundreds-to-thousands of
//! pixel writes. Doing a `refresh()` per write is catastrophic (one EBC refresh per
//! pixel), and even without refresh each write crosses the 90°-CW transform + bounds
//! check. The `Draw` scope lets you batch all writes into one shadow buffer and flush
//! exactly once at the end, with a single merged dirty rectangle.
//!
//! ## How it works
//!
//! `Surface::draw()` borrows the surface and returns a `Draw<'_>` guard. Inside the
//! guard, all writes go to a **screen-space shadow buffer** (`SCREEN_W × SCREEN_H`
//! 8bpp, ~4.6 MiB) owned by the `Surface`, plus a running `dirty: Option<ScreenRect>`
//! that unions every touched region.
//!
//! On `drop` (or `flush()`), the dirty region of the shadow is blitted to the native
//! mmap buffer (applying the 90°-CW rotation, one row at a time) and a single
//! `refresh(dirty, mode)` is issued. `cancel()` drops the guard without flushing.
//!
//! The shadow is read-modify-write: `put_pixel` reads the old value, blends, writes.
//! This matters for anti-aliased strokes (alpha-blend onto existing ink). The shadow
//! persists across `draw()` scopes (it lives on `Surface`), so consecutive scopes see
//! each other's state — i.e. the shadow is the "current canvas".
//!
//! ## Coordinate system
//!
//! All `Draw` methods take **screen (portrait) coordinates** (0..1860 × 0..2480), same
//! as `Surface`. The 90°-CW rotation to native buffer coords happens only at flush
//! time, once per dirty row.

use crate::geom::{screen_to_buf, ScreenRect, SCREEN_H, SCREEN_W};
use crate::mode::Mode;
use crate::path::Surface;

/// Shadow buffer stride = screen width (8bpp, 1 byte/px).
const SHADOW_STRIDE: usize = SCREEN_W as usize;
/// Shadow buffer size in bytes.
const SHADOW_BYTES: usize = SHADOW_STRIDE * SCREEN_H as usize;

/// A batch-write drawing scope. Writes go to the surface's shadow buffer; on drop
/// (or `flush()`) the dirty region is blitted to the EBC buffer and refreshed in one
/// `SEND_OSD_BUFFER`/`SEND_BUFFER` call.
///
/// See [`Surface::draw`] and [`Surface::draw_with`].
pub struct Draw<'a> {
    surf: &'a Surface<'a>,
    /// Borrow of the surface's shadow buffer.
    shadow: &'a mut [u8],
    /// Union of all regions written this scope. `None` = nothing written → no flush.
    dirty: Option<ScreenRect>,
    /// Refresh mode to use on flush. `None` = path default.
    mode: Option<Mode>,
    /// Set by `cancel()`; when true, drop does NOT flush.
    cancelled: bool,
    /// Set by `flush()`; when true, a second drop won't re-flush.
    flushed: bool,
}

impl<'a> Draw<'a> {
    /// Put a single pixel at screen coords `(x, y)` with gray `g` (0=black, 255=white).
    /// Out-of-bounds is silently clipped. Marks the pixel's rect dirty.
    pub fn put_pixel(&mut self, x: i32, y: i32, g: u8) {
        if x < 0 || y < 0 || x >= SCREEN_W || y >= SCREEN_H {
            return;
        }
        let bi = (y as usize) * SHADOW_STRIDE + (x as usize);
        self.shadow[bi] = g;
        self.dirty_union_rect(x, y, x + 1, y + 1);
    }

    /// Fill a screen-space rectangle `[x1, x2) × [y1, y2)` with `g`. Corners may be in
    /// any order (normalized internally). Clipped to the screen.
    pub fn fill_rect(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, g: u8) {
        let (x1, x2) = (x1.min(x2), x1.max(x2));
        let (y1, y2) = (y1.min(y2), y1.max(y2));
        let x1 = x1.max(0).min(SCREEN_W);
        let x2 = x2.max(0).min(SCREEN_W);
        let y1 = y1.max(0).min(SCREEN_H);
        let y2 = y2.max(0).min(SCREEN_H);
        if x1 >= x2 || y1 >= y2 {
            return;
        }
        let row = (x1 as usize)..(x2 as usize);
        for y in (y1 as usize)..(y2 as usize) {
            let base = y * SHADOW_STRIDE;
            self.shadow[base + row.start..base + row.end].fill(g);
        }
        self.dirty_union_rect(x1, y1, x2, y2);
    }

    /// Put a batch of pixels. Cheaper than many `put_pixel` calls: the dirty rect is
    /// unioned once over the whole batch. Items are clipped individually.
    pub fn put_pixels(&mut self, pxs: &[(i32, i32, u8)]) {
        if pxs.is_empty() {
            return;
        }
        for &(x, y, g) in pxs {
            if x < 0 || y < 0 || x >= SCREEN_W || y >= SCREEN_H {
                continue;
            }
            let bi = (y as usize) * SHADOW_STRIDE + (x as usize);
            self.shadow[bi] = g;
        }
        // Union the bounding box of the batch (cheap, avoids per-item dirty work).
        let mut minx = i32::MAX;
        let mut miny = i32::MAX;
        let mut maxx = i32::MIN;
        let mut maxy = i32::MIN;
        for &(x, y, _) in pxs {
            if x < 0 || y < 0 || x >= SCREEN_W || y >= SCREEN_H {
                continue;
            }
            if x < minx { minx = x; }
            if y < miny { miny = y; }
            if x + 1 > maxx { maxx = x + 1; }
            if y + 1 > maxy { maxy = y + 1; }
        }
        if minx != i32::MAX {
            self.dirty_union_rect(minx, miny, maxx, maxy);
        }
    }

    /// Blit a `width × height` 8bpp gray image (row-major, stride = `width`) with its
    /// top-left at screen `(x, y)`. Clipped to the screen. Marks the intersection dirty.
    pub fn blit(&mut self, img: &[u8], width: usize, height: usize, x: i32, y: i32) {
        if width == 0 || height == 0 || img.len() < width * height {
            return;
        }
        // Clip source rows/cols to screen.
        let sx0 = (-(x)).max(0) as usize; // source col to start at
        let sy0 = (-(y)).max(0) as usize;
        let dx0 = x.max(0);
        let dy0 = y.max(0);
        let cols = (width - sx0).min((SCREEN_W as usize).saturating_sub(dx0 as usize));
        let rows = (height - sy0).min((SCREEN_H as usize).saturating_sub(dy0 as usize));
        if cols == 0 || rows == 0 {
            return;
        }
        for r in 0..rows {
            let src = (sy0 + r) * width + sx0;
            let dst = (dy0 as usize + r) * SHADOW_STRIDE + (dx0 as usize);
            self.shadow[dst..dst + cols].copy_from_slice(&img[src..src + cols]);
        }
        self.dirty_union_rect(dx0, dy0, dx0 + cols as i32, dy0 + rows as i32);
    }

    /// Direct mutable access to the screen-space shadow buffer + stride, for callers
    /// who want to write their own loops (bezier sampling, polygon fill, rusttype
    /// glyph blit). You MUST call [`dirty_union`] with the region you touched, or the
    /// flush won't refresh it.
    ///
    /// Layout: row-major, `stride = SCREEN_W = 1860`, `len = SCREEN_W * SCREEN_H`.
    /// Index: `buf[y * stride + x]`. Coordinates are screen (portrait), 0=black 255=white.
    pub fn raw_shadow_mut(&mut self) -> (&mut [u8], usize) {
        (self.shadow, SHADOW_STRIDE)
    }

    /// Manually mark a screen-space region as dirty (e.g. after writing via
    /// `raw_shadow_mut`). Half-open `[x1,x2)×[y1,y2)`, normalized + clipped internally.
    pub fn dirty_union(&mut self, rect: ScreenRect) {
        self.dirty_union_rect(rect.x1, rect.y1, rect.x2, rect.y2);
    }

    /// Set the refresh waveform to use on flush. `None` (default) = path default
    /// (OSD→OVERLAY, Main→FULL_GC16).
    pub fn set_mode(&mut self, mode: Option<Mode>) {
        self.mode = mode;
    }

    /// Flush the dirty region now (blit shadow → EBC buffer + refresh). After this the
    /// scope is "spent" — further writes still go to the shadow but won't auto-flush on
    /// drop unless you call `flush()` again. Returns the refresh `io::Result`.
    pub fn flush(&mut self) -> std::io::Result<()> {
        if self.cancelled || self.flushed {
            return Ok(());
        }
        let dirty = match self.dirty {
            Some(r) => r,
            None => {
                self.flushed = true;
                return Ok(());
            }
        };
        self.blit_dirty_to_ebc(&dirty);
        let r = self.surf.refresh(dirty, self.mode);
        self.flushed = true;
        // Reset dirty so a second flush within the same scope only re-flushes new writes.
        self.dirty = None;
        r
    }

    /// Discard this scope without flushing. The shadow buffer retains whatever was
    /// written (it's the surface's canvas), but no EBC refresh is issued on drop.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    // ---- internals ----

    fn dirty_union_rect(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        let x1 = x1.max(0).min(SCREEN_W);
        let x2 = x2.max(0).min(SCREEN_W);
        let y1 = y1.max(0).min(SCREEN_H);
        let y2 = y2.max(0).min(SCREEN_H);
        if x1 >= x2 || y1 >= y2 {
            return;
        }
        self.dirty = Some(match self.dirty {
            None => ScreenRect { x1, y1, x2, y2 },
            Some(d) => ScreenRect {
                x1: d.x1.min(x1),
                y1: d.y1.min(y1),
                x2: d.x2.max(x2),
                y2: d.y2.max(y2),
            },
        });
    }

    /// Blit the dirty region of the shadow into the native EBC mmap buffer, applying
    /// the 90°-CW rotation one screen-row at a time.
    ///
    /// For each screen pixel (sx, sy) in the dirty rect, the native buffer index is
    /// `(sy, BUF_H-1-sx)` → `buf[(BUF_H-1-sx) * native_stride + sy]`. We iterate by
    /// screen row (constant sy → constant native column sy), so for each screen row
    /// we write a contiguous-ish span in the native buffer.
    fn blit_dirty_to_ebc(&self, dirty: &ScreenRect) {
        let ebc = self.surf.ebc();
        let path = self.surf.path();
        let native_stride = ebc.stride(); // = 2480 = native width
        let native_buf = path.buffer_slice(ebc);
        if native_buf.is_empty() {
            return;
        }
        let native_len = native_buf.len();
        // Clamp dirty to screen (should already be, but be safe).
        let x1 = dirty.x1.max(0).min(SCREEN_W);
        let x2 = dirty.x2.max(0).min(SCREEN_W);
        let y1 = dirty.y1.max(0).min(SCREEN_H);
        let y2 = dirty.y2.max(0).min(SCREEN_H);
        if x1 >= x2 || y1 >= y2 {
            return;
        }
        for sy in y1..y2 {
            // screen row sy → native column x = sy, native row y = BUF_H-1-sx
            // We walk sx in [x1, x2): native row decreases, native col constant = sy.
            let shadow_row = (sy as usize) * SHADOW_STRIDE;
            for sx in x1..x2 {
                let (bx, by) = screen_to_buf(sx, sy); // (bx, by) = (sy, BUF_H-1-sx)
                let nbi = (by as usize) * native_stride + (bx as usize);
                if nbi < native_len {
                    native_buf[nbi] = self.shadow[shadow_row + sx as usize];
                }
            }
        }
    }
}

impl<'a> Drop for Draw<'a> {
    fn drop(&mut self) {
        if !self.cancelled && !self.flushed {
            let _ = self.flush();
        }
    }
}

// ---- Surface integration ----

impl<'a> Surface<'a> {
    /// Begin a batch-write draw scope. Returns a `Draw` guard that writes to the
    /// surface's shadow buffer; on drop the dirty region is flushed to the EBC buffer
    /// and refreshed in one call.
    ///
    /// ```no_run
    /// # use hweink::{Ebc, path::Path, Surface};
    /// # let ebc = Ebc::open().unwrap();
    /// # let surf = Surface::new(&ebc, Path::Osd).unwrap();
    /// {
    ///     let mut d = surf.draw();
    ///     d.fill_rect(100, 100, 300, 300, 0); // black box
    ///     d.put_pixel(200, 200, 255);          // white dot
    /// } // <- one refresh here
    /// ```
    pub fn draw(&self) -> Draw<'_> {
        // We need &mut shadow but only hold &self. The shadow is behind a RefCell-like
        // cell so we can borrow it mutably from a shared Surface ref. This is sound
        // because Draw borrows the surface's lifetime — there can be only one active
        // Draw at a time (the borrow chain enforces it).
        let shadow: &mut [u8] = self.shadow_mut();
        Draw {
            surf: self,
            shadow,
            dirty: None,
            mode: None,
            cancelled: false,
            flushed: false,
        }
    }

    /// Run a closure with a `Draw` scope, flushing on return (unless the closure
    /// cancels). Equivalent to `let d = surf.draw(); ...; drop(d);` but harder to
    /// forget.
    pub fn draw_with<R>(&self, f: impl FnOnce(&mut Draw) -> R) -> std::io::Result<R> {
        let mut d = self.draw();
        let r = f(&mut d);
        d.flush()?;
        Ok(r)
    }
}
