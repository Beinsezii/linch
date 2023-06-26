use std::{
    collections::HashMap,
    env,
    num::NonZeroUsize,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use lexical_sort::{natural_lexical_cmp, StringSort};

use eframe::{
    egui::{
        self,
        style::{Selection, Spacing, WidgetVisuals, Widgets},
        CentralPanel, Color32, Frame, Grid, Key, Modifiers, Sense, Stroke, Style, TextEdit,
        Visuals,
    },
    emath::Align2,
    epaint::{FontId, Vec2},
    App, NativeOptions,
};

use clap::{Parser, Subcommand};
use regex::Regex;

fn parse_color(s: &str) -> Result<Color32, String> {
    colcon::hex_to_irgb(s).map(|rgb| Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
}

fn get_binaries() -> HashMap<String, PathBuf> {
    // {{{
    let mut binaries = HashMap::new();
    if let Ok(paths) = env::var("PATH") {
        for directory in paths.split(':') {
            if let Ok(entries) = std::fs::read_dir(directory) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        if let Ok(meta) = entry.metadata() {
                            let bit = 0b1;
                            if meta.is_file() && meta.permissions().mode() & bit == bit {
                                let path = entry.path();
                                if let Some(fname) = path.file_name() {
                                    binaries.insert(fname.to_string_lossy().to_string(), path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    binaries
} // }}}

fn scale_factor() -> f32 {
    if let Ok(val) = env::var("GDK_DPI_SCALE") {
        val.parse::<f32>().expect("Bad GDK_DPI_SCALE value")
    } else if let Ok(val) = env::var("GDK_SCALE") {
        val.parse::<f32>().expect("Bad GDK_SCALE value")
    } else {
        1.0
    }
}

struct Linch {
    input: String,
    input_compiled: Option<Regex>,
    input_selected: bool,
    index: usize,
    scroll: usize,
    hover: Option<usize>,
    focused: bool,

    response: Arc<Mutex<Option<String>>>,
    items: Vec<String>,
    prompt: String,
    columns: usize,
    rows: usize,
    fg: Color32,
    bg: Color32,
    acc: Color32,
    literal: bool,
    exit_unfocus: bool,
}

impl Linch {
    // {{{
    fn new(
        cc: &eframe::CreationContext<'_>,
        items: Vec<String>,
        response: Arc<Mutex<Option<String>>>,
        prompt: String,
        columns: usize,
        rows: usize,
        fg: Color32,
        bg: Color32,
        acc: Color32,
        opacity: f32,
        literal: bool,
        exit_unfocus: bool,
    ) -> Self {
        let style = cc.egui_ctx.style().as_ref().clone();
        cc.egui_ctx.set_style(Style {
            wrap: Some(false),
            visuals: Visuals {
                widgets: Widgets {
                    noninteractive: WidgetVisuals {
                        fg_stroke: Stroke {
                            color: fg,
                            ..Default::default()
                        },
                        ..style.visuals.widgets.noninteractive
                    },
                    ..style.visuals.widgets
                },
                selection: Selection {
                    bg_fill: acc.gamma_multiply(0.5),
                    stroke: Stroke::NONE,
                },
                window_fill: bg.gamma_multiply(opacity),
                ..style.visuals
            },
            spacing: Spacing {
                item_spacing: (0.0, 0.0).into(),
                window_margin: 0.0.into(),
                button_padding: (0.0, 0.0).into(),
                menu_margin: 0.0.into(),
                indent: 0.0,
                interact_size: (0.0, 0.0).into(),
                slider_width: 0.0,
                combo_width: 0.0,
                text_edit_width: 0.0,
                icon_width: 0.0,
                icon_width_inner: 0.0,
                icon_spacing: 0.0,
                tooltip_width: 0.0,
                combo_height: 0.0,
                scroll_bar_width: 0.0,
                scroll_handle_min_length: 0.0,
                scroll_bar_inner_margin: 0.0,
                scroll_bar_outer_margin: 0.0,
                indent_ends_with_horizontal_line: false,
            },
            ..style
        });

        Self {
            input: String::new(),
            input_compiled: None,
            input_selected: false,
            index: 0,
            scroll: 0,
            hover: None,
            focused: false,

            items,
            response,
            prompt,
            columns,
            rows,
            bg,
            fg,
            acc,
            literal,
            exit_unfocus,
        }
    }

    fn items_filter(&self) -> impl Iterator<Item = &String> {
        self.items.iter().filter(|s| {
            if let Some(re) = &self.input_compiled {
                re.is_match(s)
            } else {
                s.starts_with(&self.input)
            }
        })
    }

    fn items_filtered(&self, n: usize) -> Vec<String> {
        self.items_filter().take(n).cloned().collect()
    }

    fn compile(&mut self) {
        if !self.literal {
            self.input_compiled = Regex::new(&self.input).ok()
        }
    }

    fn set(&self) {
        *self.response.lock().unwrap() = self.items_filter().nth(self.index).cloned()
    }
} // }}}

impl App for Linch {
    // {{{
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        match frame.info().window_info.focused {
            true => self.focused = true,
            false => {
                if self.focused && self.exit_unfocus {
                    frame.close()
                }
            }
        }
        let count = self.items_filter().count().min(self.rows * self.columns);
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                self.set();
                frame.close();
            } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                frame.close();
            } else if i.consume_key(Modifiers::NONE, Key::Tab) {
                self.input_selected = !self.input_selected;
            }
            if !self.input_selected {
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) && self.index % self.rows != 0 {
                    self.index -= 1
                } else if i.consume_key(Modifiers::NONE, Key::ArrowDown)
                    && self.index % self.rows < self.rows - 1
                    && self.index < count - 1
                {
                    self.index += 1
                } else if i.consume_key(Modifiers::NONE, Key::ArrowRight)
                    && self.index + self.rows < count
                {
                    self.index += self.rows
                } else if i.consume_key(Modifiers::NONE, Key::ArrowLeft) && self.index >= self.rows
                {
                    self.index -= self.rows
                }
            }
        });
        CentralPanel::default()
            .frame(
                Frame::window(&ctx.style())
                    .inner_margin(1.0)
                    .outer_margin(1.0),
            )
            .show(ctx, |ui| {
                let (x, y) = match ui.available_size() {
                    // it works though
                    Vec2 { x, y } => (x, y),
                };
                let sx = x / self.columns as f32;
                let sy = y / (self.rows + 1) as f32;
                let font = sy * 0.75;

                let (tecol, hicol) = if self.input_selected {
                    (self.acc, self.fg)
                } else {
                    (self.fg, self.acc)
                };
                Frame::none() // the default frame isn't colorable?
                    .stroke(Stroke {
                        width: 2.0,
                        color: tecol,
                    })
                    .show(ui, |ui| {
                        let response = ui.add_sized(
                            Vec2 { x, y: sy },
                            TextEdit::singleline(&mut self.input)
                                .frame(false)
                                .font(FontId::proportional(font))
                                .text_color(tecol)
                                // hint color == gray_out(noninteractive_color)
                                .hint_text(&self.prompt)
                                .lock_focus(true),
                        );
                        if response.changed() {
                            self.compile();
                            self.index = 0
                        }
                        if response.clicked() {
                            self.input_selected = true;
                        }
                        response.request_focus()
                    });

                Grid::new("Items")
                    .min_row_height(sy)
                    .min_col_width(sx)
                    .max_col_width(sx)
                    .show(ui, |ui| {
                        let items = self.items_filtered(self.rows * self.columns);
                        let mut hover_set = false;
                        for r in 0..self.rows {
                            for c in 0..self.columns {
                                let n = r + self.rows * c;
                                if let Some(i) = items.get(n) {
                                    let mut stroke = Stroke::NONE;
                                    let mut text = ui.style().visuals.text_color();
                                    let mut fill = Color32::TRANSPARENT;
                                    let mut submit = false;
                                    if self.index == n {
                                        text = self.bg;
                                        submit = true;
                                        fill = hicol;
                                    } else if self.hover == Some(n) {
                                        stroke = Stroke {
                                            color: self.acc,
                                            width: 2.0,
                                        };
                                        text = self.acc;
                                    }
                                    let response = Frame::none()
                                        .stroke(stroke)
                                        .fill(fill)
                                        .inner_margin(3.0)
                                        .show(ui, |ui| {
                                            // manually paint text to avoid overallocation
                                            ui.allocate_painter(
                                                ui.available_size(),
                                                Sense::hover(), // 3 false
                                            )
                                            .1
                                            .text(
                                                ui.max_rect().left_center(),
                                                Align2::LEFT_CENTER,
                                                i,
                                                FontId::proportional(font),
                                                text,
                                            );
                                        })
                                        .response
                                        .interact(Sense::click());
                                    if response.hovered() {
                                        self.hover = Some(n);
                                        hover_set = true;
                                    }
                                    if response.clicked() {
                                        self.input_selected = false;
                                        if submit && !self.input_selected {
                                            self.set();
                                            frame.close();
                                        } else {
                                            self.index = n;
                                        }
                                    }
                                }
                            }
                            ui.end_row();
                        }
                        if !hover_set {
                            self.hover = None;
                        }
                    });
            });
    }
} // }}}

#[derive(Subcommand)]
enum LinchCmd {
    /// Launch a binary directly. Scans PATH by default
    Bin,
    /// Launch a desktop application.
    App,
    // Big maybe. If it's easy enough then sure, else no
    // /// dmenu compatibility mode
    // Dmenu,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct LinchArgs {
    // {{{
    /// Which mode to run
    #[command(subcommand)]
    command: LinchCmd,

    #[arg(short, long, default_value = "Run")]
    prompt: String,

    #[arg(short, long, default_value = "3")]
    columns: NonZeroUsize,

    #[arg(short, long, default_value = "15")]
    rows: NonZeroUsize,

    /// Window width in pixels
    #[arg(short = 'x', long, default_value = "800.0")]
    width: f32,

    /// Window height in pixels
    #[arg(short = 'y', long, default_value = "400.0")]
    height: f32,

    /// Foreground color in hex
    #[arg(short, long, default_value = "#ffffff", value_parser=parse_color)]
    foreground: Color32,

    /// Background color in hex
    #[arg(short, long, default_value = "#000000", value_parser=parse_color)]
    background: Color32,

    /// Accent color in hex
    #[arg(short, long, default_value = "#ffbb66", value_parser=parse_color)]
    accent: Color32,

    /// Background opacity 0.0 -> 1.0
    #[arg(short, long, default_value = "0.5")]
    opacity: f32,

    /// Match literal text as opposed to regular expressions
    #[arg(short, long)]
    literal: bool,

    /// Close linch on focus loss
    #[arg(short, long)]
    exit_unfocus: bool,
} // }}}

fn response(items: Vec<String>, args: LinchArgs) -> Option<String> {
    // {{{
    let result = Arc::new(Mutex::new(None));
    let res_send = result.clone();
    let factor = scale_factor();
    eframe::run_native(
        "Linch",
        NativeOptions {
            resizable: false,
            always_on_top: true,
            centered: true,
            initial_window_size: Some(Vec2::new(args.width * factor, args.height * factor)),
            ..Default::default()
        },
        Box::new(move |cc| {
            Box::new(Linch::new(
                cc,
                items,
                res_send,
                args.prompt,
                args.columns.into(),
                args.rows.into(),
                args.foreground,
                args.background,
                args.accent,
                args.opacity, // scales weird? Wayland issue?
                args.literal,
                args.exit_unfocus,
            ))
        }),
    )
    .expect("Linch died");
    // Arc::<Mutex<Option<String>>>::into_inner(result).map(|m| m.into_inner().unwrap()).flatten()
    let result = result.lock().unwrap().as_ref().cloned();
    result
} // }}}

fn main() {
    // {{{
    let args = LinchArgs::parse();

    match args.command {
        LinchCmd::Bin => {
            let mut items: Vec<String> = get_binaries().keys().cloned().collect();
            items.string_sort_unstable(natural_lexical_cmp);
            if let Some(result) = response(items, args) {
                let mut command = std::process::Command::new(result);
                if let Err(e) = command.spawn() {
                    panic!(
                        "Could not start process {}\n{}",
                        command.get_program().to_string_lossy(),
                        e
                    );
                };
            }
        }
        LinchCmd::App => unimplemented!("Desktop application support not yet implemented"),
    };
} // }}}
