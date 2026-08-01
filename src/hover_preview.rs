use std::io::Read;
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::PAINTSTRUCT,
    UI::WindowsAndMessaging::*,
};

use crate::{
    app::{ensure_item_image_bytes, rich_text_preview_text, ClipItem, ClipKind},
    i18n::tr,
    platform::{
        appearance as platform_appearance, gdi as platform_gdi, monitor as platform_monitor,
        string::to_wide,
        window::{self as platform_window, post_boxed_message},
    },
    ui::{draw_round_rect, draw_text_block, draw_text_ex, rgba_to_opaque_bgra_on_bg},
    win_native_style::Theme,
};

const HOVER_PREVIEW_CLASS: &str = "ZsClipHoverPreview";
const PREVIEW_W_TEXT: i32 = 420;
const PREVIEW_H_TEXT: i32 = 220;
const PREVIEW_W_IMAGE: i32 = 520;
const PREVIEW_H_IMAGE: i32 = 360;
// 392x164 的正文区使用 12px 字体时约有 10 行空间，正文保留 9 行以容纳截断提示。
const PREVIEW_TEXT_MAX_LINES: usize = 9;
const PREVIEW_TEXT_MAX_CHARS: usize = 420;
const PREVIEW_FILE_MAX_ITEMS: usize = 8;
const MARKDOWN_PREVIEW_MAX_BYTES: u64 = 32 * 1024;
const WM_HOVER_IMAGE_READY: u32 = WM_APP + 41;

struct HoverPreviewImageResult {
    item_id: i64,
    image: Option<(Vec<u8>, usize, usize)>,
}

struct HoverPreviewData {
    item_id: i64,
    header: String,
    body: String,
    image: Option<(Vec<u8>, usize, usize)>,
    image_width: usize,
    image_height: usize,
    loading_item_id: i64,
    last_x: i32,
    last_y: i32,
    last_w: i32,
    last_h: i32,
    zoom_mode: bool,
}

static HOVER_HWND: OnceLock<isize> = OnceLock::new();

unsafe extern "system" fn preview_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lparam as *const CREATESTRUCTW);
            platform_window::set_user_data(hwnd, cs.lpCreateParams as isize);
            platform_appearance::set_rounded_corners(hwnd);
            1
        }
        WM_PAINT => {
            let ptr = platform_window::user_data(hwnd) as *mut HoverPreviewData;
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = platform_gdi::begin_paint(hwnd, &mut ps);
            if !hdc.is_null() && !ptr.is_null() {
                let th = Theme::default();
                let data = &*ptr;
                let rc = platform_window::client_rect(hwnd).unwrap_or_else(|| zeroed());
                let bg = platform_gdi::create_solid_brush(th.surface);
                platform_gdi::fill_rect(hdc, &rc, bg);
                platform_gdi::delete_object(bg as _);
                draw_round_rect(hdc as _, &rc, th.surface, th.stroke, 10);

                let header_rc = RECT {
                    left: 14,
                    top: 10,
                    right: rc.right - 14,
                    bottom: 34,
                };
                draw_text_ex(
                    hdc as _,
                    &data.header,
                    &header_rc,
                    th.text_muted,
                    12,
                    true,
                    false,
                    "Segoe UI Variable Text",
                );

                if let Some((bytes, width, height)) = &data.image {
                    let bgra = rgba_to_opaque_bgra_on_bg(bytes, th.surface);
                    let content = RECT {
                        left: 12,
                        top: 40,
                        right: rc.right - 12,
                        bottom: rc.bottom - 12,
                    };
                    let avail_w = (content.right - content.left).max(1);
                    let avail_h = (content.bottom - content.top).max(1);
                    let scale = (avail_w as f32 / *width as f32)
                        .min(avail_h as f32 / *height as f32)
                        .min(1.0);
                    let dw = ((*width as f32) * scale).max(1.0) as i32;
                    let dh = ((*height as f32) * scale).max(1.0) as i32;
                    let dx = content.left + (avail_w - dw) / 2;
                    let dy = content.top + (avail_h - dh) / 2;

                    platform_gdi::stretch_top_down_32bpp(
                        hdc,
                        dx,
                        dy,
                        dw,
                        dh,
                        *width as i32,
                        *height as i32,
                        &bgra,
                    );
                } else if !data.body.is_empty() {
                    let body_rc = RECT {
                        left: 14,
                        top: 42,
                        right: rc.right - 14,
                        bottom: rc.bottom - 14,
                    };
                    draw_text_block(hdc as _, &data.body, &body_rc, th.text, 12, false);
                } else {
                    let body_rc = RECT {
                        left: 14,
                        top: 42,
                        right: rc.right - 14,
                        bottom: rc.bottom - 14,
                    };
                    draw_text_block(
                        hdc as _,
                        tr("正在加载预览…", "Loading preview..."),
                        &body_rc,
                        th.text_muted,
                        12,
                        false,
                    );
                }
            }
            platform_gdi::end_paint(hwnd, &ps);
            0
        }
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_HOVER_IMAGE_READY => {
            let payload_ptr = lparam as *mut HoverPreviewImageResult;
            if payload_ptr.is_null() {
                return 0;
            }
            let payload = Box::from_raw(payload_ptr);
            let ptr = platform_window::user_data(hwnd) as *mut HoverPreviewData;
            if !ptr.is_null() {
                let data = &mut *ptr;
                if data.item_id == payload.item_id {
                    data.image = payload.image;
                    data.loading_item_id = 0;
                    platform_gdi::invalidate_rect(hwnd, null(), 0);
                }
            }
            0
        }
        WM_NCDESTROY => {
            let ptr = platform_window::user_data(hwnd) as *mut HoverPreviewData;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
                platform_window::set_user_data(hwnd, 0);
            }
            0
        }
        _ => platform_window::default_window_proc(hwnd, msg, wparam, lparam),
    }
}

