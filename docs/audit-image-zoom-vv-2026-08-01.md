# 图片放大预览 / VV 模式布局 —— 存量代码审计与增量设计

- 审计对象：`1992ae6`、`e2b5388`、`5e64b70` 三次提交及其上下文
- 审计时间：2026-08-01
- 审计人：高见远（架构）
- 代码基线：`5e64b70`（工作区干净，仅 `.workbuddy/` 未跟踪）

## 0. 审计结论速览

三次提交**方向正确但落地不完整**，用户的质疑成立。核心判断：

1. **图片放大预览「能出来，但出错的场景比出对的场景多」**。存在一个默认配置下就会触发的幽灵热区（A-04），以及一条用户明确要求却完全没实现的路径（单击查看，A-08）。
2. **VV 模式的「字体过大导致重叠」不是错觉，而是一个双重 DPI 缩放缺陷**（A-01）。它在 `1992ae6` 之前就存在，`1992ae6` 把 `row_h 30→36` 又把所有派生尺寸额外放大了 20%（A-02），把原本勉强能看的布局推过了临界点。**改行高和宽度并没有解决问题，只是把症状挪了个位置。**
3. **`1992ae6` 附带引入了一个设置页 off-by-one 溢出**（A-03），改动者只加了控件没有同步卡片行数。
4. `e2b5388` 只用 4 行修一个类型错误，证明 `1992ae6` **未经编译即提交**。本机**没有安装 Rust 工具链**（`cargo`/`rustc` 均不存在，`~/.cargo` 不存在），无法给出编译基线——这本身是需要立刻补上的工程门禁缺口（A-12）。

> **审计限制声明**：本次审计为纯静态代码分析。A-01 的倍率推算基于「PMv2 进程下 `GetDeviceCaps(LOGPIXELSY)` 返回系统 DPI」这一 Win32 语义，未经运行时实测。修复前建议先按 §4.1 的验证方法做一次实测确认。

---

## 1. 审计结论表

| 编号 | 位置（文件:行） | 级别 | 现象 | 根因 |
|---|---|---|---|---|
| **A-01** | `src/win_system_ui.rs:83-90`<br>`src/app/vv_popup.rs:94-95`<br>`src/app_core/main_window.rs:2017-2027` | **P0** | VV 窗口字体明显偏大，文字上下被压、易截断；DPI 越高越夸张 | **双重 DPI 缩放**。布局侧 `MainVvPopupLayout::scaled(dpi)` 已把字号乘过一次 `dpi/96`；绘制侧 `create_scaled_font_for_hdc` 又乘一次 `hdc_dpi/96`。150% 下 12 →(布局) 22 →(字体) **33px**，应为 18px，约 **1.8×** |
| **A-02** | `src/app_core/main_window.rs:2013-2015` | **P0** | 即使在 100% DPI，VV 所有间距/字号相对旧版整体涨 20% | `scale_value(v) = (v*row_h + 15)/30`，分母 `30` 是**旧的 row_h 基线**。`1992ae6` 把 `row_h` 改成 36 却没改分母，于是 `s()` 从恒等映射变成隐式 **1.2× 乘子**，字号 12→15、13→16，序号区 24→29，全部连带放大 |
| **A-03** | `src/settings_model.rs:511-515`<br>vs `src/app/settings_general_page_startup.rs:102` | **P1** | 设置→常规→第一张卡片最后一行「快速删除按钮」溢出卡片边框 | 新增开关后 sec0 实占 **11 行**（`row_y(0)`…`row_y(10)`，已核实 11 处调用），但 `GENERAL_FORM_SECTIONS[0].rows` 仍为 **10**，未同步 |
| **A-04** | `src/app/main_view_helpers.rs:41-52` | **P1** | **默认配置下即触发**：没有任何缩略图，鼠标扫过图片行左侧却弹出原图大窗 | `compute_row_preview_rect` **无条件**返回矩形，未校验 `row_supports_image_preview()` 与 `image_thumb_failed`。而 `image_preview_enabled` 默认 **false**、`image_zoom_preview_enabled` 默认 **true**（`state.rs:123-124`），两者天然错配 |
| **A-05** | `src/app/main_hover_preview.rs:43-45` | **P1** | 图片**文件**条目（`ClipKind::Files` 指向 .png）显示了缩略图，却无法放大 | 放大判定用 `matches!(kind, ClipKind::Image)`，缩略图显示判定却是 `row_supports_image_preview`（含 `image_file_preview_path`）。两处口径不一致 |
| **A-06** | `src/hover_preview.rs:236-245` | **P1** | 大图放大窗出现大片空白：4000×3000 在 1080p 上左右各留 ~214px 死区 | `w`、`h` 被**各自独立**钳制到工作区 80%，未保持宽高比；而绘制侧 `scale = min(aw/w, ah/h, 1.0)` 是等比的。注释写「按比例收缩」与实现不符 |
| **A-07** | `src/app/main_input.rs:88-90`<br>`src/app/main_hover_preview.rs:22` | **P1** | 鼠标移出缩略图后放大窗**不会立即消失**，需停顿 ~400ms（`SPI_GETMOUSEHOVERTIME`）且必须停下来 | 刷新只挂在 `WM_MOUSEHOVER`（`PointerHover`）上；`WM_MOUSEMOVE` 里只有 `hover.row_changed` 才 `hide_hover_preview()`，**同一行内移动不触发**。提交说明宣称的「移开即消失」不成立 |
| **A-08** | 需求缺口<br>`src/app/main_input.rs:301-320` | **P1** | 用户明确要求的**「单击预览图查看」完全没有实现**；更糟的是单击缩略图当前会执行**粘贴** | `MainRowReleaseAction` 只有 `QuickDelete`/`Select`/`Paste`/`None`，没有 preview 分支 |
| **A-09** | `src/hover_preview.rs:243`<br>`src/app/data.rs:1653`、`main_clipboard_capture.rs:635,704` | **P2** | 历史条目 / LAN 同步条目 `image_width==0` 时，放大窗退化成 **200×160**，比普通预览 520×360 还小 | `5e64b70` 把下限从 `PREVIEW_W_IMAGE/H_IMAGE` 降到 `200/160`，但没有对 `width==0` 做兜底分支 |
| **A-10** | `src/app/vv_popup.rs:352` vs `:478-479` | **P2** | 混合 DPI 多显示器下 VV 窗口尺寸与绘制布局不一致（内容溢出或右侧留白） | 定位用 `vv_popup_layout_for_window(focus_hwnd)`（**目标应用**窗口的 DPI），绘制用 `vv_popup_layout_for_window(hwnd)`（**弹窗自身**的 DPI），两者可能不同 |
| **A-11** | `src/app_core/main_window.rs:2100` vs `1998` | **P2** | 提示行文本框越过 header 边界（100% 下 bottom=63 > header_h=58） | header 内部元素用 `s()` 缩放，`header_h` 却是独立常量 58，两套缩放体系不同步 |
| **A-12** | 工程流程（`e2b5388`） | **P1** | `1992ae6` 带着编译错误进了主干 | 无提交前编译门禁；本机无 Rust 工具链，无法本地复验 |
| **A-13** | `src/app_core/main_window.rs:2059-2070` | **P2** | VV 行命中测试忽略 x 坐标，窗口左右边距外点击也会选中行 | `hit_test` 只比较 `y`，未比较 `rect.left/right` |
| **A-14** | `src/hover_preview.rs:474-493` | **P2** | 普通预览↔放大预览切换时，已加载的大图被丢弃并重新解码，闪一下「正在加载预览…」 | `zoom_mode` 参与 `same_content` 比较，模式一变就走重建分支，`data.image = image`（此时为 `None`）把缓存冲掉 |

