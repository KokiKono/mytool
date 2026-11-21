use std::path::PathBuf;
use std::sync::Arc;

use crate::store::Store;
use crate::{Ask, AskError, Reply, prompt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    /// 末尾の空白を落とした文。画面は描くたびにこれを複製せず参照だけ増やす
    pub text: Arc<str>,
}

impl Message {
    fn new(role: Role, text: &str) -> Self {
        Self {
            role,
            text: text.trim_end().into(),
        }
    }
}

/// 返答を待っている問い合わせの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Reply,
    Draft,
}

/// 待っているあいだに持つもの。送った原文は発言の返答を待つときにしか要らないので、そこにだけ持たせる
#[derive(Debug)]
enum Waiting {
    /// 失敗したら、末尾の空白も含めて送った原文をそのまま入力欄に戻す
    Reply {
        sent: String,
    },
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
    /// 条件書を受け取って保存を試みた。結果は [`Conversation::notice`] に出る
    Draft,
    /// 送った文を、[`restore_input`] で入力欄に戻す
    Failed {
        restore: Option<String>,
    },
}

/// 返答が失敗して戻ってきた `sent` と、待つあいだに入力欄へ書き足した `typed` をつなぐ。
/// 書き足した分は消さず、戻した文の後ろに残す。
pub fn restore_input(sent: String, typed: &str) -> String {
    if typed.is_empty() {
        sent
    } else {
        format!("{sent}\n{typed}")
    }
}

#[derive(Debug, Default)]
pub struct Conversation {
    messages: Vec<Message>,
    session_id: Option<String>,
    waiting: Option<Waiting>,
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
        self.waiting.as_ref().map(|w| match w {
            Waiting::Reply { .. } => Pending::Reply,
            Waiting::Draft => Pending::Draft,
        })
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn can_draft(&self) -> bool {
        self.waiting.is_none() && self.messages.iter().any(|m| m.role == Role::Assistant)
    }

    /// 発言を会話に足し、AI に渡す問い合わせを返す。
    pub fn send(&mut self, text: &str) -> Result<Ask, Rejected> {
        if self.waiting.is_some() {
            return Err(Rejected::Busy);
        }
        if text.trim().is_empty() {
            return Err(Rejected::Empty);
        }
        self.messages.push(Message::new(Role::User, text));
        self.waiting = Some(Waiting::Reply {
            sent: text.to_string(),
        });
        self.notice = None;
        Ok(Ask {
            prompt: text.to_string(),
            resume: self.session_id.clone(),
        })
    }

    /// ここまでのやり取りから条件書を書かせる問い合わせを返す。
    pub fn request_draft(&mut self) -> Result<Ask, Rejected> {
        if self.waiting.is_some() {
            return Err(Rejected::Busy);
        }
        if !self.can_draft() {
            return Err(Rejected::Empty);
        }
        self.waiting = Some(Waiting::Draft);
        self.notice = None;
        Ok(Ask {
            prompt: prompt::DRAFT.to_string(),
            resume: self.session_id.clone(),
        })
    }

