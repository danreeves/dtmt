use color_eyre::Report;
use druid::widget::{Button, CrossAxisAlignment, Flex, Label, LineBreaking, MainAxisAlignment};
use druid::{Data, WidgetExt, WindowDesc, WindowHandle, WindowLevel, WindowSizePolicy};

const ERROR_DIALOG_SIZE: (f64, f64) = (750., 400.);

pub fn error<T: Data>(err: Report, parent: WindowHandle) -> WindowDesc<T> {
    let msg = format!("A critical error ocurred: {:?}", err);
    let stripped =
        strip_ansi_escapes::strip(msg.as_bytes()).expect("failed to strip ANSI in error");
    let msg = String::from_utf8_lossy(&stripped);

    let text = Label::new(msg.to_string()).with_line_break_mode(LineBreaking::WordWrap);

    let button = Button::new("Ok")
        .on_click(|ctx, _, _| {
            ctx.window().close();
        })
        .align_right();

    let widget = Flex::column()
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .cross_axis_alignment(CrossAxisAlignment::End)
        .with_child(text)
        .with_spacer(20.)
        .with_child(button)
        .padding(10.);

    WindowDesc::new(widget)
        .title("Error")
        .with_min_size(ERROR_DIALOG_SIZE)
        .resizable(false)
        .window_size_policy(WindowSizePolicy::Content)
        .set_always_on_top(true)
        .set_level(WindowLevel::Modal(parent))
}