**做对了的地方**（应予保留）：
- `hide_hover_preview()` 已正确执行 `data.image = None`（`hover_preview.rs:332`），**无内存泄漏**。
- 预览窗 `WM_NCHITTEST => HTTRANSPARENT`（`hover_preview.rs:155`），鼠标穿透，**避免了「预览窗盖住光标 → 主窗收到 LEAVE → 隐藏 → 又显示」的抖动死循环**，这是个正确设计。
- GDI 画刷 `create_solid_brush` / `delete_object` 配对正确，`WM_NCDESTROY` 正确 `Box::from_raw` 回收。
- 序号气泡的 `RoundFill` 已彻底移除，**无残留绘制代码**（全仓仅剩 `MainVvPopupTextRole::RowIndex` 三处引用，均为文本路径）。

---

## 2. VV 模式究竟是什么（查明结论）

### 2.1 定义

**VV 模式 = 全局快速粘贴浮层**。用户在**任意应用**的输入框中触发后，屏幕上贴着光标/插入符弹出一个最多 9 条记录的浮层，按 <kbd>1</kbd>–<kbd>9</kbd> 直接粘贴对应条目，<kbd>Esc</kbd> 取消。它**不是**主窗口，也不是快速窗口，是独立的第三种窗口。

界面标题字面量即 `tr("VV 模式", "VV Mode")`，位于 `src/app/vv_popup.rs:484`。

### 2.2 确切标识符清单

| 类别 | 标识符 | 位置 |
|---|---|---|
| 设置开关 | `AppSettings::vv_mode_enabled`（默认 **true**） | `src/app/state.rs:36,120` |
| 设置控件 ID | `IDC_SET_VV_MODE = 5054` | `src/win_system_params.rs:59` |
| 数据源设置 | `vv_source_tab`（0=记录 / 1=短语）、`vv_group_id` | `src/app/state.rs` |
| **布局结构体** | **`MainVvPopupLayout { width: 384, header_h: 58, row_h: 36 }`** | `src/app_core/main_window.rs:1996-2010` |
| 命中枚举 | `MainVvPopupHit::{Group, Row(usize), None}` | `src/app_core/main_window.rs:2059` |
| 渲染计划 | `MainVvPopupRenderPlan` / `MainVvPopupTextCommand` / `MainVvPopupTextRole::{Title,Hint,GroupName,GroupArrow,RowIndex,RowPreview,Empty}` | `src/app_core/main_window.rs:1955-1993` |
| 条目上限 | `MAIN_VV_POPUP_MAX_ITEMS = 9` | `src/app_core/main_window.rs` |
| 窗口过程 | `vv_popup_wnd_proc` | `src/app/vv_popup.rs:~460` |
| 键盘钩子 | `update_vv_mode_hook` / `vv_hook_registered` | `src/app/vv_hook.rs` |
| 运行时状态 | `AppState::vv_popup_items: Vec<VvPopupEntry>`、`vv_popup_group_id`、`vv_popup_target` | `src/app/state.rs` |

### 2.3 布局计算链路（完整）

