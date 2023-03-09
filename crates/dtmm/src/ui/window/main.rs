use std::sync::Arc;

use druid::im::Vector;
use druid::widget::{
    Button, Checkbox, CrossAxisAlignment, Flex, Label, LineBreaking, List, MainAxisAlignment,
    Maybe, Scroll, SizedBox, Split, TextBox, ViewSwitcher,
};
use druid::{lens, LifeCycleCtx};
use druid::{
    Color, FileDialogOptions, FileSpec, FontDescriptor, FontFamily, Key, LensExt, SingleUse,
    TextAlignment, Widget, WidgetExt, WindowDesc, WindowId,
};
use lazy_static::lazy_static;

use crate::state::{
    ModInfo, State, View, ACTION_ADD_MOD, ACTION_SELECTED_MOD_DOWN, ACTION_SELECTED_MOD_UP,
    ACTION_SELECT_MOD, ACTION_SET_WINDOW_HANDLE, ACTION_START_DELETE_SELECTED_MOD,
    ACTION_START_DEPLOY, ACTION_START_RESET_DEPLOYMENT,
};
use crate::ui::theme;
use crate::ui::widget::controller::{AutoScrollController, DirtyStateController};
use crate::ui::widget::PathBufFormatter;

lazy_static! {
    pub static ref WINDOW_ID: WindowId = WindowId::next();
}

const TITLE: &str = "Darktide Mod Manager";
const WINDOW_SIZE: (f64, f64) = (1080., 720.);
const MOD_DETAILS_MIN_WIDTH: f64 = 325.;

const KEY_MOD_LIST_ITEM_BG_COLOR: Key<Color> = Key::new("dtmm.mod-list.item.background-color");

pub(crate) fn new() -> WindowDesc<State> {
    WindowDesc::new(build_window())
        .title(TITLE)
        .window_size(WINDOW_SIZE)
}

fn build_top_bar() -> impl Widget<State> {
    let mods_button = Button::new("Mods")
        .on_click(|_ctx, state: &mut State, _env| state.current_view = View::Mods);

    let settings_button = Button::new("Settings").on_click(|_ctx, state: &mut State, _env| {
        state.current_view = View::Settings;
    });

    let deploy_button = {
        Button::dynamic(|state: &State, _| {
            let mut s = String::new();
            if state.dirty {
                s.push_str("! ");
            }
            s.push_str("Deploy Mods");
            s
        })
        .on_click(|ctx, _state: &mut State, _env| {
            ctx.submit_command(ACTION_START_DEPLOY);
        })
        .disabled_if(|data, _| data.is_deployment_in_progress || data.is_reset_in_progress)
    };

    let reset_button = Button::new("Reset Game")
        .on_click(|ctx, _state: &mut State, _env| {
            ctx.submit_command(ACTION_START_RESET_DEPLOYMENT);
        })
        .disabled_if(|data, _| data.is_deployment_in_progress || data.is_reset_in_progress);

    Flex::row()
        .must_fill_main_axis(true)
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_child(
            Flex::row()
                .with_child(mods_button)
                .with_default_spacer()
                .with_child(settings_button),
        )
        .with_child(
            Flex::row()
                .with_child(deploy_button)
                .with_default_spacer()
                .with_child(reset_button),
        )
        .padding(theme::TOP_BAR_INSETS)
        .background(theme::TOP_BAR_BACKGROUND_COLOR)
    // TODO: Add bottom border. Need a custom widget for that, as the built-in only provides
    // uniform borders on all sides
}

