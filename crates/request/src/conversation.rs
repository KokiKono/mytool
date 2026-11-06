use std::path::PathBuf;

use crate::{Ask, AskError, Reply, prompt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// 返答を待っている問い合わせの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Reply,
    Draft,
}

/// 画面の下に 1 つだけ出す知らせ。次の操作を始めると消える。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Error(String),
    Saved {
        path: PathBuf,
        /// 条件書に足りない見出し。空なら全部そろっている
        missing: Vec<&'static str>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// 前の問い合わせの返答をまだ待っている
    Busy,
    Empty,
}

/// [`Conversation::receive`] の結果。呼び出し側がすることを表す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Received {
    Reply,
    /// 送った文を入力欄に戻す
    Failed {
        restore: Option<String>,
    },
    /// この Markdown を条件書として保存し、[`Conversation::saved`] か
    /// [`Conversation::save_failed`] で結果を知らせる
    Draft(String),
}

#[derive(Debug, Default)]
pub struct Conversation {
    messages: Vec<Message>,
    session_id: Option<String>,
    pending: Option<Pending>,
    notice: Option<Notice>,
}

impl Conversation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn pending(&self) -> Option<Pending> {
        self.pending
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn can_draft(&self) -> bool {
        self.pending.is_none() && self.messages.iter().any(|m| m.role == Role::Assistant)
    }

    /// 発言を会話に足し、AI に渡す問い合わせを返す。
    pub fn send(&mut self, text: &str) -> Result<Ask, Rejected> {
        if self.pending.is_some() {
            return Err(Rejected::Busy);
        }
        if text.trim().is_empty() {
            return Err(Rejected::Empty);
        }
        self.messages.push(Message {
            role: Role::User,
            text: text.to_string(),
        });
        self.pending = Some(Pending::Reply);
        self.notice = None;
        Ok(Ask {
            prompt: text.to_string(),
            resume: self.session_id.clone(),
        })
    }

    /// ここまでのやり取りから条件書を書かせる問い合わせを返す。
    pub fn request_draft(&mut self) -> Result<Ask, Rejected> {
        if self.pending.is_some() {
            return Err(Rejected::Busy);
        }
        if !self.can_draft() {
            return Err(Rejected::Empty);
        }
        self.pending = Some(Pending::Draft);
        self.notice = None;
        Ok(Ask {
            prompt: prompt::DRAFT.to_string(),
            resume: self.session_id.clone(),
        })
    }

    pub fn receive(&mut self, result: Result<Reply, AskError>) -> Received {
        let Some(pending) = self.pending.take() else {
            // 待っていないのに届いた返答は捨てる
            return Received::Reply;
        };
        match (pending, result) {
            (pending, Ok(reply)) => {
                self.session_id = Some(reply.session_id);
                match pending {
                    Pending::Reply => {
                        self.messages.push(Message {
                            role: Role::Assistant,
                            text: reply.text,
                        });
                        Received::Reply
                    }
                    Pending::Draft => {
                        let markdown = prompt::strip_fence(&reply.text);
                        self.messages.push(Message {
                            role: Role::Assistant,
                            text: markdown.clone(),
                        });
                        Received::Draft(markdown)
                    }
                }
            }
            (Pending::Reply, Err(error)) => {
                // 届かなかった発言は会話から外して入力欄に戻す。残すと次に送ったとき二重になる
                let restore = match self.messages.last() {
                    Some(Message {
                        role: Role::User, ..
                    }) => self.messages.pop().map(|m| m.text),
                    _ => None,
                };
                self.notice = Some(Notice::Error(error.to_string()));
                Received::Failed { restore }
            }
            (Pending::Draft, Err(error)) => {
                self.notice = Some(Notice::Error(error.to_string()));
                Received::Failed { restore: None }
            }
        }
    }

    pub fn saved(&mut self, path: PathBuf, missing: Vec<&'static str>) {
        self.notice = Some(Notice::Saved { path, missing });
    }