```
[显示]  vv_popup_show(hwnd, state, target)              src/app/vv_popup.rs:432
          └─> vv_popup_move_near_target(state, popup)   src/app/vv_popup.rs:344
                ├─ layout = MainVvPopupLayout::default()
                │            .scaled(layout_dpi_for_window(focus_hwnd))   ← DPI 源 A：目标应用窗口
                ├─ height = layout.height(items.len())
                │            = header_h + s(20) + rows * row_h
                ├─ anchor  = IMM 候选窗 ▸ 无障碍插入符 ▸ 线程插入符 ▸ 焦点矩形 ▸ 光标
                ├─ 工作区钳制 (x/y 双向，wa.left/top/right/bottom 都有夹取)
                └─> present_vv_popup_window(popup, Rect(x, y, x+layout.width, y+height))

[绘制]  WM_PAINT                                        src/app/vv_popup.rs:470
          ├─ layout = MainVvPopupLayout::default()
          │            .scaled(layout_dpi_for_window(hwnd))    ← DPI 源 B：弹窗自身  ⚠ A-10
          │            .with_width(client_w)                   ← max(client_w, layout.width)
          ├─> layout.render_plan(rc, strings, group, items)    src/app_core/main_window.rs:2072
          │      s(v) = (v * row_h + 15) / 30                  ← ⚠ A-02 魔法分母
          │      ├─ Title      rect(s14, s10, s150, s30)   size s(13)
          │      ├─ Hint       rect(s14, s34, r-s14, s52)  size s(11)   ← ⚠ A-11 s52 > header_h
          │      ├─ group_rect (w-s150, s10, w-s14, s34)
          │      └─ 每行 row_rect(i) = (s12, header_h+s10+i*row_h, w-s12, +row_h-s2)
          │            ├─ index_rect   = (left,      top, left+s24, bottom)  size s(13)
          │            └─ preview_rect = (left+s32,  top, right,    bottom)  size s(12)
          └─> draw_vv_popup_text_command → draw_text_ex → draw_translated_text_line
                 └─> create_scaled_font_for_hdc(hdc, family, size, weight)
                        scaled_size = (size * hdc_dpi + 48) / 96          ← ⚠ A-01 第二次缩放
```

### 2.4 A-01 倍率推算（150% DPI / dpi=144）

| 步骤 | 计算 | 结果 |
|---|---|---|
| `scaled(144)` 后 row_h | `(36*144+48)/96` | 54 |
| 行文本 `s(12)` | `(12*54+15)/30` | 22 |
| 字体二次缩放 | `(22*144+48)/96` | **33 px** |
| 期望值（12 逻辑 px @150%） | `12*1.5` | **18 px** |
| **实际倍率** | | **≈1.83×** |

标题更极端：`s(13)=24` → 字体 **36px**，而标题框高度只有 `s(30)-s(10)=36px`——**字号等于框高**，`DT_VCENTER|DT_SINGLELINE` 下升部降部必然被切。这正是用户看到的「字太大、挤在一起」。

**旁证**：主窗口的 `row_text_size()` 写着 `((row_h*12)/44).clamp(12, 16)`（`main_window.rs:2932`）。那个 `.clamp(12,16)` 就是当年为了压住同一个失控倍率打的补丁——主窗口靠钳位苟住了，**VV 弹窗没有任何钳位，所以彻底崩了**。这也解释了为什么用户只在 VV 模式察觉到问题。

### 2.5 关于「重叠」的精确结论

必须说清楚：**当前代码里两个文本矩形并没有数学意义上的相交**。
- 序号区右边界 43，正文左边界 52，留 9px（100% DPI）；
- 标题右 180，分组按钮左 204，留 24px；
- 提示行底 63，首行顶 70，留 7px。

而且 `draw_translated_text_line` 使用 `DT_SINGLELINE | DT_END_ELLIPSIS`（`win_system_ui.rs:178-182`），文本超出即省略号截断，**不会越界渲染**。

所以用户看到的「重叠」，实质是 **A-01 造成字号约 1.8× 溢出 → 字形在各自的矩形内被垂直裁切、水平被省略号吃掉 → 视觉上糊成一片贴在一起**。**只调 `row_h` 和 `width` 永远治不好它**——`1992ae6` 走错了方向，而且因为 A-02 还让 `s()` 又涨了 20%，实际是**加重**了病情。

---

## 3. 增量设计

### 3.1 图片放大查看（悬浮 + 单击 双路径）

#### 3.1.1 消除「两处公式漂移」——命中矩形单一真源

当前 `compute_row_preview_rect`（`main_view_helpers.rs:41`）是把 `row_content_plan`（`main_window.rs:3184-3196`）的公式**手抄了一遍**，注释还写着「逻辑与 … 保持一致」。这种约定迟早失效。

**方案（根治）**：渲染时把每行实际的 `preview_rect` 落盘到状态，命中检测直接查表。

```rust
// src/app/state.rs —— AppState 新增
/// 最近一次绘制产生的行内缩略图矩形（客户区坐标）。
/// 仅在 row_content_plan 真正产出 preview_rect 时写入，天然携带
/// row_supports_image_preview + image_thumb_failed 的过滤结果。
pub(super) row_preview_rects: Vec<(i32 /*visible_idx*/, UiRect)>,
```

- `main_renderer.rs` 绘制循环开头 `state.row_preview_rects.clear()`；在 `if let Some(preview_rc) = row_content.preview_rect` 分支内 `push((i, preview_rc))`。
- 新增查询：`pub(super) fn row_preview_hit(state: &AppState, x: i32, y: i32) -> Option<i32>`，遍历 `row_preview_rects` 返回命中的 visible_idx。
- **删除** `compute_row_preview_rect`。

一并解决 **A-04**（幽灵热区）与 **A-05**（图片文件条目不可放大）：因为表里只会有真正画了缩略图的行，且 `row_supports_image_preview` 已经包含 `image_file_preview_path`。

> 注：滚动/窗口尺寸变化后必然重绘，表随之更新，不存在陈旧数据窗口期。若担心极端时序，可在 `state.scroll_y` 变更处一并 `clear()`。

#### 3.1.2 触发语义

