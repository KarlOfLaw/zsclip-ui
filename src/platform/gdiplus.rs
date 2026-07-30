use std::ffi::c_void;
use std::sync::OnceLock;

use crate::platform::string::to_wide;

#[repr(C)]
struct GdiplusStartupInput {
    gdiplus_version: u32,
    debug_event_callback: *const c_void,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

#[repr(C)]
struct GdipRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[repr(C)]
struct GdipBitmapData {
    width: u32,
    height: u32,
    stride: i32,
    pixel_format: i32,
    scan0: *mut c_void,
    reserved: usize,
}

const SMOOTHING_MODE_ANTI_ALIAS: i32 = 4;
const UNIT_PIXEL: i32 = 2;
const FILL_MODE_ALTERNATE: i32 = 0;
const IMAGE_LOCK_MODE_READ: u32 = 1;
const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;
const MAX_THUMBNAIL_SOURCE_PIXELS: u64 = 80_000_000;

#[link(name = "gdiplus")]
unsafe extern "system" {
    fn GdiplusStartup(
        token: *mut usize,
        input: *const GdiplusStartupInput,
        output: *mut c_void,
    ) -> i32;
    fn GdipCreateFromHDC(hdc: *mut c_void, graphics: *mut *mut c_void) -> i32;
    fn GdipDeleteGraphics(graphics: *mut c_void) -> i32;
    fn GdipSetSmoothingMode(graphics: *mut c_void, smoothing_mode: i32) -> i32;
    fn GdipCreateSolidFill(color: u32, brush: *mut *mut c_void) -> i32;
    fn GdipDeleteBrush(brush: *mut c_void) -> i32;
    fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut *mut c_void) -> i32;
    fn GdipDeletePen(pen: *mut c_void) -> i32;
    fn GdipCreatePath(fill_mode: i32, path: *mut *mut c_void) -> i32;
    fn GdipDeletePath(path: *mut c_void) -> i32;
    fn GdipAddPathArcI(
        path: *mut c_void,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        start_angle: f32,
        sweep_angle: f32,
    ) -> i32;
    fn GdipClosePathFigure(path: *mut c_void) -> i32;
    fn GdipFillPath(graphics: *mut c_void, brush: *mut c_void, path: *mut c_void) -> i32;
    fn GdipDrawPath(graphics: *mut c_void, pen: *mut c_void, path: *mut c_void) -> i32;
    fn GdipLoadImageFromFile(filename: *const u16, image: *mut *mut c_void) -> i32;
    fn GdipDisposeImage(image: *mut c_void) -> i32;
    fn GdipGetImageWidth(image: *mut c_void, width: *mut u32) -> i32;
    fn GdipGetImageHeight(image: *mut c_void, height: *mut u32) -> i32;
    fn GdipGetImageThumbnail(
        image: *mut c_void,
        thumb_width: u32,
        thumb_height: u32,
        thumb_image: *mut *mut c_void,
        callback: *mut c_void,
        callback_data: *mut c_void,
    ) -> i32;
    fn GdipBitmapLockBits(
        bitmap: *mut c_void,
        rect: *const GdipRect,
        flags: u32,
        format: i32,
        locked_bitmap_data: *mut GdipBitmapData,
    ) -> i32;
    fn GdipBitmapUnlockBits(bitmap: *mut c_void, locked_bitmap_data: *mut GdipBitmapData) -> i32;
}

static GDIP_TOKEN: OnceLock<Option<usize>> = OnceLock::new();

fn ensure_startup() -> Option<usize> {
    *GDIP_TOKEN.get_or_init(|| unsafe {
        let mut token = 0usize;
        let input = GdiplusStartupInput {
            gdiplus_version: 1,
            debug_event_callback: std::ptr::null(),
            suppress_background_thread: 0,
            suppress_external_codecs: 0,
        };
        let ok = GdiplusStartup(&mut token, &input, std::ptr::null_mut()) == 0;
        if ok {
            Some(token)
        } else {
            None
        }
    })
}

