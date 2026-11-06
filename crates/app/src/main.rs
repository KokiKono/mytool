use std::sync::Arc;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use mytool::composer::Composer;
use mytool::{demo, text_area};
use request::Assistant;
use request::claude::ClaudeCli;
use request::store::{self, RequestStore};

fn main() {
    let data_dir = store::data_dir();
    // MYTOOL_DEMO があれば偽の AI で動かし、決めておいた操作を流す（スクリーンショット用）
    let (assistant, steps): (Arc<dyn Assistant>, _) = match std::env::var("MYTOOL_DEMO") {
        Ok(name) => {
            let (assistant, steps) =
                demo::scenario(&name).unwrap_or_else(|| panic!("MYTOOL_DEMO={name} は知らない値"));
            (Arc::new(assistant), steps)
        }
        Err(_) => (Arc::new(ClaudeCli::new(data_dir.join("claude"))), vec![]),
    };
    let store = RequestStore::new(&data_dir);

    Application::new().run(move |cx: &mut App| {
        text_area::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(760.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
        cx.activate(true);
    });
}