unsafe fn ensure_preview_class() {
    let hinstance = platform_window::module_handle();
    let cname = to_wide(HOVER_PREVIEW_CLASS);
    let mut wc: WNDCLASSEXW = zeroed();
    wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(preview_wnd_proc);
    wc.hInstance = hinstance;
    wc.hCursor = platform_window::arrow_cursor();
    wc.hbrBackground = null_mut();
    wc.lpszClassName = cname.as_ptr();
    platform_window::register_class_ex(&wc);
}

unsafe fn create_preview_window() -> HWND {
    ensure_preview_class();
    platform_window::create_window_ex(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        to_wide(HOVER_PREVIEW_CLASS).as_ptr(),
        to_wide("").as_ptr(),
        WS_POPUP,
        0,
        0,
        PREVIEW_W_TEXT,
        PREVIEW_H_TEXT,
        null_mut(),
        null_mut(),
        platform_window::module_handle(),
        Box::into_raw(Box::new(HoverPreviewData {
            item_id: -1,
            header: String::new(),
            body: String::new(),
            image: None,
            image_width: 0,
            image_height: 0,
            loading_item_id: 0,
            last_x: i32::MIN,
            last_y: i32::MIN,
            last_w: 0,
            last_h: 0,
            zoom_mode: false,
        })) as _,
    )
}

unsafe fn preview_hwnd() -> HWND {
    let raw = *HOVER_HWND.get_or_init(|| create_preview_window() as isize);
    raw as HWND
}

// 放大预览窗的固定装饰尺寸（客户区外的边框/标题留白），与 WM_PAINT 内容矩形一致。
const ZOOM_CHROME_W: i32 = 24; // 左右内边距
const ZOOM_CHROME_H: i32 = 52; // 顶部 header 40 + 底部 12
const ZOOM_MIN_W: i32 = 240;
const ZOOM_MIN_H: i32 = 180;

/// 计算图片放大预览窗尺寸（物理像素，PMv2 下 1:1，不做 DPI 换算）。
///
/// 采用整数等比收缩「只缩不放」：窗口内容区宽高比严格等于图片宽高比，
/// 消除旧算法固定加常量导致的左右空白死区（A-06）。
fn image_zoom_window_size(image_width: usize, image_height: usize, work_area: &RECT) -> (i32, i32) {
    // A-09: 尺寸缺失（历史条目 / LAN 同步条目 image_width==0）时退回普通图片预览尺寸，
    //       绝不能落到比普通预览还小的退化尺寸。
    if image_width == 0 || image_height == 0 {
        return (PREVIEW_W_IMAGE, PREVIEW_H_IMAGE);
    }
    let avail_w = ((work_area.right - work_area.left) * 8 / 10 - ZOOM_CHROME_W).max(ZOOM_MIN_W);
    let avail_h = ((work_area.bottom - work_area.top) * 8 / 10 - ZOOM_CHROME_H).max(ZOOM_MIN_H);

    // A-06: 等比收缩，只缩不放。scale_num 三项取最小分别对应
    // 「宽度受限」「高度受限」「原图更小不放大」。用 i64 避免中间乘积溢出。
    let (iw, ih) = (image_width as i64, image_height as i64);
    let scale_num = (avail_w as i64 * ih).min(avail_h as i64 * iw).min(iw * ih);
    let w = ((scale_num / ih.max(1)) as i32).max(1);
    let h = ((scale_num / iw.max(1)) as i32).max(1);

    (
        (w + ZOOM_CHROME_W).max(ZOOM_MIN_W),
        (h + ZOOM_CHROME_H).max(ZOOM_MIN_H),
    )
}