unsafe fn locked_bitmap_rgba(
    bitmap: *mut c_void,
    width: usize,
    height: usize,
) -> Option<Vec<u8>> {
    let rect = GdipRect {
        x: 0,
        y: 0,
        width: i32::try_from(width).ok()?,
        height: i32::try_from(height).ok()?,
    };
    let mut locked: GdipBitmapData = core::mem::zeroed();
    if GdipBitmapLockBits(
        bitmap,
        &rect,
        IMAGE_LOCK_MODE_READ,
        PIXEL_FORMAT_32BPP_ARGB,
        &mut locked,
    ) != 0
        || locked.scan0.is_null()
    {
        return None;
    }

    let row_bytes = width.checked_mul(4)?;
    let stride = locked.stride as isize;
    let stride_bytes = stride.unsigned_abs();
    let mut rgba = if stride_bytes < row_bytes {
        None
    } else {
        let output_len = row_bytes.checked_mul(height)?;
        let mut output = vec![0u8; output_len];
        for y in 0..height {
            let source = core::slice::from_raw_parts(
                (locked.scan0 as *const u8).offset(stride * y as isize),
                row_bytes,
            );
            let destination = &mut output[y * row_bytes..(y + 1) * row_bytes];
            for (bgra, rgba) in source
                .chunks_exact(4)
                .zip(destination.chunks_exact_mut(4))
            {
                rgba[0] = bgra[2];
                rgba[1] = bgra[1];
                rgba[2] = bgra[0];
                rgba[3] = bgra[3];
            }
        }
        Some(output)
    };
    let _ = GdipBitmapUnlockBits(bitmap, &mut locked);

    if let Some(bytes) = rgba.as_mut() {
        if bytes.chunks_exact(4).all(|pixel| pixel[3] == 0) {
            for pixel in bytes.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
    }
    rgba
}

pub(crate) fn load_image_thumbnail_rgba(
    path: &str,
    max_side: usize,
) -> Option<(Vec<u8>, usize, usize)> {
    if path.trim().is_empty() || max_side == 0 || ensure_startup().is_none() {
        return None;
    }

    let wide_path = to_wide(path);
    unsafe {
        let mut source = core::ptr::null_mut();
        if GdipLoadImageFromFile(wide_path.as_ptr(), &mut source) != 0 || source.is_null() {
            return None;
        }

        let result = (|| {
            let mut source_width = 0u32;
            let mut source_height = 0u32;
            if GdipGetImageWidth(source, &mut source_width) != 0
                || GdipGetImageHeight(source, &mut source_height) != 0
                || source_width == 0
                || source_height == 0
                || u64::from(source_width) * u64::from(source_height)
                    > MAX_THUMBNAIL_SOURCE_PIXELS
            {
                return None;
            }

            let longest = source_width.max(source_height);
            let requested = u32::try_from(max_side).ok()?.min(longest);
            let (thumb_width, thumb_height) = if source_width >= source_height {
                (
                    requested,
                    ((u64::from(source_height) * u64::from(requested)
                        + u64::from(source_width) / 2)
                        / u64::from(source_width))
                    .max(1) as u32,
                )
            } else {
                (
                    ((u64::from(source_width) * u64::from(requested)
                        + u64::from(source_height) / 2)
                        / u64::from(source_height))
                    .max(1) as u32,
                    requested,
                )
            };

            let mut thumbnail = core::ptr::null_mut();
            if GdipGetImageThumbnail(
                source,
                thumb_width,
                thumb_height,
                &mut thumbnail,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            ) != 0
                || thumbnail.is_null()
            {
                return None;
            }
            let rgba =
                locked_bitmap_rgba(thumbnail, thumb_width as usize, thumb_height as usize);
            let _ = GdipDisposeImage(thumbnail);
            rgba.map(|bytes| (bytes, thumb_width as usize, thumb_height as usize))
        })();

        let _ = GdipDisposeImage(source);
        result
    }
}

#[inline]
fn colorref_to_argb(color: u32) -> u32 {
    let r = color & 0xFF;
    let g = (color >> 8) & 0xFF;
    let b = (color >> 16) & 0xFF;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

unsafe fn build_round_path(
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    radius: i32,
) -> *mut c_void {
    let mut path = std::ptr::null_mut();
    if GdipCreatePath(FILL_MODE_ALTERNATE, &mut path) != 0 || path.is_null() {
        return std::ptr::null_mut();
    }
    let w = right - left;
    let h = bottom - top;
    let r = radius.min(w / 2).min(h / 2).max(1);
    let d = r * 2;
    let ok = GdipAddPathArcI(path, left, top, d, d, 180.0, 90.0) == 0
        && GdipAddPathArcI(path, right - d, top, d, d, 270.0, 90.0) == 0
        && GdipAddPathArcI(path, right - d, bottom - d, d, d, 0.0, 90.0) == 0
        && GdipAddPathArcI(path, left, bottom - d, d, d, 90.0, 90.0) == 0
        && GdipClosePathFigure(path) == 0;
    if !ok {
        let _ = GdipDeletePath(path);
        return std::ptr::null_mut();
    }
    path
}

pub unsafe fn draw_round_rect(
    hdc: *mut c_void,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    fill: u32,
    border: u32,
    radius: i32,
) -> bool {
    if ensure_startup().is_none() {
        return false;
    }
    if right <= left || bottom <= top {
        return true;
    }
    let mut graphics = std::ptr::null_mut();
    if GdipCreateFromHDC(hdc, &mut graphics) != 0 || graphics.is_null() {
        return false;
    }
    let _ = GdipSetSmoothingMode(graphics, SMOOTHING_MODE_ANTI_ALIAS);
    let path = build_round_path(left, top, right, bottom, radius.max(1));
    if path.is_null() {
        let _ = GdipDeleteGraphics(graphics);
        return false;
    }
    let mut ok = true;
    let mut brush = std::ptr::null_mut();
    if GdipCreateSolidFill(colorref_to_argb(fill), &mut brush) == 0 && !brush.is_null() {
        ok &= GdipFillPath(graphics, brush, path) == 0;
        let _ = GdipDeleteBrush(brush);
    } else {
        ok = false;
    }
    if border != 0 && border != fill {
        let mut pen = std::ptr::null_mut();
        if GdipCreatePen1(colorref_to_argb(border), 1.0, UNIT_PIXEL, &mut pen) == 0
            && !pen.is_null()
        {
            ok &= GdipDrawPath(graphics, pen, path) == 0;
            let _ = GdipDeletePen(pen);
        }
    }
    let _ = GdipDeletePath(path);
    let _ = GdipDeleteGraphics(graphics);
    ok
}
