//! 機能要望を AI と練り込んで条件書にする画面のうち、GPUI に依存しない部分。
//! 会話の状態遷移（[`Conversation`]）、`claude` CLI の呼び出しと出力のパース（[`claude`]）、
//! 条件書の保存（[`store`]）を持つ。

pub mod claude;
mod conversation;
pub mod prompt;
pub mod store;

pub use conversation::{Conversation, Message, Notice, Pending, Received, Rejected, Role};

use std::fmt;
use std::time::Duration;

/// AI に 1 回問い合わせる内容。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub prompt: String,
    /// 2 回目以降は前回の返答の `session_id`。会話の続きとして扱わせる
    pub resume: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskError {
    /// CLI が見つからない
    NotFound(String),
    /// CLI は動いたが、失敗を返した・出力が読めなかった
    Failed(String),
    TimedOut(Duration),
}

impl fmt::Display for AskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AskError::NotFound(program) => write!(f, "{program} が見つからない"),
            AskError::Failed(detail) => write!(f, "AI の呼び出しに失敗した: {detail}"),
            AskError::TimedOut(limit) => {
                write!(f, "AI の返答が {} 秒以内に来なかった", limit.as_secs())
            }
        }
    }
}

/// 要望を聞き返す相手。本物は [`claude::ClaudeCli`]、テストでは偽物に差し替える。
/// 呼び出しは返答が届くまで戻らないので、UI のスレッドからは呼ばない。
pub trait Assistant: Send + Sync {
    fn ask(&self, ask: &Ask) -> Result<Reply, AskError>;
}
