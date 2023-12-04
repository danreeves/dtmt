use std::str::FromStr;
use std::sync::Arc;

use druid::im::Vector;
use druid::text::RichTextBuilder;
use druid::widget::{
    Checkbox, CrossAxisAlignment, Either, Flex, Image, Label, LineBreaking, List,
    MainAxisAlignment, Maybe, Scroll, SizedBox, Split, Svg, SvgData, TextBox, ViewSwitcher,
};
use druid::{lens, Env};
use druid::{
    Color, FileDialogOptions, FileSpec, FontDescriptor, FontFamily, LensExt, SingleUse, Widget,
    WidgetExt, WindowDesc, WindowId,
};
use druid::{Data, ImageBuf, LifeCycleCtx};
use druid_widget_nursery::WidgetExt as _;
use lazy_static::lazy_static;

use crate::state::{
    ModInfo, NexusInfo, NexusInfoLens, State, View, ACTION_ADD_MOD, ACTION_OPEN_LINK,
    ACTION_SELECTED_MOD_DOWN, ACTION_SELECTED_MOD_UP, ACTION_SELECT_MOD, ACTION_SET_WINDOW_HANDLE,
    ACTION_START_CHECK_UPDATE, ACTION_START_DELETE_SELECTED_MOD, ACTION_START_DEPLOY,
    ACTION_START_RESET_DEPLOYMENT,
};
use crate::ui::theme::{self, ColorExt, COLOR_YELLOW_LIGHT};
use crate::ui::widget::border::Border;
use crate::ui::widget::button::Button;
use crate::ui::widget::controller::{
    AutoScrollController, DirtyStateController, ImageLensController,
};
use crate::ui::widget::PathBufFormatter;

lazy_static! {
    pub static ref WINDOW_ID: WindowId = WindowId::next();
}

const TITLE: &str = "Darktide Mod Manager";
const WINDOW_SIZE: (f64, f64) = (1080., 720.);
const MOD_DETAILS_MIN_WIDTH: f64 = 325.;

pub(crate) fn new() -> WindowDesc<State> {
    WindowDesc::new(build_window())
        .title(TITLE)
        .window_size(WINDOW_SIZE)
}

fn build_top_bar() -> impl Widget<State> {
    let mods_button = Button::with_label("Mods")
        .on_click(|_ctx, state: &mut State, _env| state.current_view = View::Mods);

    let settings_button =
        Button::with_label("Settings").on_click(|_ctx, state: &mut State, _env| {
            state.current_view = View::Settings;
        });

    let check_update_button = {
        let make_button = || {
            Button::with_label("Check for updates").on_click(|ctx, _: &mut State, _| {
                ctx.submit_command(ACTION_START_CHECK_UPDATE);
            })
        };

        Either::new(
            |data, _| data.nexus_api_key.is_empty(),
            make_button()
                .tooltip(|_: &State, _: &Env| "A Nexus API key is required")
                .disabled_if(|_, _| true),
            make_button().disabled_if(|data, _| data.is_update_in_progress),
        )
    };

    let deploy_button = {
        let icon = Svg::new(SvgData::from_str(theme::icons::ALERT_CIRCLE).expect("invalid SVG"))
            .fix_height(druid::theme::TEXT_SIZE_NORMAL);

        let inner = Either::new(
            |state: &State, _| state.dirty,
            Flex::row()
                .with_child(icon)
                .with_spacer(3.)
                .with_child(Label::new("Deploy Mods")),
            Label::new("Deploy Mods"),
        );
        Button::new(inner)
            .on_click(|ctx, _state: &mut State, _env| {
                ctx.submit_command(ACTION_START_DEPLOY);
            })
            .disabled_if(|data, _| data.is_deployment_in_progress || data.is_reset_in_progress)
    };

    let reset_button = Button::with_label("Reset Game")
        .on_click(|ctx, _state: &mut State, _env| {
            ctx.submit_command(ACTION_START_RESET_DEPLOYMENT);
        })
        .disabled_if(|data, _| data.is_deployment_in_progress || data.is_reset_in_progress);

    let bar = Flex::row()
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
                .with_child(check_update_button)
                .with_default_spacer()
                .with_child(deploy_button)
                .with_default_spacer()
                .with_child(reset_button),
        )
        .padding(theme::TOP_BAR_INSETS)
        .background(theme::TOP_BAR_BACKGROUND_COLOR);

    Border::new(bar)
        .with_color(theme::COLOR_FG2)
        .with_bottom_border(1.)
}