    pub fn save_failed(&mut self, error: &std::io::Error) {
        self.notice = Some(Notice::Error(format!("条件書を保存できなかった: {error}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(text: &str, session: &str) -> Result<Reply, AskError> {
        Ok(Reply {
            text: text.into(),
            session_id: session.into(),
        })
    }

    #[test]
    fn first_send_starts_a_new_session_and_later_sends_resume_it() {
        let mut c = Conversation::new();
        let first = c.send("メモ帳がほしい").unwrap();
        assert_eq!(first.resume, None);
        c.receive(reply("何を保存しますか", "s-1"));

        let second = c.send("テキストだけ").unwrap();
        assert_eq!(second.resume.as_deref(), Some("s-1"));
        c.receive(reply("分かりました", "s-2"));

        let third = c.send("以上").unwrap();
        assert_eq!(third.resume.as_deref(), Some("s-2"));
    }

    #[test]
    fn messages_are_kept_in_the_order_they_happened() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        c.receive(reply("二", "s"));
        c.send("三").unwrap();
        c.receive(reply("四", "s"));
        let log: Vec<_> = c
            .messages()
            .iter()
            .map(|m| (m.role, m.text.as_str()))
            .collect();
        assert_eq!(
            log,
            [
                (Role::User, "一"),
                (Role::Assistant, "二"),
                (Role::User, "三"),
                (Role::Assistant, "四"),
            ]
        );
    }

    #[test]
    fn cannot_send_while_waiting_or_with_blank_text() {
        let mut c = Conversation::new();
        assert_eq!(c.send("  \n "), Err(Rejected::Empty));
        c.send("一").unwrap();
        assert_eq!(c.pending(), Some(Pending::Reply));
        assert_eq!(c.send("二"), Err(Rejected::Busy));
        assert_eq!(c.request_draft(), Err(Rejected::Busy));
    }

    #[test]
    fn failure_returns_the_sent_text_and_keeps_the_session() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        c.receive(reply("二", "s-1"));
        c.send("三").unwrap();

        let got = c.receive(Err(AskError::TimedOut(std::time::Duration::from_secs(5))));

        assert_eq!(
            got,
            Received::Failed {
                restore: Some("三".into())
            }
        );
        assert_eq!(c.messages().len(), 2);
        assert_eq!(c.pending(), None);
        assert_eq!(c.session_id(), Some("s-1"));
        assert!(matches!(c.notice(), Some(Notice::Error(m)) if m.contains("5 秒")));
        // 戻した文をもう一度送れる
        assert_eq!(c.send("三").unwrap().resume.as_deref(), Some("s-1"));
    }

    #[test]
    fn sending_again_clears_the_previous_error() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        c.receive(Err(AskError::NotFound("claude".into())));
        assert!(c.notice().is_some());
        c.send("一").unwrap();
        assert_eq!(c.notice(), None);
    }

    #[test]
    fn draft_needs_at_least_one_reply() {
        let mut c = Conversation::new();
        assert!(!c.can_draft());
        assert_eq!(c.request_draft(), Err(Rejected::Empty));
        c.send("一").unwrap();
        assert!(!c.can_draft());
        c.receive(reply("二", "s"));
        assert!(c.can_draft());
    }

    #[test]
    fn draft_is_asked_in_the_same_session_and_handed_back_for_saving() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        c.receive(reply("二", "s-1"));

        let ask = c.request_draft().unwrap();
        assert_eq!(ask.resume.as_deref(), Some("s-1"));
        assert_eq!(ask.prompt, prompt::DRAFT);
        assert_eq!(c.pending(), Some(Pending::Draft));

        let got = c.receive(reply("```markdown\n# 今回の条件: x\n```", "s-2"));
        assert_eq!(got, Received::Draft("# 今回の条件: x\n".into()));
        assert_eq!(c.messages().last().unwrap().role, Role::Assistant);

        c.saved(PathBuf::from("/tmp/x.md"), vec![]);
        assert!(matches!(c.notice(), Some(Notice::Saved { .. })));
    }

    #[test]
    fn failed_draft_keeps_the_conversation() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        c.receive(reply("二", "s-1"));
        c.request_draft().unwrap();

        let got = c.receive(Err(AskError::Failed("x".into())));

        assert_eq!(got, Received::Failed { restore: None });
        assert_eq!(c.messages().len(), 2);
        assert!(c.can_draft());
    }
}