| 路径 | 触发 | 行为 | 关闭 |
|---|---|---|---|
| **悬浮**（瞬态） | `WM_MOUSEMOVE` 命中 preview_rect，且悬停 ≥ `SPI_GETMOUSEHOVERTIME` | 显示放大预览窗 | 指针移出 rect → **同一条 `WM_MOUSEMOVE` 内立即 hide**；行切换、`WM_MOUSELEAVE`、滚动、隐藏窗口 → hide |
| **单击**（驻留） | `WM_LBUTTONUP` 落在 preview_rect 内 | 打开**驻留查看窗**，不随鼠标移动消失 | <kbd>Esc</kbd> / 再次单击缩略图 / 单击查看窗自身 / 主窗失焦 |

**A-07 修复**：`handle_mouse_move` 里补上「离开热区即隐藏」，不再等 `WM_MOUSEHOVER`：

```rust
// src/app/main_input.rs :: handle_mouse_move，紧跟现有 hover.row_changed 分支
// A-07: 同一行内移出缩略图也要立刻收起放大预览，不等 WM_MOUSEHOVER。
if hover_zoom_active() && row_preview_hit(state, x, y).is_none() {
    hide_hover_preview();
}
```

**A-08 修复**：`MainRowReleaseAction` 增加变体，优先级 `QuickDelete > PreviewImage > Paste`：

```rust
// src/app_core/main_window.rs
pub(crate) enum MainRowReleaseAction {
    QuickDelete { .. },
    PreviewImage { row: i32 },   // 新增
    Select { .. },
    Paste { .. },
    None,
}
```

`row_release_action` 新增入参 `preview_hit: bool`（由 `row_preview_hit()` 计算后传入，**保持 app_core 纯函数、不反向依赖 app 层**），命中则返回 `PreviewImage`。`main_input.rs:301` 的 `match` 增加分支调用 `show_pinned_image_viewer(...)`。

> 交互取舍：单击缩略图从「粘贴」改为「查看」，是行为变更。缩略图只占行内 `row_h-8` 见方的一小块，行内其余区域仍然单击即粘贴，符合「点图看图、点字粘贴」的直觉。**建议在设置项文案中写明**。

#### 3.1.3 尺寸计算（A-06 + A-09）

```rust
// src/hover_preview.rs —— 整体替换 image_zoom_window_size
const ZOOM_CHROME_W: i32 = 24;   // 左右内边距，与 WM_PAINT content 矩形保持一致
const ZOOM_CHROME_H: i32 = 52;   // 顶部 header 40 + 底部 12
const ZOOM_MIN_W: i32 = 240;
const ZOOM_MIN_H: i32 = 180;

fn image_zoom_window_size(image_width: usize, image_height: usize, work_area: &RECT) -> (i32, i32) {
    // A-09: 尺寸缺失（历史条目 / LAN 同步条目 image_width==0）时退回普通图片预览尺寸，
    //       绝不能落到 200x160 —— 那比普通预览还小，是纯粹的退化。
    if image_width == 0 || image_height == 0 {
        return (PREVIEW_W_IMAGE, PREVIEW_H_IMAGE);
    }
    let avail_w = ((work_area.right - work_area.left) * 8 / 10 - ZOOM_CHROME_W).max(ZOOM_MIN_W);
    let avail_h = ((work_area.bottom - work_area.top) * 8 / 10 - ZOOM_CHROME_H).max(ZOOM_MIN_H);

    // A-06: 等比收缩，只缩不放，保证窗口宽高比 == 图片宽高比，消除空白死区。
    let (iw, ih) = (image_width as i64, image_height as i64);
    let scale_num = (avail_w as i64 * ih).min(avail_h as i64 * iw).min(iw * ih);
    let w = ((scale_num / ih.max(1)) as i32).max(1);
    let h = ((scale_num / iw.max(1)) as i32).max(1);

    ((w + ZOOM_CHROME_W).max(ZOOM_MIN_W), (h + ZOOM_CHROME_H).max(ZOOM_MIN_H))
}
```

用整数运算避免浮点误差。`scale_num` 三项取最小分别对应「宽度受限」「高度受限」「原图更小不放大」。

**验算**：
- 4000×3000 @1920×1080 工作区：avail=(1512, 812)。`min(1512*3000, 812*4000, 12e6) = min(4.536e6, 3.248e6, 12e6) = 3.248e6` → w=1082, h=812 → 窗口 1106×864，比例 4:3 ✅（旧算法 1536×864，左右各 214px 空白）
- 800×600：`min(1512*600, 812*800, 480000) = 480000` → 800×600 → 窗口 824×652，**1:1 原尺寸** ✅
- 100×100：→ 124×152（受 ZOOM_MIN_H 抬升，可接受）
- 4000×400 全景：`min(1512*400, 812*4000, 1.6e6) = 604800` → w=1512, h=151 → 窗口 1536×203 ✅

#### 3.1.4 DPI 与坐标系（约定，必须遵守）

| 量 | 坐标系/单位 | 说明 |
|---|---|---|
| `WM_MOUSEMOVE`/`WM_LBUTTONUP` 的 x,y | **主窗口客户区**物理像素 | `UiPoint`，直接与 `row_preview_rects` 比较 |
| `row_preview_rects` 元素 | **主窗口客户区**物理像素 | 与上者同系，**禁止**混入屏幕坐标 |
| `show_hover_preview(cursor_x, cursor_y)` | **屏幕**物理像素 | 调用方必须做 `win_rc.left + x` 转换（现有代码已正确） |
| `item.image_width/height` | 图像**物理像素** | 与 `SetWindowPos` 同为物理像素，PMv2 下 **1:1 无需换算** |
| `nearest_work_rect_for_point` 返回 | **屏幕**物理像素 | 必须用光标所在屏，不能用主屏 |