fn build_mod_list() -> impl Widget<State> {
    let list = List::new(|| {
        let checkbox = Checkbox::new("")
            .lens(lens!((usize, Arc<ModInfo>, bool), 1).then(ModInfo::enabled.in_arc()));

        let name =
            Label::raw().lens(lens!((usize, Arc<ModInfo>, bool), 1).then(ModInfo::name.in_arc()));

        Flex::row()
            .must_fill_main_axis(true)
            .with_child(checkbox)
            .with_child(name)
            .padding((5.0, 4.0))
            .background(KEY_MOD_LIST_ITEM_BG_COLOR)
            .on_click(|ctx, (i, _, _), _env| ctx.submit_command(ACTION_SELECT_MOD.with(*i)))
            .env_scope(|env, (i, _, selected)| {
                if *selected {
                    env.set(KEY_MOD_LIST_ITEM_BG_COLOR, Color::NAVY);
                } else if (i % 2) == 1 {
                    env.set(KEY_MOD_LIST_ITEM_BG_COLOR, Color::WHITE.with_alpha(0.05));
                } else {
                    env.set(KEY_MOD_LIST_ITEM_BG_COLOR, Color::TRANSPARENT);
                }
            })
    });

    let scroll = Scroll::new(list).vertical().lens(lens::Identity.map(
        |state: &State| {
            state
                .mods
                .iter()
                .enumerate()
                .map(|(i, val)| (i, val.clone(), Some(i) == state.selected_mod_index))
                .collect::<Vector<_>>()
        },
        |state, infos| {
            infos.into_iter().for_each(|(i, new, _)| {
                if state.mods.get(i).cloned() != Some(new.clone()) {
                    state.mods.set(i, new);
                }
            });
        },
    ));

    Flex::column()
        .must_fill_main_axis(true)
        .with_child(Flex::row())
        .with_flex_child(scroll, 1.0)
}

fn build_mod_details_buttons() -> impl Widget<State> {
    let button_move_up = Button::new("Move Up")
        .on_click(|ctx, _state, _env| ctx.submit_command(ACTION_SELECTED_MOD_UP))
        .disabled_if(|state: &State, _env: &druid::Env| !state.can_move_mod_up());

    let button_move_down = Button::new("Move Down")
        .on_click(|ctx, _state, _env| ctx.submit_command(ACTION_SELECTED_MOD_DOWN))
        .disabled_if(|state: &State, _env: &druid::Env| !state.can_move_mod_down());

    let button_toggle_mod = Maybe::new(
        || {
            Button::dynamic(|enabled, _env| {
                if *enabled {
                    "Disable Mod".into()
                } else {
                    "Enable Mod".into()
                }
            })
            .on_click(|_ctx, enabled: &mut bool, _env| {
                *enabled = !(*enabled);
            })
            .lens(ModInfo::enabled.in_arc())
        },
        // TODO: Gray out
        || Button::new("Enable Mod"),
    )
    .disabled_if(|info: &Option<Arc<ModInfo>>, _env: &druid::Env| info.is_none())
    .lens(State::selected_mod);

    let button_add_mod = Button::new("Add Mod").on_click(|ctx, _state: &mut State, _env| {
        let zip = FileSpec::new("Zip file", &["zip"]);
        let opts = FileDialogOptions::new()
            .allowed_types(vec![zip])
            .default_type(zip)
            .name_label("Mod Archive")
            .title("Choose a mod to add")
            .accept_command(ACTION_ADD_MOD);
        ctx.submit_command(druid::commands::SHOW_OPEN_PANEL.with(opts))
    });

    let button_delete_mod = Button::new("Delete Mod")
        .on_click(|ctx, data: &mut Option<Arc<ModInfo>>, _env| {
            if let Some(info) = data {
                ctx.submit_command(
                    ACTION_START_DELETE_SELECTED_MOD.with(SingleUse::new(info.clone())),
                );
            }
        })
        .disabled_if(|info: &Option<Arc<ModInfo>>, _env: &druid::Env| info.is_none())
        .lens(State::selected_mod);

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Flex::row()
                .main_axis_alignment(MainAxisAlignment::End)
                .with_child(button_move_up)
                .with_default_spacer()
                .with_child(button_move_down),
        )
        .with_default_spacer()
        .with_child(
            Flex::row()
                .main_axis_alignment(MainAxisAlignment::End)
                .with_child(button_toggle_mod)
                .with_default_spacer()
                .with_child(button_add_mod)
                .with_default_spacer()
                .with_child(button_delete_mod),
        )
        .expand_width()
}

