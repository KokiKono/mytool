use std::sync::Arc;

use gpui::{
    App, Application, Bounds, WindowBounds, WindowKind, WindowOptions, prelude::*, px, size,
};
use mytool::composer::Composer;
use mytool::{demo, text_area};
use request::Assistant;
use request::claude::ClaudeCli;
use request::store::{self, RequestStore};

fn main() {
    let demo = std::env::var("MYTOOL_DEMO").ok();
    // デモはスクリーンショットを撮るための起動で、HOOTL が裏で何度も行う。人の作業中にキー入力を奪わないよう、
    // アプリを前面にしないパネルにする。ただの背面のウィンドウだと他のアプリに隠れ、macOS が描き直さなくなる
    let foreground = demo.is_none();
    // デモの作り物の条件書を本物の置き場に混ぜない。保存先のパスは画面に出るので、ホームの外にする
    let data_dir = match &demo {
        Some(_) if std::env::var_os("MYTOOL_DATA_DIR").is_none() => "/tmp/mytool-demo".into(),
        _ => store::data_dir(),
    };
    // MYTOOL_DEMO があれば偽の AI で動かし、決めておいた操作を流す（スクリーンショット用）
    let (assistant, steps): (Arc<dyn Assistant>, _) = match demo {
        Some(name) => {
            let (assistant, steps) =
                demo::scenario(&name).unwrap_or_else(|| panic!("MYTOOL_DEMO={name} は知らない値"));
            (Arc::new(assistant), steps)
        }
        None => (Arc::new(ClaudeCli::new(data_dir.join("claude"))), vec![]),
    };
    let store = RequestStore::new(&data_dir);

    Application::new().run(move |cx: &mut App| {
        text_area::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(760.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                focus: foreground,
                kind: if foreground {
                    WindowKind::Normal
                } else {
                    WindowKind::PopUp
                },
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let mut composer = Composer::new(assistant, store, cx);
                    composer.focus_input(window, cx);
                    composer.run_script(steps, cx);
                    composer
                })
            },
        )
        .expect("ウィンドウを開けなかった");
        if foreground {
            cx.activate(true);
        }
    });
}