进程为 `PER_MONITOR_AWARE_V2`（`src/platform/dpi.rs:25-27`），窗口尺寸一律物理像素，**放大预览窗不做任何 DPI 换算**——「原图 1 像素 = 屏幕 1 像素」正是用户要的「原图大小」。预览窗是瞬态的，**不处理 `WM_DPICHANGED`**（跨屏时下次刷新自然重算）。

#### 3.1.5 资源生命周期

- `hide_hover_preview()` 已正确清空 `data.image`，**保持现状**。
- **A-14**：`same_content` 的 `zoom_mode` 比较导致模式切换时丢弃已解码位图。修复：只有 `item_id` 变化时才 `data.image = None`；`zoom_mode` 变化仅触发 `set_pos` + `invalidate_rect`，复用已有位图。
- 新增的驻留查看窗**复用同一个 `HOVER_HWND`**（加 `pinned: bool` 字段），不新建窗口类、不新建 HWND，从根上避免窗口泄漏。
- GDI：继续遵守「谁 create 谁 delete，同一函数内配对」，`WM_PAINT` 内不缓存 HBRUSH/HFONT（字体已有全局 `SCALED_FONT_CACHE`）。

### 3.2 VV 模式布局自适应（基于字体度量，而非硬编码）

#### 3.2.1 第一步：拆掉双重缩放（A-01）——必须最先做

**这是所有 VV 布局问题的总开关，不修它，后面全是徒劳。**

`create_scaled_font_for_hdc` 是全局共用的（主窗、设置窗、VV 窗都走它），**不能改它**，否则全局字体一起变小。正确做法是：**让布局侧输出逻辑尺寸，把 DPI 缩放的唯一职责交给字体层**。

```rust
// src/app_core/main_window.rs
impl MainVvPopupTextCommand {
    /// 字号语义：逻辑像素（96 DPI 基准）。
    /// 实际 DPI 缩放由 create_scaled_font_for_hdc 统一完成，布局层不得预先缩放。
    pub(crate) size: i32,
}
```

`render_plan` 内所有 `size:` 字段改用**未经 `s()` 的字面量**：

| 角色 | 现状 | 改为 |
|---|---|---|
| Title | `s(13)` | `13` |
| Hint | `s(11)` | `11` |
| GroupName / GroupArrow | `s(11)` | `11` |
| Empty | `s(12)` | `12` |
| **RowIndex** | `s(13)` | `12` |
| **RowPreview** | `s(12)` | `12` |

**矩形**仍用 `s()`（它们是几何量，需要跟随 DPI）；**只有 `size` 字段脱离 `s()`**。

> 主窗口 `row_text_size()` 的 `.clamp(12,16)` 是同一个病的钳位补丁，属于同源缺陷。**本次不动它**（影响面覆盖整个主窗口，需独立评估与回归），仅在此登记。

#### 3.2.2 第二步：修掉魔法分母（A-02）

```rust
// src/app_core/main_window.rs
/// VV 弹窗的设计基线行高（96 DPI）。scale_value 以此为分母，
/// 确保 row_h 调整时派生尺寸保持等比，不引入隐式乘子。
const VV_BASE_ROW_H: i32 = 36;

fn scale_value(self, value: i32) -> i32 {
    ((value * self.row_h.max(1)) + VV_BASE_ROW_H / 2) / VV_BASE_ROW_H
}
```

这样 100% DPI 下 `s(v) == v`，恢复恒等映射。**今后任何人改 `row_h` 必须同步改 `VV_BASE_ROW_H`**，常量名和注释就是护栏。

#### 3.2.3 第三步：行高改为字体度量驱动

彻底摆脱「36 是拍脑袋定的」：

```rust
// src/app_core/main_window.rs
/// 依据实测文本度量构造布局。row_h 由行文本高度撑开，
/// 而非硬编码常量，从而对任意字体/DPI/用户字号自适应。
pub(crate) fn from_metrics(text_line_h: i32, dpi: u32) -> Self {
    let pad_v = (text_line_h / 3).clamp(4, 14);       // 上下留白
    let row_h = (text_line_h + pad_v * 2).max(28);
    let header_h = row_h * 2 - row_h / 6;             // header 与行高联动，修 A-11
    Self { width: MainVvPopupLayout::default().scaled(dpi).width, header_h, row_h }
}
```

调用侧（`vv_popup.rs`）在 `WM_PAINT` / `move_near_target` 中先用 `GetTextMetrics` 量出 `UiText` 在当前 HDC、当前 DPI 下的 `tmHeight + tmExternalLeading`，再喂给 `from_metrics`。

**并补上 header 内容与 header_h 的一致性断言**（A-11）：

```rust
debug_assert!(
    self.hint_rect_bottom() <= self.header_h,
    "VV header 内容溢出 header_h：hint_bottom={} header_h={}",
    self.hint_rect_bottom(), self.header_h
);
```

#### 3.2.4 第四步：统一 DPI 源（A-10）

`vv_popup_move_near_target` 与 `WM_PAINT` 必须用**同一个 DPI**。规则：**一律以弹窗自身 HWND 的 DPI 为准**；弹窗创建后若 `layout_dpi_for_window(popup) != layout_dpi_for_window(focus_hwnd)`，重新调用一次 `present_vv_popup_window` 修正尺寸。

```rust
// src/app/vv_popup.rs :: vv_popup_move_near_target
// A-10: 先按目标窗 DPI 预估位置，落位后以弹窗自身 DPI 为准复算一次，
//       保证与 WM_PAINT 使用的布局完全一致。
let popup_dpi = platform_dpi::layout_dpi_for_window(popup);
let layout = vv_popup_layout().scaled(popup_dpi);
```

#### 3.2.5 第五步：命中测试补 x 轴（A-13）

```rust
// src/app_core/main_window.rs :: hit_test
for row in 0..rows {
    let rect = self.row_rect(row);
    if rect.contains(x, y) {          // 原为仅比较 y
        return MainVvPopupHit::Row(row);
    }
}
```

