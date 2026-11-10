use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::channel::oneshot;
use futures::future::BoxFuture;
use gpui::{Entity, EntityInputHandler, TestAppContext, VisualTestContext};
use request::store::{RequestStore, Store};
use request::{Ask, AskError, Notice, Pending, Reply, Role, prompt};

use crate::asker::Asker;
use crate::composer::Composer;
use crate::demo::{Behavior, ScriptedAssistant};
use crate::text_area;

type Answer = oneshot::Sender<Result<Reply, AskError>>;

/// 返答を届ける時機をテストが決める偽の AI。届けたら `run_until_parked` で画面まで進む。
#[derive(Default)]
struct GatedAsker {
    waiting: RefCell<Vec<(Ask, Answer)>>,
}

impl GatedAsker {
    fn asked(&self) -> Vec<Ask> {
        self.waiting
            .borrow()
            .iter()
            .map(|(a, _)| a.clone())
            .collect()
    }

    fn respond(&self, result: Result<Reply, AskError>) {
        let (_, tx) = self.waiting.borrow_mut().remove(0);
        tx.send(result).unwrap();
    }
}

impl Asker for GatedAsker {
    fn ask(&self, ask: Ask) -> BoxFuture<'static, Result<Reply, AskError>> {
        let (tx, rx) = oneshot::channel();
        self.waiting.borrow_mut().push((ask, tx));
        Box::pin(async move { rx.await.unwrap() })
    }
}

/// 保存に必ず失敗する保存先。
struct BrokenStore;

impl Store for BrokenStore {
    fn save(&self, _: &str) -> io::Result<PathBuf> {
        Err(io::Error::other("ディスクがいっぱい"))
    }
}

fn open_with(
    cx: &mut TestAppContext,
    asker: Rc<dyn Asker>,
    store: Box<dyn Store>,
) -> (Entity<Composer>, &mut VisualTestContext) {
    cx.update(text_area::bind_keys);
    let (composer, cx) = cx.add_window_view(|window, cx| {
        let composer = Composer::new(asker, store, cx);
        composer.focus_input(window, cx);
        composer
    });
    cx.run_until_parked();
    (composer, cx)
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    asker: Rc<dyn Asker>,
    data_dir: &Path,
) -> (Entity<Composer>, &'a mut VisualTestContext) {
    open_with(cx, asker, Box::new(RequestStore::new(data_dir)))
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
    let (composer, cx) = open(cx, Rc::new(ScriptedAssistant::new(vec![])), tmp.path());

    cx.simulate_input("abc");

    assert_eq!(input_text(&composer, cx), "abc");
    composer.read_with(cx, |c, _| assert!(c.conversation().messages().is_empty()));
}

#[gpui::test]
fn enter_adds_a_newline_and_cmd_enter_sends(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Rc::new(ScriptedAssistant::new(vec![reply("返答")]));
    let (composer, cx) = open(cx, fake.clone(), tmp.path());

    cx.simulate_input("ab");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("cd");
    assert_eq!(input_text(&composer, cx), "ab\ncd");
    assert!(fake.asked().is_empty());

    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();

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
                (Role::User, "ab\ncd".into()),
                (Role::Assistant, "返答".into())
            ]
        );
    });
}

#[gpui::test]
fn enter_while_composing_neither_sends_nor_adds_a_newline(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Rc::new(ScriptedAssistant::new(vec![reply("返答")]));
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
    let fake = Rc::new(GatedAsker::default());
    let (composer, cx) = open(cx, fake.clone(), tmp.path());

    cx.simulate_input("要望");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();
    assert_eq!(fake.asked().len(), 1);

    // 返答はまだ来ていない。そのあいだも入力欄は打てる
    composer.read_with(cx, |c, _| {
        assert_eq!(c.conversation().pending(), Some(Pending::Reply))
    });
    cx.simulate_input("次の文");
    assert_eq!(input_text(&composer, cx), "次の文");

    fake.respond(Ok(Reply {
        text: "質問です".into(),
        session_id: "s-1".into(),
    }));
    cx.run_until_parked();

    composer.read_with(cx, |c, _| {
        let last = c.conversation().messages().last().unwrap();
        assert_eq!((last.role, &*last.text), (Role::Assistant, "質問です"));
    });
    assert_eq!(input_text(&composer, cx), "次の文");
}

#[gpui::test]
fn failure_puts_the_sent_text_before_what_was_typed_while_waiting(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Rc::new(GatedAsker::default());
    let (composer, cx) = open(cx, fake.clone(), tmp.path());

    cx.simulate_input("送った文");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();
    cx.simulate_input("書き足した文");

    fake.respond(Err(AskError::TimedOut(std::time::Duration::from_millis(
        300,
    ))));
    cx.run_until_parked();

    assert_eq!(input_text(&composer, cx), "送った文\n書き足した文");
    composer.read_with(cx, |c, _| {
        assert!(
            matches!(c.conversation().notice(), Some(Notice::Error(m)) if m.contains("0.3 秒"))
        );
    });
}

#[gpui::test]
fn second_message_resumes_the_first_session(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Rc::new(ScriptedAssistant::new(vec![reply("一"), reply("二")]));
    let (_, cx) = open(cx, fake.clone(), tmp.path());

    cx.simulate_input("最初");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();
    cx.simulate_input("続き");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();

    let asked = fake.asked();
    assert_eq!(asked[0].resume, None);
    assert_eq!(asked[1].resume.as_deref(), Some("fake-session-1"));
}

#[gpui::test]
fn failure_shows_an_error_and_puts_the_text_back(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Rc::new(ScriptedAssistant::new(vec![Behavior::Fail(
        AskError::NotFound("claude".into()),
    )]));
    let (composer, cx) = open(cx, fake, tmp.path());

    cx.simulate_input("消えては困る文");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();

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
    let fake = Rc::new(ScriptedAssistant::new(vec![reply("質問"), reply(doc)]));
    let (composer, cx) = open(cx, fake.clone(), tmp.path());

    cx.simulate_input("要望");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();
    composer.update(cx, |c, cx| c.draft(cx));
    cx.run_until_parked();

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
fn draft_that_cannot_be_saved_shows_an_error(cx: &mut TestAppContext) {
    let fake = Rc::new(ScriptedAssistant::new(vec![
        reply("質問"),
        reply("# 今回の条件: x\n"),
    ]));
    let (composer, cx) = open_with(cx, fake, Box::new(BrokenStore));

    cx.simulate_input("要望");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("notice").is_none());
    composer.update(cx, |c, cx| c.draft(cx));
    cx.run_until_parked();

    composer.read_with(cx, |c, _| {
        assert_eq!(
            c.conversation().notice(),
            Some(&Notice::Error(
                "条件書を保存できなかった: ディスクがいっぱい".into()
            ))
        );
        assert!(c.conversation().can_draft());
    });
    // エラーの知らせが画面に描かれている
    assert!(cx.debug_bounds("notice").is_some());
}

#[gpui::test]
fn demo_scenarios_reach_their_screens(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    for (name, messages) in [("two-turns", 4), ("error", 2), ("saved", 5)] {
        let (fake, steps) = crate::demo::scenario(name).unwrap();
        let (composer, vcx) = open(cx, Rc::new(fake), tmp.path());
        composer.update(vcx, |c, cx| c.run_script(steps, cx));
        vcx.run_until_parked();
        composer.read_with(vcx, |c, _| {
            assert_eq!(c.conversation().messages().len(), messages, "{name}");
        });
    }
    assert!(crate::demo::scenario("unknown").is_none());
}
