//! 複数行の入力欄。gpui の `examples/input.rs`（単一行）を、折り返し・改行・縦移動に広げたもの。
//! Enter は改行、⌘Enter は [`Send`] を親へ流す。IME で変換中はどちらも何もしない。

mod buffer;

use std::{ops::Range, rc::Rc};

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla,
    KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, ScrollWheelEvent, SharedString, Style, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*, px, relative, rgb,
    rgba, size,
};

use buffer::Buffer;

actions!(
    text_area,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Newline,
        Send,
    ]
);

const CONTEXT: &str = "TextArea";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new("left", Left, Some(CONTEXT)),
        KeyBinding::new("right", Right, Some(CONTEXT)),
        KeyBinding::new("up", Up, Some(CONTEXT)),
        KeyBinding::new("down", Down, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("cmd-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("cmd-left", Home, Some(CONTEXT)),
        KeyBinding::new("cmd-right", End, Some(CONTEXT)),
        KeyBinding::new("home", Home, Some(CONTEXT)),
        KeyBinding::new("end", End, Some(CONTEXT)),
        KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
        KeyBinding::new("cmd-x", Cut, Some(CONTEXT)),
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        KeyBinding::new("enter", Newline, Some(CONTEXT)),
        KeyBinding::new("cmd-enter", Send, Some(CONTEXT)),
    ]);
}

/// 直前に描いたときの折り返し結果。マウス位置や上下移動の計算に使う。
struct Layout {
    /// 改行で区切った行ごとの (先頭のオフセット, 上端の y, 折り返し済みの行)
    lines: Vec<(usize, Pixels, WrappedLine)>,
    line_height: Pixels,
    bounds: Bounds<Pixels>,
    scroll_y: Pixels,
}

impl Layout {
    /// 欄の左上を原点、スクロール前の座標で `offset` の位置を返す。
    fn position_for(&self, offset: usize) -> Point<Pixels> {
        for (start, y, line) in self.lines.iter().rev() {
            if offset >= *start {
                let p = line
                    .position_for_index(offset - start, self.line_height)
                    .unwrap_or_default();
                return point(p.x, *y + p.y);
            }
        }
        Point::default()
    }

    fn offset_for(&self, p: Point<Pixels>, len: usize) -> usize {
        if p.y < Pixels::ZERO {
            return 0;
        }
        for (start, y, line) in &self.lines {
            let height = line.size(self.line_height).height;
            if p.y < *y + height {
                let local = point(p.x.max(Pixels::ZERO), p.y - *y);
                let ix = match line.closest_index_for_position(local, self.line_height) {
                    Ok(ix) | Err(ix) => ix,
                };
                return start + ix.min(line.len());
            }
        }
        len
    }

    fn total_height(&self) -> Pixels {
        self.lines
            .last()
            .map(|(_, y, line)| *y + line.size(self.line_height).height)
            .unwrap_or(self.line_height)
    }

    /// 画面上の点を、欄の左上を原点・スクロール前の座標に直す。
    fn to_content(&self, p: Point<Pixels>) -> Point<Pixels> {
        point(
            p.x - self.bounds.left(),
            p.y - self.bounds.top() + self.scroll_y,
        )
    }
}

pub struct TextArea {
    focus_handle: FocusHandle,
    buffer: Buffer,
    placeholder: SharedString,
    layout: Option<Rc<Layout>>,
    scroll_y: Pixels,
    /// 次の描画でカーソルが見える位置までスクロールする
    reveal_cursor: bool,
    is_selecting: bool,
}

