use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use gpui::{Entity, EntityInputHandler, TestAppContext, VisualTestContext};
use request::store::RequestStore;
use request::{Ask, AskError, Assistant, Notice, Pending, Reply, Role, prompt};

use crate::composer::Composer;
use crate::demo::{Behavior, ScriptedAssistant};
use crate::text_area;

/// 本物の `claude` の代わりに置く、テストが返答の時機を決める偽の AI。
struct GatedAssistant {
    asked: mpsc::Sender<Ask>,
    replies: std::sync::Mutex<mpsc::Receiver<Result<Reply, AskError>>>,
}

impl Assistant for GatedAssistant {
    fn ask(&self, ask: &Ask) -> Result<Reply, AskError> {
        self.asked.send(ask.clone()).unwrap();
        self.replies.lock().unwrap().recv().unwrap()
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    assistant: Arc<dyn Assistant>,
    data_dir: &std::path::Path,
) -> (Entity<Composer>, &'a mut VisualTestContext) {
    cx.update(text_area::bind_keys);
    let store = RequestStore::new(data_dir);
    let (composer, cx) = cx.add_window_view(|window, cx| {
        let composer = Composer::new(assistant, store, cx);
        composer.focus_input(window, cx);
        composer
    });
    cx.run_until_parked();
    (composer, cx)
}

/// 偽の AI は別スレッドで応えるので、返答が画面に届くまで回す。
fn settle(composer: &Entity<Composer>, cx: &mut VisualTestContext) {
    for _ in 0..500 {
        cx.run_until_parked();
        if composer.read_with(cx, |c, _| c.conversation().pending().is_none()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("返答が届かなかった");
}

fn input_text(composer: &Entity<Composer>, cx: &mut VisualTestContext) -> String {
    composer.read_with(cx, |c, cx| c.input().read(cx).text().to_string())
}

fn reply(text: &'static str) -> Behavior {
    Behavior::Reply(text)
}

#[gpui::test]
fn starts_with_an_empty_focused_input(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let (composer, cx) = open(cx, Arc::new(ScriptedAssistant::new(vec![])), tmp.path());

    cx.simulate_input("abc");

    assert_eq!(input_text(&composer, cx), "abc");
    composer.read_with(cx, |c, _| assert!(c.conversation().messages().is_empty()));
}

#[gpui::test]
fn enter_adds_a_newline_and_cmd_enter_sends(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Arc::new(ScriptedAssistant::new(vec![reply("返答")]));
    let (composer, cx) = open(cx, fake.clone(), tmp.path());

    cx.simulate_input("ab");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("cd");
    assert_eq!(input_text(&composer, cx), "ab\ncd");
    assert!(fake.asked().is_empty());

    cx.simulate_keystrokes("cmd-enter");
    settle(&composer, cx);

    assert_eq!(fake.asked()[0].prompt, "ab\ncd");
    assert_eq!(input_text(&composer, cx), "");
    composer.read_with(cx, |c, _| {
        let log: Vec<_> = c
            .conversation()
            .messages()
            .iter()
            .map(|m| (m.role, m.text.clone()))
            .collect();
        assert_eq!(
            log,
            [
                (Role::User, "ab\ncd".to_string()),
                (Role::Assistant, "返答".to_string())
            ]
        );
    });
}

#[gpui::test]
fn enter_while_composing_neither_sends_nor_adds_a_newline(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Arc::new(ScriptedAssistant::new(vec![reply("返答")]));
    let (composer, cx) = open(cx, fake.clone(), tmp.path());
    let input = composer.read_with(cx, |c, _| c.input().clone());

    // IME が変換中の文字を置いた状態
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "にほん", None, window, cx)
        })
    });
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();

    assert_eq!(input_text(&composer, cx), "にほん");
    assert!(fake.asked().is_empty());

    // IME が確定した後は、Enter で改行が入る
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "日本", window, cx)
        })
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(input_text(&composer, cx), "日本\n");
}