fn build_mod_details_info() -> impl Widget<State> {
    Maybe::new(
        || {
            let name = Label::raw()
                .with_text_alignment(TextAlignment::Center)
                .with_text_size(24.)
                // Force the label to take up the entire details' pane width,
                // so that we can center-align it.
                .expand_width()
                .lens(ModInfo::name.in_arc());
            let summary = Label::raw()
                .with_line_break_mode(LineBreaking::WordWrap)
                .lens(ModInfo::summary.in_arc());

            // TODO: Image/icon?

            let version_line = Label::dynamic(|info: &Arc<ModInfo>, _| {
                if let Some(author) = &info.author {
                    format!("Version: {}, by {author}", info.version)
                } else {
                    format!("Version: {}", info.version)
                }
            });

            let categories = Label::dynamic(|info: &Arc<ModInfo>, _| {
                if info.categories.is_empty() {
                    String::from("Uncategorized")
                } else {
                    info.categories.iter().enumerate().fold(
                        String::from("Category: "),
                        |mut s, (i, category)| {
                            if i > 0 {
                                s.push_str(", ");
                            }
                            s.push_str(category);
                            s
                        },
                    )
                }
            });

            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .main_axis_alignment(MainAxisAlignment::Start)
                .with_child(name)
                .with_spacer(4.)
                .with_child(summary)
                .with_spacer(4.)
                .with_child(version_line)
                .with_spacer(4.)
                .with_child(categories)
        },
        Flex::column,
    )
    .padding((4., 4.))
    .lens(State::selected_mod)
}

fn build_mod_details() -> impl Widget<State> {
    Flex::column()
        .must_fill_main_axis(true)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_flex_child(build_mod_details_info(), 1.0)
        .with_child(build_mod_details_buttons().padding(4.))
}

fn build_view_mods() -> impl Widget<State> {
    Split::columns(build_mod_list(), build_mod_details())
        .split_point(0.75)
        .min_size(0.0, MOD_DETAILS_MIN_WIDTH)
        .solid_bar(true)
        .bar_size(2.0)
        .draggable(true)
}

fn build_view_settings() -> impl Widget<State> {
    let data_dir_setting = Flex::row()
        .must_fill_main_axis(true)
        .main_axis_alignment(MainAxisAlignment::Start)
        .with_child(Label::new("Data Directory:"))
        .with_default_spacer()
        .with_flex_child(
            TextBox::new()
                .with_formatter(PathBufFormatter::new())
                .expand_width()
                .lens(State::data_dir),
            1.,
        )
        .expand_width();

    let game_dir_setting = Flex::row()
        .must_fill_main_axis(true)
        .main_axis_alignment(MainAxisAlignment::Start)
        .with_child(Label::new("Game Directory:"))
        .with_default_spacer()
        .with_flex_child(
            TextBox::new()
                .with_formatter(PathBufFormatter::new())
                .expand_width()
                .lens(State::game_dir),
            1.,
        )
        .expand_width();

    let content = Flex::column()
        .must_fill_main_axis(true)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(data_dir_setting)
        .with_default_spacer()
        .with_child(game_dir_setting);

    SizedBox::new(content)
        .width(800.)
        .expand_height()
        .padding(5.)
}

fn build_main() -> impl Widget<State> {
    ViewSwitcher::new(
        |state: &State, _| state.current_view,
        |selector, _, _| match selector {
            View::Mods => Box::new(build_view_mods()),
            View::Settings => Box::new(build_view_settings()),
        },
    )
}

fn build_log_view() -> impl Widget<State> {
    let font = FontDescriptor::new(FontFamily::MONOSPACE);
    let label = Label::raw()
        .with_font(font)
        .with_line_break_mode(LineBreaking::WordWrap)
        .lens(State::log)
        .padding(4.)
        .scroll()
        .vertical()
        .controller(AutoScrollController);

    SizedBox::new(label).expand_width().height(128.0)
}

fn build_window() -> impl Widget<State> {
    // TODO: Add  borders between the sections
    Flex::column()
        .must_fill_main_axis(true)
        .with_child(build_top_bar())
        .with_flex_child(build_main(), 1.0)
        .with_child(build_log_view())
        .controller(DirtyStateController)
        .on_added(|_, ctx: &mut LifeCycleCtx, _, _| {
            ctx.submit_command(
                ACTION_SET_WINDOW_HANDLE.with(SingleUse::new((*WINDOW_ID, ctx.window().clone()))),
            );
        })
}
