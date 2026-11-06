//! 本物の `claude` を呼ばずに画面の状態を作るための偽の AI と、作り物のやり取り。
//! `MYTOOL_DEMO` で起動したときと、テストで使う。

use std::sync::Mutex;
use std::time::Duration;

use request::{Ask, AskError, Assistant, Reply};

use crate::composer::Step;

/// 偽の AI が 1 回の問い合わせにどう応えるか。
#[derive(Clone, Debug)]
pub enum Behavior {
    Reply(&'static str),
    /// 返答しない。返答待ちの画面を撮るために使う
    Hang,
    Fail(AskError),
}

/// 決めておいた順に応える偽の AI。受け取った問い合わせを覚えておく。
pub struct ScriptedAssistant {
    behaviors: Mutex<Vec<Behavior>>,
    asked: Mutex<Vec<Ask>>,
}

impl ScriptedAssistant {
    pub fn new(behaviors: Vec<Behavior>) -> Self {
        Self {
            behaviors: Mutex::new(behaviors),
            asked: Mutex::new(Vec::new()),
        }
    }

    pub fn asked(&self) -> Vec<Ask> {
        self.asked.lock().unwrap().clone()
    }
}

impl Assistant for ScriptedAssistant {
    fn ask(&self, ask: &Ask) -> Result<Reply, AskError> {
        let n = {
            let mut asked = self.asked.lock().unwrap();
            asked.push(ask.clone());
            asked.len()
        };
        let behavior = {
            let mut behaviors = self.behaviors.lock().unwrap();
            if behaviors.is_empty() {
                Behavior::Reply("（偽の AI の返答はここまで）")
            } else {
                behaviors.remove(0)
            }
        };
        match behavior {
            Behavior::Reply(text) => Ok(Reply {
                text: text.into(),
                session_id: format!("fake-session-{n}"),
            }),
            Behavior::Hang => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
            Behavior::Fail(error) => Err(error),
        }
    }
}

const USER_1: &str =
    "クリップボードの履歴を見たい。\n直近のものから選んで貼り付けられるようにしたい。";
const AI_1: &str = "いくつか確かめさせてください。\n1. 履歴は何件まで残しますか（例: 100 件）\n2. 画像も対象にしますか、テキストだけですか\n3. アプリを終了しても履歴を残しますか";
const USER_2: &str = "テキストだけで 50 件。終了したら消えてよい。";
const AI_2: &str = "分かりました。受け入れ基準の案です。\n1. テキストをコピーすると履歴の先頭に入り、51 件目は消える\n2. 履歴の項目を選ぶと、その文字列がクリップボードに入る\n\n画像の履歴と、終了後も残す機能は「やらないこと」に回します。";
const DRAFT: &str = "# 今回の条件: クリップボードの履歴\n\n## ゴール\n\nコピーしたテキストを 50 件まで覚えておき、選んで貼り付け直せる。\n\n## 受け入れ基準\n\n1. テキストをコピーすると履歴の先頭に入り、51 件目は消える\n2. 履歴の項目を選ぶと、その文字列がクリップボードに入る\n\n## やらないこと\n\n- 画像の履歴\n- 終了後も履歴を残すこと\n\n## 上限\n\n- 2 時間、または scripts/verify.sh の実行 40 回\n";

/// `MYTOOL_DEMO` の値から、偽の AI と起動時に流す操作を作る。知らない値なら `None`。
pub fn scenario(name: &str) -> Option<(ScriptedAssistant, Vec<Step>)> {
    let two_sends = || vec![Step::Send(USER_1.into()), Step::Send(USER_2.into())];
    let (behaviors, steps) = match name {
        "idle" => (vec![], vec![]),
        "two-turns" => (
            vec![Behavior::Reply(AI_1), Behavior::Reply(AI_2)],
            two_sends(),
        ),
        "waiting" => (vec![Behavior::Reply(AI_1), Behavior::Hang], two_sends()),
        "error" => (
            vec![
                Behavior::Reply(AI_1),
                Behavior::Fail(AskError::TimedOut(Duration::from_secs(180))),
            ],
            two_sends(),
        ),
        "saved" => {
            let mut steps = two_sends();
            steps.push(Step::Draft);
            (
                vec![
                    Behavior::Reply(AI_1),
                    Behavior::Reply(AI_2),
                    Behavior::Reply(DRAFT),
                ],
                steps,
            )
        }
        _ => return None,
    };
    Some((ScriptedAssistant::new(behaviors), steps))
}