fn build_mod_list() -> impl Widget<State> {
    let list = List::new(|| {
        let checkbox = Checkbox::new("")
            .env_scope(|env, selected| {
                env.set(druid::theme::BORDER_DARK, theme::COLOR_BG3);
                env.set(druid::theme::BORDER_LIGHT, theme::COLOR_BG3);
                env.set(druid::theme::TEXT_COLOR, theme::COLOR_ACCENT_FG);

                if *selected {
                    env.set(druid::theme::BACKGROUND_DARK, theme::COLOR_ACCENT);
                    env.set(druid::theme::BACKGROUND_LIGHT, theme::COLOR_ACCENT);
                } else {
                    env.set(druid::theme::BACKGROUND_DARK, Color::TRANSPARENT);
                    env.set(druid::theme::BACKGROUND_LIGHT, Color::TRANSPARENT);
                }
            })
            .lens(lens!((usize, Arc<ModInfo>, bool), 1).then(ModInfo::enabled.in_arc()));

        let name = Label::dynamic(|info: &Arc<ModInfo>, _| {
            info.nexus
                .as_ref()
                .map(|n| n.name.clone())
                .unwrap_or_else(|| info.name.clone())
        })
        .lens(lens!((usize, Arc<ModInfo>, bool), 1));

        let version = {
            let icon = {
                let tree =
                    theme::icons::parse_svg(theme::icons::ALERT_TRIANGLE).expect("invalid SVG");

                let tree = theme::icons::recolor_icon(tree, true, COLOR_YELLOW_LIGHT);

                Svg::new(tree).fix_height(druid::theme::TEXT_SIZE_NORMAL)
            };

            Either::new(
                |info, _| {
                    info.nexus
                        .as_ref()
                        .map(|n| info.version != n.version)
                        .unwrap_or(false)
                },
                Flex::row()
                    .with_child(icon)
                    .with_spacer(3.)
                    .with_child(Label::raw().lens(ModInfo::version.in_arc())),
                Label::raw().lens(ModInfo::version.in_arc()),
            )
            .lens(lens!((usize, Arc<ModInfo>, bool), 1))
        };

        let fields = Flex::row()
            .must_fill_main_axis(true)
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_child(name)
            .with_child(version);

        Flex::row()
            .must_fill_main_axis(true)
            .with_child(checkbox)
            .with_flex_child(fields, 1.)
            .padding((5.0, 4.0))
            .background(theme::keys::KEY_MOD_LIST_ITEM_BG_COLOR)
            .on_click(|ctx, (i, _, _), _env| ctx.submit_command(ACTION_SELECT_MOD.with(*i)))
            .env_scope(|env, (i, _, selected)| {
                if *selected {
                    env.set(theme::keys::KEY_MOD_LIST_ITEM_BG_COLOR, theme::COLOR_ACCENT);
                    env.set(
                        druid::theme::TEXT_COLOR,
                        theme::COLOR_ACCENT_FG.darken(0.05),
                    );
                } else {
                    env.set(druid::theme::TEXT_COLOR, theme::COLOR_FG);

                    if (i % 2) == 1 {
                        env.set(theme::keys::KEY_MOD_LIST_ITEM_BG_COLOR, theme::COLOR_BG1);
                    } else {
                        env.set(theme::keys::KEY_MOD_LIST_ITEM_BG_COLOR, theme::COLOR_BG);
                    }
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
                if !Data::same(&state.mods.get(i).cloned(), &Some(new.clone())) {
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
    let button_move_up = Button::with_label("Move Up")
        .on_click(|ctx, _state, _env| ctx.submit_command(ACTION_SELECTED_MOD_UP))
        .disabled_if(|state: &State, _env: &druid::Env| !state.can_move_mod_up());

    let button_move_down = Button::with_label("Move Down")
        .on_click(|ctx, _state, _env| ctx.submit_command(ACTION_SELECTED_MOD_DOWN))
        .disabled_if(|state: &State, _env: &druid::Env| !state.can_move_mod_down());

    let button_toggle_mod = Maybe::new(
        || {
            let inner = Label::dynamic(|enabled, _env| {
                if *enabled {
                    "Disable Mod".into()
                } else {
                    "Enable Mod".into()
                }
            });
            Button::new(inner)
                .on_click(|_ctx, enabled: &mut bool, _env| {
                    *enabled = !(*enabled);
                })
                .lens(ModInfo::enabled.in_arc())
        },
        // TODO: Gray out
        || Button::with_label("Enable Mod"),
    )
    .disabled_if(|info: &Option<Arc<ModInfo>>, _env: &druid::Env| info.is_none())
    .lens(State::selected_mod);

    let button_add_mod = Button::with_label("Add Mod").on_click(|ctx, _state: &mut State, _env| {
        let zip = FileSpec::new("Zip file", &["zip"]);
        let opts = FileDialogOptions::new()
            .allowed_types(vec![zip])
            .default_type(zip)
            .name_label("Mod Archive")
            .title("Choose a mod to add")
            .accept_command(ACTION_ADD_MOD);
        ctx.submit_command(druid::commands::SHOW_OPEN_PANEL.with(opts))
    });

    let button_delete_mod = Button::with_label("Delete Mod")
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
                .with_text_size(24.)
                // Force the label to take up the entire details' pane width,
                // so that we can center-align it.
                .expand_width()
                .lens(ModInfo::name.in_arc());
            let summary = Label::raw()
                .with_line_break_mode(LineBreaking::WordWrap)
                .lens(NexusInfoLens::new(NexusInfo::summary, ModInfo::summary).in_arc());

            // TODO: Image/icon?

            let version_line = Label::dynamic(|info: &Arc<ModInfo>, _| {
                let author = info
                    .nexus
                    .as_ref()
                    .map(|n| &n.author)
                    .or(info.author.as_ref());

                if let Some(author) = &author {
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

            let nexus_link = Maybe::or_empty(|| {
                let link = Label::raw().lens(NexusInfo::id.map(
                    |id| {
                        let url = format!("https://nexusmods.com/warhammer40kdarktide/mods/{}", id);
                        let mut builder = RichTextBuilder::new();
                        builder
                            .push("Open on Nexusmods")
                            .underline(true)
                            .text_color(theme::LINK_COLOR)
                            .link(ACTION_OPEN_LINK.with(Arc::new(url)));
                        builder.build()
                    },
                    |_, _| {},
                ));
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_alignment(MainAxisAlignment::Start)
                    .with_child(link)
                    .with_spacer(4.)
            })
            .lens(ModInfo::nexus.in_arc());

            let details = Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .main_axis_alignment(MainAxisAlignment::Start)
                .with_child(name)
                .with_spacer(4.)
                .with_child(summary)
                .with_spacer(4.)
                .with_child(nexus_link)
                .with_child(version_line)
                .with_spacer(4.)
                .with_child(categories)
                .padding((4., 4.));

            let image =
                Maybe::or_empty(|| Image::new(ImageBuf::empty()).controller(ImageLensController))
                    .lens(ModInfo::image.in_arc());

            Flex::column()
                .main_axis_alignment(MainAxisAlignment::Start)
                .must_fill_main_axis(true)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(image)
                // .with_spacer(4.)
                // .with_flex_child(details, 1.)
                .with_child(details)
        },
        Flex::column,
    )
    .lens(State::selected_mod)
}

fn build_mod_details() -> impl Widget<State> {
    Flex::column()
        .must_fill_main_axis(true)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_flex_child(build_mod_details_info(), 1.0)
        .with_child(build_mod_details_buttons().padding((4., 4., 4., 8.)))
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

    let nexus_apy_key_setting = Flex::row()
        .must_fill_main_axis(true)
        .main_axis_alignment(MainAxisAlignment::Start)
        .with_child(Label::new("Nexus API Key:"))
        .with_default_spacer()
        .with_flex_child(TextBox::new().expand_width().lens(State::nexus_api_key), 1.)
        .expand_width();

    let io_setting = Flex::row()
        .must_fill_main_axis(true)
        .main_axis_alignment(MainAxisAlignment::Start)
        .with_child(Label::new("Enable unsafe I/O:"))
        .with_default_spacer()
        .with_child(Checkbox::from_label(Label::dynamic(
            |enabled: &bool, _: &Env| {
                if *enabled {
                    "Enabled".into()
                } else {
                    "Disabled".into()
                }
            },
        )))
        .lens(State::is_io_enabled)
        .tooltip(|_: &State, _: &Env| {
            "Enabling this gives ANY mod full access to your files \
                and the ability to load arbitrary software libraries.\n\
                Only enable this if it is crucial for a mod's functionality, \
                and you are sure none of the ones you have installed are malicious."
        })
        .expand_width();

    let content = Flex::column()
        .must_fill_main_axis(true)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(data_dir_setting)
        .with_default_spacer()
        .with_child(game_dir_setting)
        .with_default_spacer()
        .with_child(io_setting)
        .with_default_spacer()
        .with_child(nexus_apy_key_setting);

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
    let list = List::new(|| {
        Label::raw()
            .with_font(FontDescriptor::new(FontFamily::MONOSPACE))
            .with_line_break_mode(LineBreaking::WordWrap)
    })
    .lens(State::log)
    .padding(4.)
    .scroll()
    .vertical()
    .controller(AutoScrollController);

    let inner = Border::new(list)
        .with_color(theme::COLOR_FG2)
        .with_top_border(1.);

    SizedBox::new(inner).expand_width().height(128.0)
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