### 3.3 简约序号样式定义

`1992ae6` 已经把气泡改成纯文本，方向对，但**没做完**：字号 `s(13)` 比正文 `s(12)` 还大、`bold: true`，序号比内容更抢眼，谈不上简约。

| 属性 | 现状 | 定稿 | 理由 |
|---|---|---|---|
| 背景 | 无（已移除气泡）✅ | 无 | 保持 |
| 颜色 | `TextMuted` ✅ | `TextMuted` | 保持 |
| 字号 | `s(13)`（>正文） | **`12`（逻辑 px，与正文同级）** | 序号不该比内容更大 |
| 字重 | `bold: true` | **`false`** | 弱化为辅助信息 |
| 对齐 | `Center` | **`Center`** | 1–9 全为单字符，居中最稳 |
| 序号区宽 | `s(24)` | **`s(20)`** | 单字符够用，给正文让出 4px |
| 正文左偏移 | `s(32)` | **`s(28)`** | 与序号区右边界保持 8px 间隙 |
| 字体 | `UiText` | `UiText`（等宽数字特性由字体族保证） | 保持 |
| 选中/悬停行 | 无差异 | **命中行序号提到 `Text` 色** | 低成本焦点反馈 |

几何自检（100% DPI，修完 A-02 后 `s(v)==v`）：序号区 `[14, 34)`，正文起于 `14+28=42`，间隙 **8px**；12px 字号的单字符宽约 7px，居中于 20px 区内左右各余 6.5px。**无重叠，留白均匀。**

---

## 4. 修复任务列表

> 严格按顺序执行。T1 是所有 VV 视觉问题的总开关，**不先做 T1，后面的调参全是白费**。

### T1 —— 拆除 VV 双重 DPI 缩放 + 修复魔法分母 【P0，阻断】

- **文件**：`src/app_core/main_window.rs`（`MainVvPopupLayout::scale_value` 2013-2015、`render_plan` 2072-2205）
- **依赖**：无
- **改动**：
  1. 新增 `const VV_BASE_ROW_H: i32 = 36;`，`scale_value` 分母由字面量 `30` 改为 `VV_BASE_ROW_H`，舍入项同步改 `VV_BASE_ROW_H / 2`。
  2. `render_plan` 中 **6 处 `size:` 字段**去掉 `s()`：Title `s(13)`→`13`、Hint `s(11)`→`11`、GroupName `s(11)`→`11`、GroupArrow `s(11)`→`11`、Empty `s(12)`→`12`、RowIndex `s(13)`→`12`、RowPreview `s(12)`→`12`。**矩形一律保持 `s()` 不变。**
  3. 在 `MainVvPopupTextCommand::size` 上加文档注释，写明「逻辑像素，DPI 缩放由字体层唯一负责」。
- **验收**：96 DPI 下 `s(12) == 12`；144 DPI 下行文本实际字号 = `(12*144+48)/96 = 18`px（修复前 33px）。
- **测试**：`main_vv_popup_layout_*` 三个测试的期望值必须整体重算（`main_window.rs:5372-5450`）。**逐个手算核对，不要照着新输出回填。**

### T2 —— VV 布局字体度量自适应 + header 一致性 + DPI 源统一 + 命中测试 【P1】

- **文件**：`src/app_core/main_window.rs`（新增 `from_metrics`、`hit_test` 2059-2070、header 断言）、`src/app/vv_popup.rs`（`vv_popup_layout_for_window` 94-96、`vv_popup_move_near_target` 352、`WM_PAINT` 478-479）
- **依赖**：**T1**
- **改动**：
  1. 新增 `MainVvPopupLayout::from_metrics(text_line_h, dpi)`（§3.2.3）。
  2. `vv_popup.rs` 用 `GetTextMetrics` 取当前 HDC 下 `UiText` 的 `tmHeight + tmExternalLeading` 驱动 `from_metrics`；无 HDC 的路径回退 `default().scaled(dpi)`。
  3. `vv_popup_move_near_target` 改用 `layout_dpi_for_window(popup)`（A-10）。
  4. `header_h` 与 `row_h` 联动，加 `debug_assert!(hint_bottom <= header_h)`（A-11）。
  5. `hit_test` 行判定改 `rect.contains(x, y)`（A-13）。
- **验收**：100%/125%/150%/200% 四档，标题、提示、分组按钮、序号、正文均无裁切；行左右边距外点击不选中。

### T3 —— 简约序号定稿 【P2】

- **文件**：`src/app_core/main_window.rs`（`render_plan` 行循环 2169-2200）
- **依赖**：**T1**（字号语义变更后才有意义）
- **改动**：按 §3.3 表格落值——RowIndex `size: 12`、`bold: false`、序号区 `s(24)`→`s(20)`、RowPreview 左偏移 `s(32)`→`s(28)`；命中行序号色切 `Text`。
- **验收**：序号视觉权重低于正文；序号区与正文间隙 8px（100% DPI）。

### T4 —— 缩略图命中矩形单一真源 + 修幽灵热区 + 图片文件条目 【P1】

- **文件**：`src/app/state.rs`（新增 `row_preview_rects`）、`src/app/main_renderer.rs`（339 附近写入）、`src/app/main_view_helpers.rs`（**删除** `compute_row_preview_rect`，新增 `row_preview_hit`）、`src/app/main_hover_preview.rs`（43-49 改用新函数）
- **依赖**：无（可与 T1 并行）
- **改动**：
  1. `AppState` 增 `row_preview_rects: Vec<(i32, UiRect)>`。
  2. `main_renderer.rs` 行循环开始前 `clear()`；`if let Some(preview_rc) = row_content.preview_rect` 分支内 `push((i, preview_rc))`。
  3. 新增 `pub(super) fn row_preview_hit(state: &AppState, x: i32, y: i32) -> Option<i32>`。
  4. **删除** `compute_row_preview_rect`（A-04 幽灵热区随之消失）。
  5. `main_hover_preview.rs` 的 zoom 判定改为 `state.settings.image_zoom_preview_enabled && row_preview_hit(state, x, y) == Some(state.hover_idx)`，**去掉 `is_image` 条件**（A-05，让图片文件条目也能放大）。
