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
        CentralPanel, Color32, Frame, Grid, Key, Modifiers, RichText,
        Sense, Stroke, Style, TextEdit, Visuals,
    },
    epaint::{FontId, Vec2},
    App, NativeOptions,
};

fn get_binaries() -> HashMap<String, PathBuf> {
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
}

// fn scale_factor() -> f64 {
//     if let Ok(val) = env::var("GDK_DPI_SCALE") {
//         val.parse::<f64>().expect("Bad GDK_DPI_SCALE value")
//     } else if let Ok(val) = env::var("GDK_SCALE") {
//         val.parse::<f64>().expect("Bad GDK_SCALE value")
//     } else {
//         1.0
//     }
// }

struct Linch {
    input: String,
    input_selected: bool,
    index: usize,
    scroll: usize,
    hover: Option<usize>,
    focused: bool,
    exit_unfocus: bool,
    items: Vec<String>,

    response: Arc<Mutex<Option<String>>>,
    columns: usize,
    rows: usize,
    fg: Color32,
    bg: Color32,
    acc: Color32,
}

impl Linch {
    fn new(
        cc: &eframe::CreationContext<'_>,
        items: Vec<String>,
        response: Arc<Mutex<Option<String>>>,
        columns: usize,
        rows: usize,
        fg: Color32,
        bg: Color32,
        acc: Color32,
        opacity: f32,
        exit_unfocus: bool,
    ) -> Self {
        let style = cc.egui_ctx.style().as_ref().clone();
        cc.egui_ctx.set_style(Style {
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
            input_selected: false,
            index: 0,
            scroll: 0,
            hover: None,
            focused: false,
            exit_unfocus,
            items,

            response,
            rows,
            columns,
            bg,
            fg,
            acc,
        }
    }

    fn items_filter(&self) -> impl Iterator<Item = &String> {
        self.items.iter().filter(|s| s.starts_with(&self.input))
    }

    fn items_filtered(&self) -> Vec<String> {
        self.items_filter().map(|s| s.clone()).collect()
    }

    fn set(&self) {
        *self.response.lock().unwrap() = self.items_filter().nth(self.index).cloned()
    }
}

impl App for Linch {
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
                                .text_color(tecol),
                        );

                        if response.changed() {
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
                    .show(ui, |ui| {
                        let items = self.items_filtered();
                        let mut hover_set = false;
                        for r in 0..self.rows {
                            for c in 0..self.columns {
                                let n = r + self.rows * c;
                                if let Some(i) = items.get(n) {
                                    let mut stroke = Stroke::NONE;
                                    let (text, fill, submit) = if self.index == n {
                                        (RichText::new(i).size(font).color(self.bg), hicol, true)
                                    } else if self.hover == Some(n) {
                                        stroke = Stroke {
                                            color: self.acc,
                                            width: 2.0,
                                        };
                                        (
                                            RichText::new(i).size(font).color(self.acc),
                                            Color32::TRANSPARENT,
                                            false,
                                        )
                                    } else {
                                        (RichText::new(i).size(font), Color32::TRANSPARENT, false)
                                    };
                                    let response = Frame::none()
                                        .stroke(stroke)
                                        .fill(fill)
                                        .inner_margin(4.0)
                                        .show(ui, |ui| {
                                            // this prevents the Grid from shrinking but resize is
                                            // false for now so it doesn't matter
                                            ui.set_min_size(ui.available_size());
                                            ui.label(text)
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
}

fn main() {
    let response = Arc::new(Mutex::new(None));
    let resp_capt = response.clone();
    let binaries = get_binaries();
    let mut items: Vec<String> = binaries.keys().cloned().collect();
    items.string_sort_unstable(natural_lexical_cmp);
    eframe::run_native(
        "Linch",
        NativeOptions {
            resizable: false,
            always_on_top: true,
            centered: true,
            initial_window_size: Some(Vec2::new(800.0, 400.0)),
            ..Default::default()
        },
        Box::new(|cc| {
            Box::new(Linch::new(
                cc,
                items,
                resp_capt,
                3,
                15,
                Color32::WHITE,
                Color32::BLACK,
                Color32::LIGHT_GREEN,
                0.2, // scales weird?
                true,
            ))
        }),
    )
    .expect("Linch died");

    if let Some(response) = response.lock().unwrap().as_ref() {
        // Child doesnt implement Drop so it just conveniently forks
        let mut command = std::process::Command::new(response);
        if let Err(e) = command.spawn() {
            panic!(
                "Could not start process {}\n{}",
                command.get_program().to_string_lossy(),
                e
            );
        };
    };
}
