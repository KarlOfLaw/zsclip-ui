use super::prelude::*;

pub(super) fn main_theme_role_color(role: MainThemeRole, th: Theme) -> u32 {
    match role {
        MainThemeRole::Surface => th.surface,
        MainThemeRole::Surface2 => th.surface2,
        MainThemeRole::Stroke => th.stroke,
        MainThemeRole::SegmentSelected => {
            if th.bg == rgb(255, 255, 255) {
                th.surface2
            } else {
                th.nav_sel_fill
            }
        }
        MainThemeRole::Background => th.bg,
        MainThemeRole::ControlBg => th.control_bg,
        MainThemeRole::ControlStroke => th.control_stroke,
        MainThemeRole::ButtonHover => th.button_hover,
        MainThemeRole::ButtonPressed => th.button_pressed,
        MainThemeRole::CloseHover => th.close_hover,
        MainThemeRole::ItemSelected => th.item_selected,
        MainThemeRole::ItemHovered => th.item_hover,
        MainThemeRole::Accent => th.accent,
        MainThemeRole::OnAccent => rgb(255, 255, 255),
        MainThemeRole::Text => th.text,
        MainThemeRole::TextMuted => th.text_muted,
    }
}

pub(super) fn pt_in_rect(x: i32, y: i32, rc: &RECT) -> bool {
    x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom
}

pub(super) fn row_supports_image_preview(item: &ClipItem, settings: &AppSettings) -> bool {
    settings.image_preview_enabled
        && (item.kind == ClipKind::Image || image_file_preview_path(item).is_some())
}

/// 计算指定行内图片预览缩略图的矩形区域，用于命中测试。
/// 逻辑与 `MainListLayout::row_content_plan` 中 preview_rect 的计算保持一致。
pub(super) fn compute_row_preview_rect(state: &AppState, visible_idx: i32) -> Option<UiRect> {
    let layout = state.layout();
    let row = layout.row_rect(visible_idx, state.visible_count(), state.scroll_y)?;
    let row_h = row.height();
    let mut text_left = row.left + (row_h * 12 / 44).clamp(10, 20);
    if let Some(icon) = layout.row_icon_rect(visible_idx, state.visible_count(), state.scroll_y) {
        text_left = text_left.max(icon.right + (row_h * 12 / 44).clamp(10, 18));
    }
    let size = (row_h - 8).max(24);
    let left = text_left + 2;
    let top = row.top + (row_h - size) / 2;
    Some(UiRect::new(left, top, left + size, top + size))
}

pub(super) fn scroll_to_top_visible(state: &AppState) -> bool {
    state.scroll_y > state.layout().row_h
}

pub(super) fn main_title_button_visibility(settings: &AppSettings) -> TitleButtonVisibility {
    TitleButtonVisibility {
        search: title_button_visible(settings, "search"),
        setting: title_button_visible(settings, "setting"),
        minimize: title_button_visible(settings, "min"),
        close: title_button_visible(settings, "close"),
    }
}

pub(super) fn main_empty_state_kind(state: &AppState) -> MainEmptyStateKind {
    if state.active_load_state().loading {
        MainEmptyStateKind::Loading
    } else if state.active_load_state().error.is_some() {
        MainEmptyStateKind::Error
    } else if state.settings.grouping_enabled && state.current_group_filter != 0 {
        MainEmptyStateKind::Group
    } else if state.tab_index == 0 {
        MainEmptyStateKind::Records
    } else {
        MainEmptyStateKind::Phrases
    }
}

pub(super) unsafe fn hovered_item_clone(state: &AppState) -> Option<ClipItem> {
    if state.hover_idx < 0 {
        return None;
    }
    state.active_items().get(state.hover_idx as usize).cloned()
}