impl TextArea {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            buffer: Buffer::default(),
            placeholder: placeholder.into(),
            layout: None,
            scroll_y: Pixels::ZERO,
            reveal_cursor: true,
            is_selecting: false,
        }
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn is_composing(&self) -> bool {
        self.buffer.is_composing()
    }

    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.buffer.set_text(text);
        self.changed(cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.reveal_cursor = true;
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.backspace();
        self.changed(cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.delete();
        self.changed(cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.left();
        self.changed(cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.right();
        self.changed(cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer
            .select_to(self.buffer.previous_boundary(self.buffer.cursor()));
        self.changed(cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer
            .select_to(self.buffer.next_boundary(self.buffer.cursor()));
        self.changed(cx);
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.vertical_target(-1.);
        self.buffer.move_to(to);
        self.changed(cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.vertical_target(1.);
        self.buffer.move_to(to);
        self.changed(cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_to(self.vertical_target(-1.));
        self.changed(cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_to(self.vertical_target(1.));
        self.changed(cx);
    }

    /// 見た目の 1 行上（`dir` = -1）か下（1）の、同じ x に近い位置。描く前は行頭・行末で代用する。
    fn vertical_target(&self, dir: f32) -> usize {
        let cursor = self.buffer.cursor();
        let Some(layout) = &self.layout else {
            return if dir < 0. {
                self.buffer.line_start(cursor)
            } else {
                self.buffer.line_end(cursor)
            };
        };
        let p = layout.position_for(cursor);
        let target = point(
            p.x,
            p.y + layout.line_height * dir + layout.line_height / 2.,
        );
        if target.y < Pixels::ZERO {
            return 0;
        }
        if target.y >= layout.total_height() {
            return self.buffer.text().len();
        }
        layout.offset_for(target, self.buffer.text().len())
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_all();
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer
            .move_to(self.buffer.line_start(self.buffer.cursor()));
        self.changed(cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer
            .move_to(self.buffer.line_end(self.buffer.cursor()));
        self.changed(cx);
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        // macOS では変換中の Enter は先に IME へ渡り、ここまで来ない。
        // それでも届いたときに改行を足すと確定と二重になるので、変換中は何もしない
        if self.buffer.is_composing() {
            return;
        }
        self.buffer.replace(None, "\n");
        self.changed(cx);
    }

    fn send(&mut self, _: &Send, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.is_composing() {
            return;
        }
        cx.propagate();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.buffer.replace(None, &text.replace("\r\n", "\n"));
            self.changed(cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let selected = self.buffer.selected_text();
        if !selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected.to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let selected = self.buffer.selected_text();
        if !selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected.to_string()));
            self.buffer.replace(None, "");
            self.changed(cx);
        }
    }

    fn offset_at(&self, position: Point<Pixels>) -> usize {
        match &self.layout {
            Some(layout) => {
                layout.offset_for(layout.to_content(position), self.buffer.text().len())
            }
            None => self.buffer.text().len(),
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        let offset = self.offset_at(event.position);
        if event.modifiers.shift {
            self.buffer.select_to(offset);
        } else {
            self.buffer.move_to(offset);
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.buffer.select_to(self.offset_at(event.position));
            cx.notify();
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height()).y;
        self.scroll_y -= delta;
        self.reveal_cursor = false;
        cx.notify();
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.buffer.range_from_utf16(&range_utf16);
        actual_range.replace(self.buffer.range_to_utf16(&range));
        Some(self.buffer.text()[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(&self.buffer.selected()),
            reversed: self.buffer.reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer
            .marked()
            .map(|range| self.buffer.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.buffer.unmark();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|r| self.buffer.range_from_utf16(&r));
        self.buffer.replace(range, new_text);
        self.changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|r| self.buffer.range_from_utf16(&r));
        // 選択範囲は new_text の中の UTF-16 位置で来る
        let selected = new_selected_range_utf16.map(|r| {
            buffer::utf16_to_utf8(new_text, r.start)..buffer::utf16_to_utf8(new_text, r.end)
        });
        self.buffer.replace_and_mark(range, new_text, selected);
        self.changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let range = self.buffer.range_from_utf16(&range_utf16);
        let start = layout.position_for(range.start);
        let origin = point(
            layout.bounds.left() + start.x,
            layout.bounds.top() + start.y - layout.scroll_y,
        );
        Some(Bounds::new(origin, size(px(1.), layout.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let offset = layout.offset_for(layout.to_content(point), self.buffer.text().len());
        Some(self.buffer.offset_to_utf16(offset))
    }
}

struct TextAreaElement {
    input: Entity<TextArea>,
}

struct Prepaint {
    layout: Rc<Layout>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl IntoElement for TextAreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let buffer = &input.buffer;
        let style = window.text_style();
        let line_height = window.line_height();
        let font_size = style.font_size.to_pixels(window.rem_size());

        let (text, color): (SharedString, Hsla) = if buffer.text().is_empty() {
            (input.placeholder.clone(), rgb(0x6b6b6b).into())
        } else {
            (buffer.text().to_string().into(), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs: Vec<TextRun> = match buffer.marked() {
            Some(marked) if !buffer.text().is_empty() => [
                (0..marked.start, None),
                (
                    marked.clone(),
                    Some(UnderlineStyle {
                        color: Some(color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                ),
                (marked.end..text.len(), None),
            ]
            .into_iter()
            .filter(|(r, _)| !r.is_empty())
            .map(|(r, underline)| TextRun {
                len: r.len(),
                underline,
                ..run.clone()
            })
            .collect(),
            _ => vec![run],
        };

        let wrapped = window
            .text_system()
            .shape_text(
                text.clone(),
                font_size,
                &runs,
                Some(bounds.size.width),
                None,
            )
            .unwrap_or_default();
        let mut lines = Vec::with_capacity(wrapped.len());
        let (mut start, mut y) = (0, Pixels::ZERO);
        for line in wrapped {
            let len = line.len();
            let height = line.size(line_height).height.max(line_height);
            lines.push((start, y, line));
            start += len + 1;
            y += height;
        }

        let mut layout = Layout {
            lines,
            line_height,
            bounds,
            scroll_y: Pixels::ZERO,
        };
        let (cursor_pos, selection_pos) = if buffer.text().is_empty() {
            (Point::default(), None)
        } else {
            let sel = buffer.selected();
            (
                layout.position_for(buffer.cursor()),
                (!sel.is_empty())
                    .then(|| (layout.position_for(sel.start), layout.position_for(sel.end))),
            )
        };

        let max_scroll = (layout.total_height() - bounds.size.height).max(Pixels::ZERO);
        let mut scroll_y = input.scroll_y;
        if input.reveal_cursor {
            if cursor_pos.y < scroll_y {
                scroll_y = cursor_pos.y;
            } else if cursor_pos.y + line_height > scroll_y + bounds.size.height {
                scroll_y = cursor_pos.y + line_height - bounds.size.height;
            }
        }
        scroll_y = scroll_y.clamp(Pixels::ZERO, max_scroll);
        layout.scroll_y = scroll_y;

        let origin = point(bounds.left(), bounds.top() - scroll_y);
        let cursor = selection_pos.is_none().then(|| {
            fill(
                Bounds::new(origin + cursor_pos, size(px(2.), line_height)),
                rgb(0x6ea8ff),
            )
        });
        let mut selection = Vec::new();
        if let Some((from, to)) = selection_pos {
            let width = bounds.size.width;
            let mut row_y = from.y;
            while row_y <= to.y {
                let left = if row_y == from.y {
                    from.x
                } else {
                    Pixels::ZERO
                };
                let right = if row_y == to.y { to.x } else { width };
                selection.push(fill(
                    Bounds::from_corners(
                        origin + point(left, row_y),
                        origin + point(right.max(left + px(4.)), row_y + line_height),
                    ),
                    rgba(0x3d6bff55),
                ));
                row_y += line_height;
            }
        }

        let layout = Rc::new(layout);
        self.input.update(cx, |input, _| {
            input.scroll_y = scroll_y;
            input.reveal_cursor = false;
            input.layout = Some(layout.clone());
        });
        Prepaint {
            layout,
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for quad in prepaint.selection.drain(..) {
                window.paint_quad(quad);
            }
            let layout = &prepaint.layout;
            for (_, y, line) in &layout.lines {
                let origin = point(bounds.left(), bounds.top() + *y - layout.scroll_y);
                let _ = line.paint(
                    origin,
                    layout.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            if focus_handle.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
        });
    }
}

impl Render for TextArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::send))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(TextAreaElement { input: cx.entity() })
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
