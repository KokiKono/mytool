//! 複数行の入力欄の中身と選択範囲。レイアウトに依らない編集はここで済ませる。
//! オフセットは UTF-8 のバイト位置。IME とのやり取りだけ UTF-16 に直す。

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Default)]
pub struct Buffer {
    text: String,
    selected: Range<usize>,
    reversed: bool,
    /// IME で変換中の範囲。`Some` のあいだ Enter は変換の確定に使われる
    marked: Option<Range<usize>>,
}

impl Buffer {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selected(&self) -> Range<usize> {
        self.selected.clone()
    }

    pub fn reversed(&self) -> bool {
        self.reversed
    }

    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    pub fn is_composing(&self) -> bool {
        self.marked.is_some()
    }

    pub fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    pub fn set_text(&mut self, text: String) {
        self.selected = text.len()..text.len();
        self.text = text;
        self.reversed = false;
        self.marked = None;
    }

    pub fn move_to(&mut self, offset: usize) {
        let offset = offset.min(self.text.len());
        self.selected = offset..offset;
        self.reversed = false;
    }

    pub fn select_to(&mut self, offset: usize) {
        let offset = offset.min(self.text.len());
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
    }

    pub fn select_all(&mut self) {
        self.selected = 0..self.text.len();
        self.reversed = false;
    }

    pub fn previous_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    pub fn next_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.text.len())
    }

    pub fn line_start(&self, offset: usize) -> usize {
        self.text[..offset].rfind('\n').map_or(0, |i| i + 1)
    }

    pub fn line_end(&self, offset: usize) -> usize {
        self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |i| offset + i)
    }

    pub fn left(&mut self) {
        if self.selected.is_empty() {
            self.move_to(self.previous_boundary(self.cursor()));
        } else {
            self.move_to(self.selected.start);
        }
    }

    pub fn right(&mut self) {
        if self.selected.is_empty() {
            self.move_to(self.next_boundary(self.cursor()));
        } else {
            self.move_to(self.selected.end);
        }
    }

    pub fn backspace(&mut self) {
        if self.selected.is_empty() {
            self.select_to(self.previous_boundary(self.cursor()));
        }
        self.replace(None, "");
    }

    pub fn delete(&mut self) {
        if self.selected.is_empty() {
            self.select_to(self.next_boundary(self.cursor()));
        }
        self.replace(None, "");
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selected.clone()]
    }

    /// `range` が無ければ変換中の範囲、それも無ければ選択範囲を置き換える。変換は確定する。
    pub fn replace(&mut self, range: Option<Range<usize>>, new_text: &str) {
        let range = self.target(range);
        self.text.replace_range(range.clone(), new_text);
        let end = range.start + new_text.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
    }

    /// IME の変換中の文字を置く。`selected_in_new` は `new_text` の中での選択範囲。
    pub fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        selected_in_new: Option<Range<usize>>,
    ) {
        let range = self.target(range);
        self.text.replace_range(range.clone(), new_text);
        self.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected = match selected_in_new {
            Some(sel) => {
                let clamp = |o: usize| (range.start + o).min(range.start + new_text.len());
                clamp(sel.start)..clamp(sel.end)
            }
            None => {
                let end = range.start + new_text.len();
                end..end
            }
        };
        self.reversed = false;
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }

    fn target(&self, range: Option<Range<usize>>) -> Range<usize> {
        let len = self.text.len();
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selected.clone());
        range.start.min(len)..range.end.min(len)
    }

    pub fn offset_from_utf16(&self, utf16: usize) -> usize {
        utf16_to_utf8(&self.text, utf16)
    }

    pub fn offset_to_utf16(&self, utf8: usize) -> usize {
        self.text[..utf8.min(self.text.len())]
            .encode_utf16()
            .count()
    }

    pub fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }

    pub fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(r.start)..self.offset_to_utf16(r.end)
    }
}

/// `text` の中の UTF-16 位置を UTF-8 位置にする。サロゲートの途中なら次の文字の頭へ寄せる。
pub fn utf16_to_utf8(text: &str, utf16: usize) -> usize {
    let mut count = 0;
    for (i, ch) in text.char_indices() {
        if count >= utf16 {
            return i;
        }
        count += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str) -> Buffer {
        let mut b = Buffer::default();
        b.set_text(text.into());
        b
    }

    #[test]
    fn typing_inserts_at_the_cursor_and_replaces_the_selection() {
        let mut b = buf("ab");
        b.move_to(1);
        b.replace(None, "x");
        assert_eq!(b.text(), "axb");
        b.select_all();
        b.replace(None, "y\nz");
        assert_eq!(b.text(), "y\nz");
        assert_eq!(b.cursor(), 3);
    }

    #[test]
    fn backspace_removes_one_grapheme() {
        let mut b = buf("あ👍🏽");
        b.backspace();
        assert_eq!(b.text(), "あ");
        b.backspace();
        assert_eq!(b.text(), "");
        b.backspace();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn delete_removes_the_next_grapheme() {
        let mut b = buf("a\nb");
        b.move_to(1);
        b.delete();
        assert_eq!(b.text(), "ab");
    }

    #[test]
    fn left_and_right_collapse_a_selection_first() {
        let mut b = buf("abc");
        b.move_to(1);
        b.select_to(2);
        b.left();
        assert_eq!(b.selected(), 1..1);
        b.select_to(3);
        b.right();
        assert_eq!(b.selected(), 3..3);
    }

    #[test]
    fn selecting_backwards_flips_the_range() {
        let mut b = buf("abc");
        b.move_to(2);
        b.select_to(0);
        assert_eq!(b.selected(), 0..2);
        assert!(b.reversed());
        assert_eq!(b.cursor(), 0);
        assert_eq!(b.selected_text(), "ab");
    }

    #[test]
    fn line_start_and_end_follow_newlines() {
        let b = buf("ab\ncd\n");
        assert_eq!(b.line_start(4), 3);
        assert_eq!(b.line_end(4), 5);
        assert_eq!(b.line_start(1), 0);
        assert_eq!(b.line_end(6), 6);
    }

    #[test]
    fn composing_marks_text_and_committing_replaces_it() {
        let mut b = buf("x");
        b.replace_and_mark(None, "に", None);
        b.replace_and_mark(None, "にほ", None);
        assert_eq!(b.text(), "xにほ");
        assert_eq!(b.marked(), Some(1..7));
        assert!(b.is_composing());

        b.replace(None, "日本");
        assert_eq!(b.text(), "x日本");
        assert!(!b.is_composing());
        assert_eq!(b.cursor(), "x日本".len());
    }

    #[test]
    fn utf16_offsets_round_trip() {
        let b = buf("a日👍b");
        assert_eq!(b.offset_to_utf16(b.text().len()), 1 + 1 + 2 + 1);
        for (utf8, utf16) in [(0, 0), (1, 1), (4, 2), (8, 4), (9, 5)] {
            assert_eq!(b.offset_to_utf16(utf8), utf16);
            assert_eq!(b.offset_from_utf16(utf16), utf8);
        }
    }
}
