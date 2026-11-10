//! AI の返答を、UI のスレッドを塞がずに待つ。

use std::sync::Arc;

use futures::channel::oneshot;
use futures::future::BoxFuture;
use request::{Ask, AskError, Assistant, Reply};

/// 画面から見た AI。返答は future で受け取る。
/// 本物は [`OnThread`]、テストでは返答を届ける時機をテストが決める偽物に差し替える。
pub trait Asker {
    fn ask(&self, ask: Ask) -> BoxFuture<'static, Result<Reply, AskError>>;
}

/// 同期の [`Assistant`] を、問い合わせごとに立てる OS スレッドで呼ぶ。
/// 返答まで最長で数分かかるので、gpui の executor のスレッドでは待たない。
pub struct OnThread(pub Arc<dyn Assistant>);

impl Asker for OnThread {
    fn ask(&self, ask: Ask) -> BoxFuture<'static, Result<Reply, AskError>> {
        let (tx, rx) = oneshot::channel();
        let assistant = self.0.clone();
        std::thread::spawn(move || {
            let _ = tx.send(assistant.ask(&ask));
        });
        Box::pin(async move {
            rx.await
                .unwrap_or_else(|_| Err(AskError::Failed("AI の呼び出しが途中で止まった".into())))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    struct Echo;

    impl Assistant for Echo {
        fn ask(&self, ask: &Ask) -> Result<Reply, AskError> {
            Ok(Reply {
                text: ask.prompt.clone(),
                session_id: "s".into(),
            })
        }
    }

    struct Panics;

    impl Assistant for Panics {
        fn ask(&self, _: &Ask) -> Result<Reply, AskError> {
            panic!("偽の AI が落ちた")
        }
    }

    fn ask(prompt: &str) -> Ask {
        Ask {
            prompt: prompt.into(),
            resume: None,
        }
    }

    #[test]
    fn hands_the_reply_from_the_thread_to_the_future() {
        let reply = block_on(OnThread(Arc::new(Echo)).ask(ask("やあ")));
        assert_eq!(reply.unwrap().text, "やあ");
    }

    #[test]
    fn a_thread_that_dies_becomes_an_error() {
        let reply = block_on(OnThread(Arc::new(Panics)).ask(ask("やあ")));
        assert_eq!(
            reply,
            Err(AskError::Failed("AI の呼び出しが途中で止まった".into()))
        );
    }
}
