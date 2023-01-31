use druid::im::Vector;
use druid::widget::{
    Align, Button, CrossAxisAlignment, Flex, Label, List, MainAxisAlignment, Maybe, Scroll, Split,
    ViewSwitcher,
};
use druid::{lens, Insets, LensExt, Widget, WidgetExt, WindowDesc};

use crate::state::{ModInfo, State, View};
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
                .with_child(Button::new("Deploy Mods").on_click(
                    |_ctx, _state: &mut State, _env| {
                        todo!();
                    },
                ))
                .with_default_spacer()
                .with_child(
                    Button::new("Run Game").on_click(|_ctx, _state: &mut State, _env| {
                        todo!();
                    }),
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
            // .with_child(
            //     Label::dynamic(|enabled, _env| {
            //         if *enabled {
            //             "Enabled".into()
            //         } else {
            //             "Disabled".into()
            //         }
            //     })
            //     .lens(
            //         lens::Identity
            //             .map(
            //                 |(i, info)| info,
            //                 |(i, info), new_info| {
            //                     todo!();
            //                 },
            //             )
            //             .then(ModInfo::enabled),
            //     ),
            // )
            // .with_child(Label::raw().lens(ModInfo::name))
            .on_click(|_ctx, state, _env| {
                todo!();
            })
    });

    Scroll::new(list)
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
        .content_must_fill()
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
    .lens(State::selected_mod);

    let button_move_up = Button::new("Move Up")
        .on_click(|_ctx, index: &mut Option<usize>, _env| {
            if let Some(i) = index.as_mut() {
                *i = i.saturating_sub(1)
            }
        })
        .lens(State::selected_mod_index);

    let button_move_down = Button::new("Move Down")
        .on_click(|_ctx, index: &mut Option<usize>, _env| {
            if let Some(i) = index.as_mut() {
                *i = i.saturating_add(1)
            }
        })
        .lens(State::selected_mod_index);

    let button_toggle_mod = Maybe::new(
        || {
            Button::dynamic(|enabled, _env| {
                if *enabled {
                    "Disable Mod".into()
                } else {
                    "Enabled Mod".into()
                }
            })
            .on_click(|_ctx, info: &mut bool, _env| {
                *info = !*info;
            })
            .lens(ModInfo::enabled)
        },
        // TODO: Gray out
        || Button::new("Enable Mod"),
    )
    .lens(State::selected_mod);

    let button_add_mod = Button::new("Add Mod").on_click(|_ctx, state: &mut State, _env| {
        // TODO: Implement properly
        let info = ModInfo::new();
        state.add_mod(info);
    });

    let button_delete_mod = Button::new("Delete Mod")
        .on_click(|_ctx, data: &mut State, _env| data.delete_selected_mod());

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
    Label::new("Settings")
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
