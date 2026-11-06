//! 要望を書いて AI とやり取りし、条件書にする画面。

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;

use futures::channel::oneshot;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, Focusable, IntoElement, Render,
    ScrollHandle, SharedString, Task, Window, div, prelude::*, px, rgb,
};
use request::{
    Ask, AskError, Assistant, Conversation, Notice, Pending, Received, Reply, Role, prompt,
    store::RequestStore,
};

use crate::text_area::{self, TextArea};

/// 起動時に自動で流す操作。スクリーンショット用の画面の状態を作るために使う。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Send(String),
    Draft,
}

pub struct Composer {
    conversation: Conversation,
    assistant: Arc<dyn Assistant>,
    store: RequestStore,
    input: Entity<TextArea>,
    log: ScrollHandle,
    focus_handle: FocusHandle,
    script: VecDeque<Step>,
    _task: Option<Task<()>>,
}

impl Composer {
    pub fn new(assistant: Arc<dyn Assistant>, store: RequestStore, cx: &mut Context<Self>) -> Self {
        Self {
            conversation: Conversation::new(),
            assistant,
            store,
            input: cx.new(|cx| TextArea::new("mytool に足したい機能を書く", cx)),
            log: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            script: VecDeque::new(),
            _task: None,
        }
    }

    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    pub fn input(&self) -> &Entity<TextArea> {
        &self.input
    }