- **验收**：`image_preview_enabled = false` 时（默认值）悬浮任何位置都不弹放大窗；`.png` 文件条目可放大；缩略图加载失败的行不触发。
- **注意**：`state.rs` 的字段声明有源码字符串断言测试（`app_tests.rs:3211` 形如 `assert!(state.contains("pub(super) vv_popup_items: ..."))`），新增字段后检查是否需要补对应断言。

### T5 —— 放大窗尺寸等比化 + 零尺寸兜底 + 位图复用 【P1】

- **文件**：`src/hover_preview.rs`（`image_zoom_window_size` 236-245、`show_hover_preview` 的 `same_content` 474-493）
- **依赖**：无（可与 T1/T4 并行）
- **改动**：
  1. 按 §3.1.3 整体替换 `image_zoom_window_size`，新增 4 个 `ZOOM_*` 常量。
  2. `image_width == 0 || image_height == 0` 退回 `(PREVIEW_W_IMAGE, PREVIEW_H_IMAGE)`（A-09）。
  3. A-14：`data.image = None` 的条件收紧为「`item_id` 变化」；`zoom_mode` 变化只做 `set_pos` + `invalidate_rect`，复用已解码位图。
  4. 为 `image_zoom_window_size` 补**纯函数单元测试**（该函数不含 unsafe，可直接测）：4000×3000、800×600、100×100、4000×400、0×0 五组，断言窗口宽高比与图片一致（±1px）且不超工作区 80%。
- **验收**：大图无空白死区；800×600 呈 1:1；零尺寸条目不退化为 200×160。

### T6 —— 悬浮即时收起 + 新增单击查看路径 【P1】

- **文件**：`src/app/main_input.rs`（`handle_mouse_move` 88-90、`handle_lbutton_up` 301-320）、`src/app_core/main_window.rs`（`MainRowReleaseAction`、`row_release_action`）、`src/hover_preview.rs`（`pinned` 支持）
- **依赖**：**T4**（需要 `row_preview_hit`）、**T5**（需要正确的尺寸算法）
- **改动**：
  1. A-07：`handle_mouse_move` 中「命中过 zoom 且当前不在任何 preview_rect 内」→ 立即 `hide_hover_preview()`。
  2. A-08：`MainRowReleaseAction` 新增 `PreviewImage { row: i32 }`；`row_release_action` 增加入参 `preview_hit: bool`，优先级 `QuickDelete > PreviewImage > Paste`。
  3. `handle_lbutton_up` 的 `match` 增加 `PreviewImage` 分支。
  4. `HoverPreviewData` 增 `pinned: bool`：为 true 时忽略 hover 驱动的 hide；<kbd>Esc</kbd> / 再次单击缩略图 / 主窗失焦 → 清 `pinned` 并 hide。**复用现有 `HOVER_HWND`，不新建窗口。**
  5. 设置项文案更新为「悬停/单击缩略图放大查看」，说明单击缩略图不再粘贴。
- **验收**：移出缩略图立即消失（无 400ms 延迟）；单击缩略图打开驻留窗且**不粘贴**；Esc 关闭；行内非缩略图区域单击仍正常粘贴。
- **风险**：改变既有单击语义，需在 CHANGELOG 显著位置说明。

### T7 —— 修复设置页第一节行数溢出 【P1，独立】

- **文件**：`src/settings_model.rs:511-515`
- **依赖**：无（**可立即执行，与其他任务完全独立**）
- **改动**：`GENERAL_FORM_SECTIONS[0].rows` 由 `10` 改为 `11`。
- **验收**：设置→常规，第一张卡片完整包住「快速删除按钮」，不越框。
- **附带**：搜索是否有断言 `control_rows` 的测试需同步。

### T8 —— 补齐编译与回归门禁 【P1，流程】

- **文件**：`.github/workflows/build-windows-exe.yml`（或新增 `ci-check.yml`）
- **依赖**：无（**建议与 T1 并行启动，越早越好**）
- **改动**：
  1. 增加 PR/push 触发的 `cargo check --all-targets` + `cargo test` + `cargo clippy -- -D warnings` 作业。
  2. 在 `docs/` 或 `CONTRIBUTING` 记录：**本地无 Rust 工具链时禁止直接推主干**，必须走 CI 验证。
- **理由**：`e2b5388` 用 4 行修一个类型错误，证明 `1992ae6` 未编译即提交。本机 `cargo`/`rustc` 均不存在、`~/.cargo` 目录不存在，**本次审计无法提供编译基线**。这个洞不补，同类问题一定会再来。
- **验收**：CI 在 `5e64b70` 上跑绿，确立基线。

### 任务依赖图

```
T1 (P0 双重DPI+魔法分母) ──┬──> T2 (字体度量自适应/DPI源/命中)
                          └──> T3 (简约序号)

T4 (命中矩形单一真源) ──┬──> T6 (即时收起 + 单击查看)
T5 (尺寸等比+兜底)  ────┘

T7 (设置页行数)    —— 独立，可立即执行
T8 (CI 门禁)       —— 独立，建议最先启动
```

**建议执行批次**：
- **第 1 批（并行）**：T8、T7、T1、T4、T5
- **第 2 批**：T2、T6
- **第 3 批**：T3

