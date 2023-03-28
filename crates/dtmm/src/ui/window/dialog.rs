use color_eyre::{Handler, HelpInfo, Report};
use druid::widget::{CrossAxisAlignment, Flex, Label, LineBreaking};
use druid::{Data, WidgetExt, WindowDesc, WindowHandle};

use crate::ui::theme;
use crate::ui::widget::button::Button;

const WINDOW_SIZE: (f64, f64) = (600., 250.);

pub fn error<T: Data>(err: Report, _parent: WindowHandle) -> WindowDesc<T> {
    let (title, msg) = {
        let count = err.chain().count();

        if count == 1 {
            (
                String::from("An error occurred!"),
                err.root_cause().to_string(),
            )
        } else {
            let first = err.chain().next().unwrap();
            let root = err.root_cause();

            if count > 2 {
                // The second to last one, the context to the root cause
                let context = err.chain().nth(count - 2).unwrap();

                (format!("{first}!"), format!("{}: {}", context, root))
            } else {
                (format!("{first}!"), root.to_string())
            }
        }
    };

    let title = Label::new(title)
        .with_text_size(24.)
        .with_text_color(theme::COLOR_RED_LIGHT);
    let text = Label::new(msg).with_line_break_mode(LineBreaking::WordWrap);

    let button = Button::with_label("Ok")
        .on_click(|ctx, _, _| {
            ctx.window().close();
        })
        .align_right();

    let mut widget = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(title)
        .with_default_spacer()
        .with_child(text);

    if let Some(handler) = err.handler().downcast_ref::<Handler>() {
        let mut first = true;
        for section in handler.sections() {
            if let HelpInfo::Suggestion(data, _) = section {
                if first {
                    widget.add_default_spacer();
                    first = false;
                }

                let w = Flex::row()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .with_child(Label::new("Suggestion:").with_text_color(theme::COLOR_GREEN_LIGHT))
                    .with_spacer(2.)
                    .with_child(
                        Label::new(data.to_string()).with_line_break_mode(LineBreaking::WordWrap),
                    );

                widget.add_child(w);
            }
        }
    }

    let widget = widget.with_flex_spacer(1.).with_child(button).padding(10.);

    WindowDesc::new(widget)
        .title("Critical Error")
        .show_titlebar(true)
        .with_min_size(WINDOW_SIZE)
        .set_always_on_top(true)
        .resizable(false)
}
