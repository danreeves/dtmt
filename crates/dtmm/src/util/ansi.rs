use ansi_parser::{AnsiParser, AnsiSequence, Output};
use druid::text::{RichText, RichTextBuilder};
use druid::{Color, FontStyle, FontWeight};

use crate::ui::theme;

#[derive(Default, Debug)]
struct TextState {
    color: Option<Color>,
    dim: bool,
    bold: bool,
    underline: bool,
    strikethrough: bool,
    italic: bool,
}

pub fn ansi_to_rich_text(input: &str) -> RichText {
    let mut builder = RichTextBuilder::new();

    let mut state = TextState::default();

    for token in input.ansi_parse() {
        match token {
            Output::TextBlock(text) => {
                let mut attr = builder.push(text);
                attr.underline(state.underline);
                attr.strikethrough(state.strikethrough);

                if state.bold {
                    attr.weight(FontWeight::BOLD);
                }

                if state.italic {
                    attr.style(FontStyle::Italic);
                }

                if let Some(color) = state.color {
                    attr.text_color(color);
                }
            }
            Output::Escape(AnsiSequence::SetGraphicsMode(values)) => {
                for v in values {
                    match v {
                        0 => {
                            state = Default::default();
                            break;
                        }
                        1 => state.bold = true,
                        2 => state.dim = true,
                        3 => state.italic = true,
                        4 => state.underline = true,
                        9 => state.strikethrough = true,
                        22 => {
                            state.bold = false;
                            state.dim = false;
                        }
                        23 => state.italic = false,
                        24 => state.underline = false,
                        29 => state.underline = false,
                        30..=40 | 90..=100 => {
                            let mut col = v - 30;
                            if col > 9 {
                                state.bold = true;
                                col -= 60;
                            }

                            state.color = match col {
                                // This escape code is usually called 'black', but is actually used
                                // as "foreground color", in regards to light themes.
                                0 => Some(theme::COLOR_FG),
                                1 => Some(theme::COLOR_RED_LIGHT),
                                2 => Some(theme::COLOR_GREEN_LIGHT),
                                3 => Some(theme::COLOR_YELLOW_LIGHT),
                                4 => Some(theme::COLOR_BLUE_LIGHT),
                                5 => Some(theme::COLOR_PURPLE_LIGHT),
                                6 => Some(theme::COLOR_AQUA_LIGHT),
                                // Similarly, 'white' is the background color
                                7 => Some(theme::COLOR_BG),
                                9 => None,
                                _ => unreachable!(),
                            };
                        }
                        _ => {}
                    }
                }
            }
            Output::Escape(_) => {}
        }
    }

    builder.build()
}
