use super::prelude::*;

pub(super) unsafe fn ensure_mouse_leave_tracking(hwnd: HWND) {
    platform_input::track_mouse_leave_and_hover(
        hwnd,
        platform_system_parameters::mouse_hover_time_ms(),
    );
}

pub(super) unsafe fn hover_preview_blocked_at_point(state: &AppState, x: i32, y: i32) -> bool {
    if scroll_to_top_visible(state) && pt_in_rect(x, y, &state.scroll_to_top_rect()) {
        return true;
    }
    let Some(item) = hovered_item_clone(state) else {
        return false;
    };
    row_quick_delete_rect(state, state.hover_idx, &item)
        .map(|rc| pt_in_rect(x, y, &rc))
        .unwrap_or(false)
}

unsafe fn refresh_hover_preview(hwnd: HWND, state: &mut AppState, x: i32, y: i32) {
    if state.edge_hidden {
        hide_hover_preview();
        return;
    }
    let Some(item_summary) = hovered_item_clone(state) else {
        hide_hover_preview();
        return;
    };
    if hover_preview_blocked_at_point(state, x, y) {
        return;
    }
    let Some(win_rc) = platform_window::window_rect(hwnd) else {
        hide_hover_preview();
        return;
    };

    // 该行是否绘制了行内缩略图（图片条目与图片文件条目均可能命中）。
    // row_preview_rects 是渲染时落盘的单一真源，天然携带 row_supports_image_preview
    // + image_thumb_failed 的过滤结果（A-04 幽灵热区随之消失）。
    let row_has_thumb = state
        .row_preview_rects
        .iter()
        .any(|(idx, _)| *idx == state.hover_idx);

    // 图片缩略图放大预览：独立于通用 hover_preview 开关，仅由 image_zoom_preview_enabled 控制。
    // 命中判定改用渲染时落盘的 preview_rect 单一真源；去掉 is_image 条件后，图片文件条目
    // （ClipKind::Files 指向图片）同样可以放大查看（A-05）。
    let zoom = state.settings.image_zoom_preview_enabled
        && row_preview_hit(state, x, y) == Some(state.hover_idx);

    if zoom {
        show_hover_preview(&item_summary, win_rc.left + x, win_rc.top + y, true);
        return;
    }

    // 通用 hover 预览（文本/文件/图片），仍受 hover_preview 开关控制。
    if !state.settings.hover_preview {
        hide_hover_preview();
        return;
    }
    // 缩略图行在放大预览开启时，仅在缩略图上放大显示，其余区域不再显示通用小预览，
    // 以保证「移开缩略图即消失」的预期行为（图片条目与图片文件条目一致）。
    if row_has_thumb && state.settings.image_zoom_preview_enabled {
        hide_hover_preview();
        return;
    }

    let item = if matches!(item_summary.kind, ClipKind::Text | ClipKind::Phrase) {
        state
            .load_item_full_cached(item_summary.id)
            .unwrap_or(item_summary)
    } else {
        item_summary
    };
    show_hover_preview(&item, win_rc.left + x, win_rc.top + y, false);
}

pub(super) unsafe fn handle_mouse_hover_main(hwnd: HWND, position: UiPoint) {
    let ptr = get_state_ptr(hwnd);
    if ptr.is_null() {
        return;
    }
    let state = &mut *ptr;
    refresh_hover_preview(hwnd, state, position.x, position.y);
}

pub(super) unsafe fn handle_mouse_leave_main(hwnd: HWND) {
    let ptr = get_state_ptr(hwnd);
    if ptr.is_null() {
        return;
    }
    let state = &mut *ptr;
    let transition = main_hover_target_from_state(state).clear_transition(true);
    if transition.changed {
        apply_main_hover_target(state, transition.next);
    }
    hide_hover_preview();
    if state.settings.edge_auto_hide && !state.edge_hidden && !vv_popup_menu_active() {
        if let Some(pt) = platform_input::cursor_pos() {
            if edge_window_scope_contains_point(hwnd, pt) {
                ensure_mouse_leave_tracking(hwnd);
            }
        }
    }
    if transition.changed {
        platform_gdi::invalidate_rect(hwnd, null(), 0);
    }
}

pub(super) unsafe fn clear_main_hover_state(hwnd: HWND) {
    let ptr = get_state_ptr(hwnd);
    if ptr.is_null() {
        return;
    }
    let state = &mut *ptr;
    let transition = main_hover_target_from_state(state).clear_transition(false);
    let mut dirty = transition.changed;
    if transition.changed {
        apply_main_hover_target(state, transition.next);
    }
    if state.down_to_top {
        state.down_to_top = false;
        dirty = true;
    }
    if state.down_row != -1 {
        state.down_row = -1;
        state.down_x = 0;
        state.down_y = 0;
        dirty = true;
    }
    hide_hover_preview();
    if dirty {
        platform_gdi::invalidate_rect(hwnd, null(), 0);
    }
}

pub(super) unsafe fn main_window_should_stay_noactivate(state: &AppState, x: i32, y: i32) -> bool {
    hit_test_row(state, x, y) >= 0
}