fn limit_preview_text(text: &str, max_lines: usize, max_chars: usize) -> String {
    let mut out = String::new();
    let mut chars = 0usize;
    let mut lines = 0usize;
    let mut truncated = false;

    let mut source_lines = text.lines().peekable();
    while let Some(line) = source_lines.next() {
        if lines >= max_lines || chars >= max_chars {
            truncated = true;
            break;
        }
        let remaining = max_chars.saturating_sub(chars);
        let chunk: String = line.chars().take(remaining).collect();
        let chunk_chars = chunk.chars().count();
        chars += chunk_chars;
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&chunk);
        lines += 1;
        if chunk_chars < line.chars().count() || source_lines.peek().is_some() && lines >= max_lines
        {
            truncated = true;
            break;
        }
    }

    if out.is_empty() {
        return String::new();
    }
    if truncated {
        out.push_str(" ......");
    }
    out
}

fn limit_file_preview(paths: &[String], max_items: usize) -> String {
    let mut out = paths
        .iter()
        .take(max_items)
        .map(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(path.as_str())
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    if paths.len() > max_items {
        out.push_str(&format!("\n......{} {}", tr("共", "Total"), paths.len()));
    }
    out
}

fn markdown_file_preview_text(paths: &[String]) -> Option<String> {
    if paths.len() != 1 {
        return None;
    }
    let path = std::path::Path::new(&paths[0]);
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "md" | "markdown") {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.by_ref()
        .take(MARKDOWN_PREVIEW_MAX_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    let preview = limit_preview_text(&text, PREVIEW_TEXT_MAX_LINES, PREVIEW_TEXT_MAX_CHARS);
    (!preview.is_empty()).then_some(preview)
}

pub(crate) unsafe fn hide_hover_preview() {
    let hwnd = preview_hwnd();
    if platform_window::exists(hwnd) {
        let ptr = platform_window::user_data(hwnd) as *mut HoverPreviewData;
        if !ptr.is_null() {
            (*ptr).item_id = 0;
            (*ptr).header.clear();
            (*ptr).header.shrink_to_fit();
            (*ptr).body.clear();
            (*ptr).body.shrink_to_fit();
            (*ptr).image = None;
            (*ptr).image_width = 0;
            (*ptr).image_height = 0;
            (*ptr).loading_item_id = 0;
            (*ptr).zoom_mode = false;
        }
        platform_window::hide(hwnd);
    }
}

/// 当前是否存在"瞬态"放大预览（zoom_mode 且可见）。
/// 供 handle_mouse_move 的即时收起判定使用。
pub(crate) unsafe fn hover_zoom_active() -> bool {
    let hwnd = preview_hwnd();
    if !platform_window::exists(hwnd) {
        return false;
    }
    let ptr = platform_window::user_data(hwnd) as *mut HoverPreviewData;
    !ptr.is_null() && (*ptr).zoom_mode && platform_window::is_visible(hwnd)
}

fn spawn_hover_image_load(hwnd: HWND, item: ClipItem) {
    let hwnd_raw = hwnd as isize;
    std::thread::spawn(move || {
        let payload = Box::new(HoverPreviewImageResult {
            item_id: item.id,
            image: ensure_item_image_bytes(&item),
        });
        unsafe {
            let _ = post_boxed_message(hwnd_raw, WM_HOVER_IMAGE_READY, 0, payload);
        }
    });
}

pub(crate) unsafe fn show_hover_preview(
    item: &ClipItem,
    cursor_x: i32,
    cursor_y: i32,
    zoom: bool,
) {
    let hwnd = preview_hwnd();
    if !platform_window::exists(hwnd) {
        return;
    }
    let ptr = platform_window::user_data(hwnd) as *mut HoverPreviewData;
    if ptr.is_null() {
        return;
    }

    let markdown_file_preview = if item.kind == ClipKind::Files {
        item.file_paths
            .as_ref()
            .and_then(|paths| markdown_file_preview_text(paths))
    } else {
        None
    };
    let header = match item.kind {
        ClipKind::Image => tr("图片预览", "Image Preview").to_string(),
        ClipKind::Files if markdown_file_preview.is_some() => {
            tr("Markdown 预览", "Markdown Preview").to_string()
        }
        ClipKind::Files => tr("文件预览", "File Preview").to_string(),
        ClipKind::Phrase => tr("短语预览", "Phrase Preview").to_string(),
        ClipKind::Text if item.rich_text_html.is_some() => {
            tr("富文本预览", "Rich Text Preview").to_string()
        }
        ClipKind::Text => tr("文本预览", "Text Preview").to_string(),
    };
    let body = match item.kind {
        ClipKind::Text | ClipKind::Phrase => {
            if let Some(html) = item.rich_text_html.as_deref() {
                let text = rich_text_preview_text(
                    html,
                    item.text.as_deref().unwrap_or(item.preview.as_str()),
                    PREVIEW_TEXT_MAX_LINES + 1,
                    PREVIEW_TEXT_MAX_CHARS + 1,
                );
                limit_preview_text(&text, PREVIEW_TEXT_MAX_LINES, PREVIEW_TEXT_MAX_CHARS)
            } else {
                limit_preview_text(
                    item.text.as_deref().unwrap_or(item.preview.as_str()),
                    PREVIEW_TEXT_MAX_LINES,
                    PREVIEW_TEXT_MAX_CHARS,
                )
            }
        }
        ClipKind::Files => markdown_file_preview.unwrap_or_else(|| {
            item.file_paths
                .as_ref()
                .map(|paths| limit_file_preview(paths, PREVIEW_FILE_MAX_ITEMS))
                .unwrap_or_else(|| item.preview.clone())
        }),
        ClipKind::Image => String::new(),
    };
    let image_shape = if item.kind == ClipKind::Image {
        Some((item.image_width, item.image_height))
    } else {
        None
    };

    let wa = platform_monitor::nearest_work_rect_for_point(POINT {
        x: cursor_x,
        y: cursor_y,
    });
    let (w, h) = if zoom && image_shape.is_some() {
        image_zoom_window_size(item.image_width, item.image_height, &wa)
    } else if image_shape.is_some() {
        (PREVIEW_W_IMAGE, PREVIEW_H_IMAGE)
    } else {
        (PREVIEW_W_TEXT, PREVIEW_H_TEXT)
    };
    let mut x = cursor_x + 16;
    let mut y = cursor_y + 22;
    if x + w > wa.right {
        x = wa.right - w;
    }
    if y + h > wa.bottom {
        y = wa.bottom - h;
    }
    x = x.max(wa.left);
    y = y.max(wa.top);

    let data = &mut *ptr;
    let same_image_shape = image_shape == Some((data.image_width, data.image_height));
    // 除 zoom_mode 外的内容是否完全相同。header/body/image_shape 都由 item 派生，
    // 因此该标志为真 ⟺ 仍是同一条目（A-14：据此复用已解码位图）。
    let same_content_ignoring_zoom = data.item_id == item.id
        && data.header == header
        && data.body == body
        && same_image_shape;
    let same_content = same_content_ignoring_zoom && data.zoom_mode == zoom;
    let same_geometry =
        data.last_x == x && data.last_y == y && data.last_w == w && data.last_h == h;
    let visible = platform_window::is_visible(hwnd);

    if visible && same_content && same_geometry {
        return;
    }

    if visible && same_content {
        data.last_x = x;
        data.last_y = y;
        data.last_w = w;
        data.last_h = h;
        platform_window::set_pos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        return;
    }

    // A-14：同一条目、仅 zoom_mode 切换（悬浮小预览 <-> 放大查看）。
    // 绝不能重置 data.image / loading_item_id —— 那会丢弃已解码位图并触发无谓重载。
    // 只更新几何与模式，复用现有位图并重绘。
    if visible && same_content_ignoring_zoom {
        data.last_x = x;
        data.last_y = y;
        data.last_w = w;
        data.last_h = h;
        data.zoom_mode = zoom;
        platform_window::set_pos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        platform_gdi::invalidate_rect(hwnd, null(), 0);
        return;
    }

    let image = if item.kind == ClipKind::Image {
        if let Some(bytes) = item.image_bytes.as_ref() {
            Some((bytes.clone(), item.image_width, item.image_height))
        } else {
            if data.loading_item_id != item.id {
                data.loading_item_id = item.id;
                spawn_hover_image_load(hwnd, item.clone());
            }
            None
        }
    } else {
        data.loading_item_id = 0;
        None
    };

    data.item_id = item.id;
    data.header = header;
    data.body = body;
    data.image = image;
    data.image_width = image_shape.map(|shape| shape.0).unwrap_or(0);
    data.image_height = image_shape.map(|shape| shape.1).unwrap_or(0);
    data.last_x = x;
    data.last_y = y;
    data.last_w = w;
    data.last_h = h;
    data.zoom_mode = zoom;

    platform_window::set_pos(
        hwnd,
        HWND_TOPMOST,
        x,
        y,
        w,
        h,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    if !same_content {
        platform_gdi::invalidate_rect(hwnd, null(), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        image_zoom_window_size, limit_preview_text, PREVIEW_H_IMAGE, PREVIEW_TEXT_MAX_CHARS,
        PREVIEW_TEXT_MAX_LINES, PREVIEW_W_IMAGE,
    };
    use windows_sys::Win32::Foundation::RECT;

    /// 1920×1080 工作区：avail_w=(1536-24).max(240)=1512，avail_h=(864-52).max(180)=812。
    fn work_area_1080p() -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        }
    }

    #[test]
    fn image_zoom_size_keeps_aspect_ratio_for_large_image() {
        // 4000×3000（高度受限）：scale_num=min(1512*3000, 812*4000, 4000*3000)=3_248_000
        // w=3_248_000/3000=1082, h=3_248_000/4000=812 → 加 chrome (24,52) → (1106, 864)。
        assert_eq!(
            image_zoom_window_size(4000, 3000, &work_area_1080p()),
            (1106, 864)
        );
    }

    #[test]
    fn image_zoom_size_is_one_to_one_for_small_image() {
        // 800×600（原图更小，只缩不放）：scale_num=min(907200, 649600, 480000)=480_000
        // w=480_000/600=800, h=480_000/800=600 → 加 chrome → (824, 652)，即 1:1 原尺寸。
        assert_eq!(
            image_zoom_window_size(800, 600, &work_area_1080p()),
            (824, 652)
        );
    }

    #[test]
    fn image_zoom_size_lifts_tiny_image_to_minimum() {
        // 100×100：内容 100×100 + chrome=(124,152)，低于 (ZOOM_MIN_W,ZOOM_MIN_H)=(240,180)
        // → 最终 max 钳位抬升到 (240, 180)。
        // （审计正文示例写 124×152 漏算了末行的 min 钳位，此处以代码语义为准。）
        assert_eq!(
            image_zoom_window_size(100, 100, &work_area_1080p()),
            (240, 180)
        );
    }

    #[test]
    fn image_zoom_size_handles_panorama() {
        // 4000×400（宽度受限）：scale_num=min(1512*400, 812*4000, 4000*400)=604_800
        // w=604_800/400=1512, h=604_800/4000=151 → 加 chrome → (1536, 203)。
        assert_eq!(
            image_zoom_window_size(4000, 400, &work_area_1080p()),
            (1536, 203)
        );
    }

    #[test]
    fn image_zoom_size_falls_back_when_dimensions_missing() {
        // A-09：image_width/height==0（历史条目 / LAN 同步条目）→ 退回普通图片预览尺寸，
        // 绝不缩到比普通预览更小。
        assert_eq!(
            image_zoom_window_size(0, 0, &work_area_1080p()),
            (PREVIEW_W_IMAGE, PREVIEW_H_IMAGE)
        );
        assert_eq!(
            image_zoom_window_size(1920, 0, &work_area_1080p()),
            (PREVIEW_W_IMAGE, PREVIEW_H_IMAGE)
        );
    }

    #[test]
    fn text_preview_capacity_matches_text_window() {
        let source = (1..=12)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let preview = limit_preview_text(&source, PREVIEW_TEXT_MAX_LINES, PREVIEW_TEXT_MAX_CHARS);

        assert!(preview.contains("line 9"));
        assert!(!preview.contains("line 10"));
        assert!(preview.ends_with("......"));
    }

    #[test]
    fn complete_multiline_preview_does_not_add_false_ellipsis() {
        let preview = limit_preview_text(
            "first line\nsecond line",
            PREVIEW_TEXT_MAX_LINES,
            PREVIEW_TEXT_MAX_CHARS,
        );

        assert_eq!(preview, "first line\nsecond line");
    }

    #[test]
    fn text_preview_uses_full_420_character_capacity() {
        let source = "测试abc123".repeat(80);
        let preview = limit_preview_text(&source, PREVIEW_TEXT_MAX_LINES, PREVIEW_TEXT_MAX_CHARS);
        let content = preview.strip_suffix(" ......").unwrap();

        assert_eq!(content.chars().count(), PREVIEW_TEXT_MAX_CHARS);
    }
}