---

## 5. 共享知识（跨文件约定）

### 5.1 坐标系约定

| 场景 | 坐标系 | 单位 |
|---|---|---|
| `UiEvent::Pointer*` 的 `position` | 所属窗口客户区 | 物理像素 |
| `AppState::row_preview_rects`、`row_rect`、`quick_action_rect` | 主窗口客户区 | 物理像素 |
| `show_hover_preview(cursor_x, cursor_y)` | **屏幕** | 物理像素 |
| `platform_monitor::nearest_work_rect_for_*` | **屏幕** | 物理像素 |
| `MainVvPopupLayout` 全部矩形 | VV 弹窗客户区 | 物理像素 |
| `ClipItem::image_width/height` | —— | 图像物理像素 |

**铁律**：客户区 → 屏幕必须显式 `win_rc.left + x` 或 `client_to_screen`。任何函数只要同时接触两种坐标系，**必须在参数名或文档注释中标注**（如 `cursor_x: 屏幕坐标`）。

### 5.2 DPI 换算职责（本次修复后的唯一版本）

进程为 **`PER_MONITOR_AWARE_V2`**（`src/platform/dpi.rs:25-27`），所有 Win32 尺寸均为物理像素。

```
几何量（矩形、行高、内边距）：布局层负责 DPI 缩放
    MainVvPopupLayout::scaled(dpi) / main_layout_for_dpi(dpi)

字号（TextCommand::size）：布局层输出【逻辑像素，96 DPI 基准】，不缩放
    ↓
    create_scaled_font_for_hdc 是【唯一】的字号 DPI 缩放点
    scaled_size = (size * dpi + 48) / 96
```

> **任何人在布局层对 `size` 字段调用 `s()` / `scaled()`，都是在重新制造 A-01。** Code Review 必查项。

DPI 取值统一走 `platform_dpi::layout_dpi_for_window(hwnd)`，**以「要绘制的那个窗口」自身的 HWND 为准**，不得借用其他窗口的 DPI（A-10 即为反例）。

### 5.3 资源生命周期规则

| 资源 | 规则 |
|---|---|
| GDI 对象（HBRUSH/HPEN/HBITMAP） | 谁 create 谁 delete，**同一函数体内配对**；`WM_PAINT` 内不跨消息缓存 |
| HFONT | **只允许**经 `create_scaled_font_for_hdc`，由全局 `SCALED_FONT_CACHE` 持有，**调用方不得 DeleteObject** |
| 窗口 user data（`Box::into_raw`） | 必须在 `WM_NCDESTROY` 中 `Box::from_raw` 回收，并 `set_user_data(hwnd, 0)` |
| 跨线程消息载荷 | `Box::into_raw` 投递，接收方**无论是否使用都必须** `Box::from_raw`（`WM_HOVER_IMAGE_READY` 是正确范例） |
| 大位图（`HoverPreviewData::image`） | 隐藏时置 `None` 立即释放；仅当 `item_id` 变化时才丢弃重载（T5-3） |
| 预览/查看窗 HWND | **全进程唯一**，由 `HOVER_HWND: OnceLock` 持有；驻留查看窗复用同一 HWND，**禁止新建** |

### 5.4 设置项新增检查清单

新增一个 `AppSettings` 布尔项，必须同步改**七处**（`1992ae6` 漏了第 7 项，导致 A-03）：

1. `src/app/state.rs` —— 字段 + `Default`
2. `src/win_system_params.rs` —— `IDC_SET_*` 常量（勿与既有 ID 冲突）
3. `src/app/prelude.rs` —— re-export
4. `src/app/settings_general_page_startup.rs` —— `own_toggle_row(..., row_y(N))`
5. `src/app/settings_toggle_state_general.rs` —— 读/写分支
6. `src/settings_model.rs` —— `native_control_binding_for_key` / `native_control_route_for_key` / `settings_native_control_summaries` / `settings_native_json_updates_for_applied_field`
7. **`src/settings_model.rs` 的 `*_FORM_SECTIONS[n].rows` 必须 +1** ← **最易遗漏，A-03 就死在这里**

### 5.5 布局改动红线

- **禁止**为解决视觉问题去调 `row_h` / `width` 等基线常量而不核查其派生链。`scale_value` 类函数的分母**必须**是命名常量并与基线绑定。
- 命中矩形与绘制矩形**必须同源**。发现「注释写着保持一致」的手抄公式，一律视为缺陷（A-04 的根因）。
- 任何依赖 `paint_commands.len()` 的断言都是脆弱的，应改为按 `role` 查找。

---

## 6. 遗留与后续建议

1. **主窗口 `row_text_size()` 的 `.clamp(12,16)`**（`main_window.rs:2932`）与 A-01 同源。本次不动，建议单开任务评估——影响面覆盖整个主窗口，需完整回归。
2. **`native_host_vv_select_specs`** 位于 `#[cfg(feature = "vv-paste")]` 之后（`vv_popup.rs:515`）。该 feature 若未纳入 CI 编译矩阵，其中的 `debug_assert_eq!` 长期无人验证。建议 T8 的 CI 加上 `--all-features` 一档。
3. **`hide_hover_preview()` 内部调用 `preview_hwnd()`**，而后者是 `get_or_init` —— 首次调用会**创建**窗口。即「隐藏一个从未显示过的预览」会凭空造出一个 HWND。非泄漏（`OnceLock` 只创建一次），但语义别扭，建议改为「窗口不存在则直接返回」。
4. **测试期望值回填风险**：T1/T2/T3 会让 `main_vv_popup_layout_*` 三个测试的几乎所有期望值变化。**务必逐个手算核对，严禁把新输出直接粘回断言**——那等于把 bug 固化成规格。
