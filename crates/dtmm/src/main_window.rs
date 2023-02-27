use druid::im::Vector;
use druid::widget::{
    Align, Button, CrossAxisAlignment, Flex, Label, List, MainAxisAlignment, Maybe, Scroll, Split,
    TextBox, ViewSwitcher,
};
use druid::{
    lens, FileDialogOptions, FileSpec, Insets, LensExt, SingleUse, Widget, WidgetExt, WindowDesc,
};

use crate::state::{ModInfo, PathBufFormatter, State, View, ACTION_START_RESET_DEPLOYMENT};
use crate::state::{
    ACTION_ADD_MOD, ACTION_SELECTED_MOD_DOWN, ACTION_SELECTED_MOD_UP, ACTION_SELECT_MOD,
    ACTION_START_DELETE_SELECTED_MOD, ACTION_START_DEPLOY,
};
use crate::theme;
use crate::widget::ExtraWidgetExt;

const TITLE: &str = "Darktide Mod Manager";
const WINDOW_SIZE: (f64, f64) = (800.0, 600.0);
const MOD_DETAILS_MIN_WIDTH: f64 = 325.0;

pub(crate) fn new() -> WindowDesc<State> {
    WindowDesc::new(build_window())
        .title(TITLE)
        .window_size(WINDOW_SIZE)
}

fn build_top_bar() -> impl Widget<State> {
    Flex::row()
        .must_fill_main_axis(true)
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_child(
            Flex::row()
                .with_child(
                    Button::new("Mods").on_click(|_ctx, state: &mut State, _env| {
                        state.set_current_view(View::Mods)
                    }),
                )
                .with_default_spacer()
                .with_child(
                    Button::new("Settings").on_click(|_ctx, state: &mut State, _env| {
                        state.set_current_view(View::Settings)
                    }),
                )
                .with_default_spacer()
                .with_child(
                    Button::new("About").on_click(|_ctx, state: &mut State, _env| {
                        state.set_current_view(View::About)
                    }),
                ),
        )
        .with_child(
            Flex::row()
                .with_child(
                    Button::new("Deploy Mods")
                        .on_click(|ctx, _state: &mut State, _env| {
                            ctx.submit_command(ACTION_START_DEPLOY);
                        })
                        .disabled_if(|data, _| !data.can_deploy_mods()),
                )
                .with_default_spacer()
                .with_child(
                    Button::new("Reset Mods")
                        .on_click(|ctx, _state: &mut State, _env| {
                            ctx.submit_command(ACTION_START_RESET_DEPLOYMENT);
                        })
                        .disabled_if(|data, _| !data.can_reset_deployment()),
                ),
        )
        .padding(theme::TOP_BAR_INSETS)
        .background(theme::TOP_BAR_BACKGROUND_COLOR)
    // TODO: Add bottom border. Need a custom widget for that, as the built-in only provides
    // uniform borders on all sides
}

fn build_mod_list() -> impl Widget<State> {
    let list = List::new(|| {
        Flex::row()
            .must_fill_main_axis(true)
            .with_child(
                Label::dynamic(|enabled, _env| {
                    if *enabled {
                        "Enabled".into()
                    } else {
                        "Disabled".into()
                    }
                })
                .lens(lens!((usize, ModInfo), 1).then(ModInfo::enabled)),
            )
            .with_child(Label::raw().lens(lens!((usize, ModInfo), 1).then(ModInfo::name)))
            .on_click(|ctx, (i, _info), _env| ctx.submit_command(ACTION_SELECT_MOD.with(*i)))
    });

    let scroll = Scroll::new(list)
        .vertical()
        .lens(State::mods.map(
            |mods| {
                mods.iter()
                    .enumerate()
                    .map(|(i, val)| (i, val.clone()))
                    .collect::<Vector<_>>()
            },
            |mods, infos| {
                infos.into_iter().for_each(|(i, info)| {
                    mods.set(i, info);
                });
            },
        ))
        .content_must_fill();

    Flex::column()
        .must_fill_main_axis(true)
        .with_child(Flex::row())
        .with_flex_child(scroll, 1.0)
}

fn build_mod_details() -> impl Widget<State> {
    let details_container = Maybe::new(
        || {
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(Label::raw().lens(ModInfo::name))
                .with_flex_child(Label::raw().lens(ModInfo::description), 1.0)
        },
        Flex::column,
    )
    .padding(Insets::uniform_xy(5.0, 1.0))
    .lens(State::selected_mod);

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
            .lens(ModInfo::enabled)
        },
        // TODO: Gray out
        || Button::new("Enable Mod"),
    )
    .disabled_if(|info: &Option<ModInfo>, _env: &druid::Env| info.is_none())
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
        .on_click(|ctx, data: &mut Option<ModInfo>, _env| {
            if let Some(info) = data {
                ctx.submit_command(
                    ACTION_START_DELETE_SELECTED_MOD.with(SingleUse::new(info.clone())),
                );
            }
        })
        .disabled_if(|info: &Option<ModInfo>, _env: &druid::Env| info.is_none())
        .lens(State::selected_mod);

    let buttons = Flex::column()
        .with_child(
            Flex::row()
                .main_axis_alignment(MainAxisAlignment::End)
                .with_child(button_move_up)
                .with_default_spacer()
                .with_child(button_move_down)
                .padding(Insets::uniform_xy(5.0, 2.0)),
        )
        .with_child(
            Flex::row()
                .main_axis_alignment(MainAxisAlignment::End)
                .with_child(button_toggle_mod)
                .with_default_spacer()
                .with_child(button_add_mod)
                .with_default_spacer()
                .with_child(button_delete_mod)
                .padding(Insets::uniform_xy(5.0, 2.0)),
        )
        .with_default_spacer();

    Flex::column()
        .must_fill_main_axis(true)
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_flex_child(details_container, 1.0)
        .with_child(buttons)
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
    let game_dir_setting = Flex::row()
        .main_axis_alignment(MainAxisAlignment::Start)
        .with_child(Label::new("Game Directory:"))
        .with_default_spacer()
        .with_child(
            TextBox::new()
                .with_formatter(PathBufFormatter::new())
                .lens(State::game_dir),
        );
    let data_dir_setting = Flex::row()
        .main_axis_alignment(MainAxisAlignment::Start)
        .with_child(Label::new("Data Directory:"))
        .with_default_spacer()
        .with_child(
            TextBox::new()
                .with_formatter(PathBufFormatter::new())
                .lens(State::data_dir),
        );

    Flex::column()
        .with_child(data_dir_setting)
        .with_child(game_dir_setting)
        .padding(Insets::uniform(5.0))
}

fn build_view_about() -> impl Widget<State> {
    Align::centered(
        Flex::column()
            .with_child(Label::new("Darktide Mod Manager"))
            .with_child(Label::new(
                "Website: https://git.sclu1034.dev/bitsquid_dt/dtmt",
            )),
    )
}

fn build_main() -> impl Widget<State> {
    ViewSwitcher::new(
        |state: &State, _env| state.get_current_view(),
        |selector, _state, _env| match selector {
            View::Mods => Box::new(build_view_mods()),
            View::Settings => Box::new(build_view_settings()),
            View::About => Box::new(build_view_about()),
        },
    )
}

fn build_window() -> impl Widget<State> {
    Flex::column()
        .must_fill_main_axis(true)
        .with_child(build_top_bar())
        .with_flex_child(build_main(), 1.0)
}