    pub fn focus_input(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.input.focus_handle(cx));
    }

    pub fn run_script(&mut self, steps: Vec<Step>, cx: &mut Context<Self>) {
        self.script = steps.into();
        self.next_step(cx);
    }

    fn next_step(&mut self, cx: &mut Context<Self>) {
        if self.conversation.pending().is_some() {
            return;
        }
        match self.script.pop_front() {
            Some(Step::Send(text)) => {
                self.input.update(cx, |input, cx| input.set_text(text, cx));
                self.send(cx);
            }
            Some(Step::Draft) => self.draft(cx),
            None => {}
        }
    }

    pub fn send(&mut self, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            return;
        }
        let text = self.input.read(cx).text().to_string();
        if let Ok(ask) = self.conversation.send(&text) {
            self.input
                .update(cx, |input, cx| input.set_text(String::new(), cx));
            self.ask(ask, cx);
        }
    }

    pub fn draft(&mut self, cx: &mut Context<Self>) {
        if let Ok(ask) = self.conversation.request_draft() {
            self.ask(ask, cx);
        }
    }

    fn ask(&mut self, ask: Ask, cx: &mut Context<Self>) {
        // 返答まで最長で数分かかる。gpui の executor のスレッドを塞がないよう専用のスレッドで待つ
        let (tx, rx) = oneshot::channel();
        let assistant = self.assistant.clone();
        std::thread::spawn(move || {
            let _ = tx.send(assistant.ask(&ask));
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err(AskError::Failed("AI の呼び出しが途中で止まった".into())));
            let _ = this.update(cx, |this, cx| this.receive(result, cx));
        }));
        self.log.scroll_to_bottom();
        cx.notify();
    }

    fn receive(&mut self, result: Result<Reply, AskError>, cx: &mut Context<Self>) {
        match self.conversation.receive(result) {
            Received::Reply => {}
            Received::Failed { restore } => {
                if let Some(sent) = restore {
                    self.input.update(cx, |input, cx| {
                        // 待っているあいだに書き足した分は、戻した文の後ろに残す
                        let typed = input.text();
                        let text = if typed.is_empty() {
                            sent
                        } else {
                            format!("{sent}\n{typed}")
                        };
                        input.set_text(text, cx);
                    });
                }
                self.script.clear();
            }
            Received::Draft(markdown) => match self.store.save(&markdown, SystemTime::now()) {
                Ok(path) => self
                    .conversation
                    .saved(path, prompt::missing_sections(&markdown)),
                Err(error) => self.conversation.save_failed(&error),
            },
        }
        self.log.scroll_to_bottom();
        cx.notify();
        self.next_step(cx);
    }

    fn on_send(&mut self, _: &text_area::Send, _: &mut Window, cx: &mut Context<Self>) {
        self.send(cx);
    }

    fn render_message(role: Role, text: SharedString) -> AnyElement {
        let (label, bg, label_color) = match role {
            Role::User => ("あなた", rgb(0x2a3340), rgb(0x8fb4ff)),
            Role::Assistant => ("AI", rgb(0x262626), rgb(0x9fd39a)),
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .rounded_md()
            .bg(bg)
            .child(div().text_xs().text_color(label_color).child(label))
            .child(div().child(text))
            .into_any_element()
    }

    fn render_notice(notice: &Notice) -> AnyElement {
        let (bg, border, text): (_, _, SharedString) = match notice {
            Notice::Error(message) => (rgb(0x3a1f1f), rgb(0xa34848), message.clone().into()),
            Notice::Saved { path, missing } => {
                let mut text = format!("条件書を保存した: {}", path.display());
                if !missing.is_empty() {
                    text.push_str(&format!("\n足りない見出し: {}", missing.join(" / ")));
                }
                (rgb(0x1f3324), rgb(0x4c9a5c), text.into())
            }
        };
        div()
            .id("notice")
            .mx_4()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(text)
            .into_any_element()
    }

    fn button(
        id: &'static str,
        label: &'static str,
        enabled: bool,
        primary: bool,
        cx: &mut Context<Self>,
        on_click: fn(&mut Self, &mut Context<Self>),
    ) -> AnyElement {
        let bg = match (enabled, primary) {
            (false, _) => rgb(0x2b2b2b),
            (true, true) => rgb(0x3d6bff),
            (true, false) => rgb(0x3a3a3a),
        };
        let mut button = div()
            .id(id)
            .px_3()
            .py_1()
            .rounded_md()
            .bg(bg)
            .text_color(if enabled {
                rgb(0xffffff)
            } else {
                rgb(0x6b6b6b)
            })
            .child(label);
        if enabled {
            button = button
                .cursor_pointer()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)));
        }
        button.into_any_element()
    }
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pending = self.conversation.pending();
        let messages: Vec<AnyElement> = self
            .conversation
            .messages()
            .iter()
            .map(|m| Self::render_message(m.role, m.text.trim_end().to_string().into()))
            .collect();
        let waiting = pending.map(|p| {
            let label = match p {
                Pending::Reply => "考え中…",
                Pending::Draft => "条件書を書いています…",
            };
            div()
                .id("waiting")
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x4a4a4a))
                .text_color(rgb(0xb0b0b0))
                .child(label)
        });
        let empty = self.conversation.messages().is_empty() && pending.is_none();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xe6e6e6))
            .font_family("Menlo")
            .text_size(px(13.))
            .line_height(px(19.))
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_send))
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(0x333333))
                    .text_color(rgb(0xb0b0b0))
                    .child("要望を条件書にする"),
            )
            .child(
                div()
                    .id("log")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.log)
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .when(empty, |log| {
                        log.child(
                            div()
                                .text_color(rgb(0x7a7a7a))
                                .child("足したい機能を書いて送ると、AI が質問や指摘を返します。\nやり取りが済んだら「条件書にする」で Markdown に保存します。"),
                        )
                    })
                    .children(messages)
                    .children(waiting),
            )
            .children(self.conversation.notice().map(Self::render_notice))
            .child(
                div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .h(px(19. * 5. + 16.))
                            .p_2()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0x4a4a4a))
                            .bg(rgb(0x161616))
                            .child(self.input.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(rgb(0x7a7a7a))
                                    .child("Enter で改行 / ⌘Enter で送信"),
                            )
                            .child(Self::button(
                                "draft",
                                "条件書にする",
                                self.conversation.can_draft(),
                                false,
                                cx,
                                Self::draft,
                            ))
                            .child(Self::button(
                                "send",
                                "送信",
                                pending.is_none(),
                                true,
                                cx,
                                Self::send,
                            )),
                    ),
            )
    }
}

impl Focusable for Composer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
