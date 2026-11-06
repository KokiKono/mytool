use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use mytool::Shell;

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(720.), px(480.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Shell::new()),
        )
        .expect("ウィンドウを開けなかった");
        cx.activate(true);
    });
}
