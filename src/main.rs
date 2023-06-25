use std::{
    collections::HashMap,
    env,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use lexical_sort::{natural_lexical_cmp, StringSort};

use eframe::{
    egui::{
        self,
        style::{Margin, Spacing, WidgetVisuals, Widgets},
        Button, CentralPanel, Color32, Context, Frame, Grid, Key, Label, RichText, Sense, Stroke,
        Style, TextEdit, Visuals,
    },
    epaint::{FontId, Rounding, Vec2},
    App, NativeOptions,
};

fn get_binaries() -> HashMap<String, PathBuf> {
    let mut binaries = HashMap::new();
    if let Ok(paths) = env::var("PATH") {
        for directory in paths.split(':') {
            if let Ok(entries) = std::fs::read_dir(directory) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let entry = entry.path();
                        if let Some(fname) = entry.file_name() {
                            binaries.insert(fname.to_string_lossy().to_string(), entry);
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
    items: Vec<String>,

    command: Arc<Mutex<Option<std::process::Command>>>,
    columns: usize,
    rows: usize,
    fg: Color32,
    bg: Color32,
    acc: Color32,
}

impl Linch {
    fn new(
        cc: &eframe::CreationContext<'_>,
        command: Arc<Mutex<Option<std::process::Command>>>,
        columns: usize,
        rows: usize,
        fg: Color32,
        bg: Color32,
        acc: Color32,
        opacity: f32,
    ) -> Self {
        let binaries = get_binaries();
        let keys = binaries.keys();
        let mut items = Vec::with_capacity(keys.len());
        for key in keys {
            items.push(key.to_string())
        }
        items.string_sort_unstable(natural_lexical_cmp);

        // let rounding = Rounding::none();
        // let bg_stroke = Stroke::none();
        let style = cc.egui_ctx.style().as_ref().clone();
        let stroke_fg = Stroke {
            color: fg,
            ..Default::default()
        };
        let stroke_bg = Stroke {
            color: bg,
            ..Default::default()
        };
        let stroke_acc = Stroke {
            color: acc,
            ..Default::default()
        };
        cc.egui_ctx.set_style(Style {
            visuals: Visuals {
                widgets: Widgets {
                    inactive: WidgetVisuals {
                        fg_stroke: stroke_fg,
                        ..style.visuals.widgets.inactive
                    },
                    hovered: WidgetVisuals {
                        fg_stroke: stroke_acc,
                        ..style.visuals.widgets.hovered
                    },
                    active: WidgetVisuals {
                        fg_stroke: stroke_bg,
                        ..style.visuals.widgets.active
                    },
                    ..style.visuals.widgets
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
            input_selected: true,
            index: 0,
            scroll: 0,
            items,

            command,
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
        *self.command.lock().unwrap() = self
            .items_filter()
            .nth(self.index)
            .map(|s| std::process::Command::new(s));
    }
}

impl App for Linch {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let count = self.items_filter().count().min(self.rows * self.columns);
        ctx.input(|i| {
            if i.key_pressed(Key::Enter) {
                self.set();
                frame.close();
            } else if i.key_pressed(Key::Escape) {
                frame.close();
            } else if i.key_pressed(Key::Tab) {
                self.input_selected = !self.input_selected;
            } else if i.key_pressed(Key::ArrowUp) && !self.input_selected {
                if self.index % self.rows == 0 {
                    self.input_selected = true;
                } else {
                    self.index -= 1
                }
            } else if i.key_pressed(Key::ArrowDown) {
                if self.input_selected {
                    self.input_selected = false;
                } else if self.index % self.rows < self.rows - 1 && self.index < count - 1 {
                    self.index += 1
                }
            } else if i.key_pressed(Key::ArrowRight)
                && !self.input_selected
                && self.index + self.rows < count
            {
                self.index += self.rows
            } else if i.key_pressed(Key::ArrowLeft)
                && !self.input_selected
                && self.index >= self.rows
            {
                self.index -= self.rows
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
                        match self.input_selected {
                            true => response.request_focus(),
                            false => response.surrender_focus(),
                        };
                    });

                let sense = Sense {
                    click: true,
                    drag: false,
                    focusable: true,
                };
                Grid::new("Items")
                    .min_row_height(sy)
                    .min_col_width(sx)
                    .show(ui, |ui| {
                        let items = self.items_filtered();
                        for r in 0..self.rows {
                            for c in 0..self.columns{
                                if let Some(i) = items.get(c * self.rows + r) {
                                    if self.index == r + self.rows * c {
                                        Frame::none().fill(hicol).show(ui, |ui| {
                                            ui.set_min_size(ui.available_size());
                                            if ui
                                                .add(
                                                    Label::new(
                                                        RichText::new(i).size(font).color(self.bg),
                                                    )
                                                    .sense(sense),
                                                )
                                                .clicked()
                                            {
                                                if self.input_selected {
                                                    self.input_selected = false;
                                                } else {
                                                    self.set();
                                                    frame.close();
                                                }
                                            }
                                        });
                                    } else {
                                        if ui
                                            .add(
                                                Label::new(RichText::new(i).size(font))
                                                    .sense(sense),
                                            )
                                            .clicked()
                                        {
                                            self.input_selected = false;
                                            self.index = r + self.rows * c
                                        };
                                    }
                                }
                            };
                            ui.end_row();
                        };
                    });
            });
    }
}

fn main() {
    let command: Arc<Mutex<Option<std::process::Command>>> = Default::default();
    let cmd2 = command.clone();

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
                cmd2,
                3,
                15,
                Color32::WHITE,
                Color32::BLACK,
                Color32::LIGHT_GREEN,
                0.2 // scales weird?
            ))
        }),
    )
    .expect("Linch died");

    if let Some(cmd) = command.lock().unwrap().as_mut() {
        // Child doesnt implement Drop so it just conveniently forks
        if let Err(e) = cmd.spawn() {
            panic!(
                "Could not start process {}\n{}",
                cmd.get_program().to_string_lossy(),
                e
            );
        };
    };
}
