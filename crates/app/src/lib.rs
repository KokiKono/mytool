use gpui::{Context, IntoElement, Render, SharedString, Window, div, prelude::*, rgb};

pub struct Shell {
    title: SharedString,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            title: "mytool".into(),
        }
    }

    pub fn title(&self) -> &SharedString {
        &self.title
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xe6e6e6))
            .flex()
            .items_center()
            .justify_center()
            .child(self.title.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn shell_shows_app_name(cx: &mut TestAppContext) {
        let shell = cx.new(|_| Shell::new());
        shell.read_with(cx, |shell, _| assert_eq!(shell.title().as_ref(), "mytool"));
    }
}