#[gpui::test]
fn shows_waiting_and_keeps_accepting_input_until_the_reply_arrives(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let (asked_tx, asked_rx) = mpsc::channel();
    let (reply_tx, reply_rx) = mpsc::channel();
    let fake = Arc::new(GatedAssistant {
        asked: asked_tx,
        replies: reply_rx.into(),
    });
    let (composer, cx) = open(cx, fake, tmp.path());

    cx.simulate_input("要望");
    cx.simulate_keystrokes("cmd-enter");
    asked_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    // 返答はまだ来ていない。そのあいだも入力欄は打てる
    composer.read_with(cx, |c, _| {
        assert_eq!(c.conversation().pending(), Some(Pending::Reply))
    });
    cx.simulate_input("次の文");
    assert_eq!(input_text(&composer, cx), "次の文");

    reply_tx
        .send(Ok(Reply {
            text: "質問です".into(),
            session_id: "s-1".into(),
        }))
        .unwrap();
    settle(&composer, cx);

    composer.read_with(cx, |c, _| {
        let last = c.conversation().messages().last().unwrap();
        assert_eq!(
            (last.role, last.text.as_str()),
            (Role::Assistant, "質問です")
        );
    });
    assert_eq!(input_text(&composer, cx), "次の文");
}

#[gpui::test]
fn second_message_resumes_the_first_session(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Arc::new(ScriptedAssistant::new(vec![reply("一"), reply("二")]));
    let (composer, cx) = open(cx, fake.clone(), tmp.path());

    cx.simulate_input("最初");
    cx.simulate_keystrokes("cmd-enter");
    settle(&composer, cx);
    cx.simulate_input("続き");
    cx.simulate_keystrokes("cmd-enter");
    settle(&composer, cx);

    let asked = fake.asked();
    assert_eq!(asked[0].resume, None);
    assert_eq!(asked[1].resume.as_deref(), Some("fake-session-1"));
}

#[gpui::test]
fn failure_shows_an_error_and_puts_the_text_back(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Arc::new(ScriptedAssistant::new(vec![Behavior::Fail(
        AskError::NotFound("claude".into()),
    )]));
    let (composer, cx) = open(cx, fake, tmp.path());

    cx.simulate_input("消えては困る文");
    cx.simulate_keystrokes("cmd-enter");
    settle(&composer, cx);

    assert_eq!(input_text(&composer, cx), "消えては困る文");
    composer.read_with(cx, |c, _| {
        assert!(c.conversation().messages().is_empty());
        assert!(
            matches!(c.conversation().notice(), Some(Notice::Error(m)) if m.contains("claude が見つからない"))
        );
    });
}

#[gpui::test]
fn draft_is_saved_as_markdown_and_its_path_is_shown(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let doc =
        "# 今回の条件: x\n\n## ゴール\n\n## 受け入れ基準\n\n1. a\n\n## やらないこと\n\n## 上限\n";
    let fake = Arc::new(ScriptedAssistant::new(vec![reply("質問"), reply(doc)]));
    let (composer, cx) = open(cx, fake.clone(), tmp.path());

    cx.simulate_input("要望");
    cx.simulate_keystrokes("cmd-enter");
    settle(&composer, cx);
    composer.update(cx, |c, cx| c.draft(cx));
    settle(&composer, cx);

    assert_eq!(fake.asked()[1].prompt, prompt::DRAFT);
    assert_eq!(fake.asked()[1].resume.as_deref(), Some("fake-session-1"));
    let (path, missing) = composer.read_with(cx, |c, _| match c.conversation().notice() {
        Some(Notice::Saved { path, missing }) => (path.clone(), missing.clone()),
        other => panic!("{other:?}"),
    });
    assert!(path.starts_with(tmp.path().join("requests")));
    assert_eq!(path.extension().unwrap(), "md");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), doc);
    assert!(missing.is_empty());
}

#[gpui::test]
fn demo_scenarios_reach_their_screens(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    for (name, messages) in [("two-turns", 4), ("error", 2), ("saved", 5)] {
        let (fake, steps) = crate::demo::scenario(name).unwrap();
        let (composer, vcx) = open(cx, Arc::new(fake), tmp.path());
        composer.update(vcx, |c, cx| c.run_script(steps, cx));
        for _ in 0..4 {
            settle(&composer, vcx);
        }
        composer.read_with(vcx, |c, _| {
            assert_eq!(c.conversation().messages().len(), messages, "{name}");
        });
    }
    assert!(crate::demo::scenario("unknown").is_none());
}