    /// 届いた返答を会話に足す。条件書なら `store` に保存し、足りない見出しを調べて知らせる。
    pub fn receive(&mut self, result: Result<Reply, AskError>, store: &dyn Store) -> Received {
        let Some(waiting) = self.waiting.take() else {
            // 待っていないのに届いた返答は捨てる
            return Received::Reply;
        };
        if let Ok(reply) = &result {
            self.session_id = Some(reply.session_id.clone());
        }
        match (waiting, result) {
            (Waiting::Reply { .. }, Ok(reply)) => {
                self.messages
                    .push(Message::new(Role::Assistant, &reply.text));
                Received::Reply
            }
            (Waiting::Draft, Ok(reply)) => {
                let markdown = prompt::strip_fence(&reply.text);
                self.messages.push(Message::new(Role::Assistant, &markdown));
                self.notice = Some(match store.save(&markdown) {
                    Ok(path) => Notice::Saved {
                        path,
                        missing: prompt::missing_sections(&markdown),
                    },
                    Err(error) => Notice::Error(format!("条件書を保存できなかった: {error}")),
                });
                Received::Draft
            }
            (Waiting::Reply { sent }, Err(error)) => {
                // 届かなかった発言は会話から外して入力欄に戻す。残すと次に送ったとき二重になる
                self.messages.pop();
                self.notice = Some(Notice::Error(error.to_string()));
                Received::Failed {
                    restore: Some(sent),
                }
            }
            (Waiting::Draft, Err(error)) => {
                self.notice = Some(Notice::Error(error.to_string()));
                Received::Failed { restore: None }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io;

    /// 保存した中身を覚えておく偽の保存先。`fail` なら保存に失敗する。
    #[derive(Default)]
    struct FakeStore {
        fail: bool,
        saved: RefCell<Vec<String>>,
    }

    impl Store for FakeStore {
        fn save(&self, markdown: &str) -> io::Result<PathBuf> {
            if self.fail {
                return Err(io::Error::other("書けない"));
            }
            self.saved.borrow_mut().push(markdown.to_string());
            Ok(PathBuf::from("/tmp/x.md"))
        }
    }

    fn receive_with_fake(c: &mut Conversation, result: Result<Reply, AskError>) -> Received {
        c.receive(result, &FakeStore::default())
    }

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
        receive_with_fake(&mut c, reply("何を保存しますか", "s-1"));

        let second = c.send("テキストだけ").unwrap();
        assert_eq!(second.resume.as_deref(), Some("s-1"));
        receive_with_fake(&mut c, reply("分かりました", "s-2"));

        let third = c.send("以上").unwrap();
        assert_eq!(third.resume.as_deref(), Some("s-2"));
    }

    #[test]
    fn messages_are_kept_in_the_order_they_happened() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        receive_with_fake(&mut c, reply("二", "s"));
        c.send("三").unwrap();
        receive_with_fake(&mut c, reply("四", "s"));
        let log: Vec<_> = c.messages().iter().map(|m| (m.role, &*m.text)).collect();
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
        receive_with_fake(&mut c, reply("二", "s-1"));
        c.send("三").unwrap();

        let got = receive_with_fake(
            &mut c,
            Err(AskError::TimedOut(std::time::Duration::from_secs(5))),
        );

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
        receive_with_fake(&mut c, Err(AskError::NotFound("claude".into())));
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
        receive_with_fake(&mut c, reply("二", "s"));
        assert!(c.can_draft());
    }

    #[test]
    fn draft_is_asked_in_the_same_session_saved_and_checked_for_missing_sections() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        receive_with_fake(&mut c, reply("二", "s-1"));

        let ask = c.request_draft().unwrap();
        assert_eq!(ask.resume.as_deref(), Some("s-1"));
        assert_eq!(ask.prompt, prompt::DRAFT);
        assert_eq!(c.pending(), Some(Pending::Draft));

        let store = FakeStore::default();
        let got = c.receive(
            reply("```markdown\n# 今回の条件: x\n\n## ゴール\n```", "s-2"),
            &store,
        );

        assert_eq!(got, Received::Draft);
        assert_eq!(*store.saved.borrow(), ["# 今回の条件: x\n\n## ゴール\n"]);
        assert_eq!(
            &*c.messages().last().unwrap().text,
            "# 今回の条件: x\n\n## ゴール"
        );
        assert_eq!(
            c.notice(),
            Some(&Notice::Saved {
                path: PathBuf::from("/tmp/x.md"),
                missing: vec!["## 受け入れ基準", "## やらないこと", "## 上限"],
            })
        );
        assert!(c.can_draft());
    }

    #[test]
    fn draft_that_cannot_be_saved_shows_an_error_and_keeps_the_conversation() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        receive_with_fake(&mut c, reply("二", "s-1"));
        c.request_draft().unwrap();

        let store = FakeStore {
            fail: true,
            ..Default::default()
        };
        let got = c.receive(reply("# 今回の条件: x\n", "s-2"), &store);

        assert_eq!(got, Received::Draft);
        assert!(
            matches!(c.notice(), Some(Notice::Error(m)) if m == "条件書を保存できなかった: 書けない")
        );
        assert_eq!(c.messages().len(), 3);
        assert!(c.can_draft());
    }

    #[test]
    fn failed_send_gives_back_the_text_exactly_as_typed() {
        let mut c = Conversation::new();
        c.send("一\n\n").unwrap();
        assert_eq!(&*c.messages()[0].text, "一");

        let got = receive_with_fake(&mut c, Err(AskError::NotFound("claude".into())));

        assert_eq!(
            got,
            Received::Failed {
                restore: Some("一\n\n".into())
            }
        );
    }

    #[test]
    fn restored_text_goes_before_what_was_typed_while_waiting() {
        assert_eq!(restore_input("送った文".into(), ""), "送った文");
        assert_eq!(
            restore_input("送った文".into(), "書き足した文"),
            "送った文\n書き足した文"
        );
        assert_eq!(restore_input("一\n".into(), "二"), "一\n\n二");
    }

    #[test]
    fn failed_draft_keeps_the_conversation() {
        let mut c = Conversation::new();
        c.send("一").unwrap();
        receive_with_fake(&mut c, reply("二", "s-1"));
        c.request_draft().unwrap();

        let got = receive_with_fake(&mut c, Err(AskError::Failed("x".into())));

        assert_eq!(got, Received::Failed { restore: None });
        assert_eq!(c.messages().len(), 2);
        assert!(c.can_draft());
    }
}
