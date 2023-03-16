use color_eyre::Report;
use druid::widget::{CrossAxisAlignment, Flex, Label, LineBreaking, MainAxisAlignment};
use druid::{Data, WidgetExt, WindowDesc, WindowHandle, WindowSizePolicy};

use crate::ui::widget::button::Button;

pub fn error<T: Data>(err: Report, _parent: WindowHandle) -> WindowDesc<T> {
    let msg = {
        let msg = format!("A critical error ocurred: {:?}", err);
        if let Ok(stripped) = strip_ansi_escapes::strip(msg.as_bytes()) {
            String::from_utf8_lossy(&stripped).to_string()
        } else {
            msg
        }
    };

    let text = Label::new(msg).with_line_break_mode(LineBreaking::WordWrap);

    let button = Button::with_label("Ok")
        .on_click(|ctx, _, _| {
            ctx.window().close();
        })
        .align_right();

    let widget = Flex::column()
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .must_fill_main_axis(true)
        .cross_axis_alignment(CrossAxisAlignment::End)
        .with_child(text)
        .with_spacer(20.)
        .with_child(button)
        .padding(10.);

    WindowDesc::new(widget)
        .title("Error")
        .show_titlebar(true)
        .window_size_policy(WindowSizePolicy::Content)
        .set_always_on_top(true)
        .resizable(false)
}
