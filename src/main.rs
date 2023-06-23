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
        Button, CentralPanel, Color32, Context, Frame, Grid, Key, Label, RichText, Stroke, Style,
        TextEdit, Visuals,
    },
    epaint::{Rounding, Vec2},
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
    // font_size: f32,
    // fg: Color,
    // bg: Color,
    // acc: Color,
}

impl Linch {
    fn new(
        cc: &eframe::CreationContext<'_>,
        command: Arc<Mutex<Option<std::process::Command>>>,
        columns: usize,
        rows: usize,
    ) -> Self {
        let binaries = get_binaries();
        let keys = binaries.keys();
        let mut items = Vec::with_capacity(keys.len());
        for key in keys {
            items.push(key.to_string())
        }
        items.string_sort_unstable(natural_lexical_cmp);
        Self {
            input: String::new(),
            input_selected: true,
            index: 0,
            scroll: 0,
            items,

            command,
            rows,
            columns,
            // font_size: flags.font_size,
            // bg: flags.background,
            // fg: flags.foreground,
            // acc: flags.accent,
        }
    }

    fn items_filter(&self) -> impl Iterator<Item = &String> {
        self.items.iter().filter(|s| s.starts_with(&self.input))
    }

    fn items_filtered(&self) -> Vec<String> {
        self.items_filter().map(|s| s.clone()).collect()
    }
}

impl App for Linch {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let count = self.items_filter().count().min(self.rows * self.columns);
        let sx = 800.0 / self.columns as f32;
        let sy = 400.0 / (self.rows as f32 + 1.0);
        ctx.input(|i| {
            if i.key_pressed(Key::Enter) {
                *self.command.lock().unwrap() = self
                    .items_filter()
                    .nth(self.index)
                    .map(|s| std::process::Command::new(s));
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
                    .inner_margin(0.0)
                    .outer_margin(0.0),
            )
            .show(ctx, |ui| {
                *ui.spacing_mut() = Spacing {
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
                    ..Default::default()
                };
                let response = ui.add_sized(
                    Vec2 { x: 800.0, y: sy },
                    TextEdit::singleline(&mut self.input),
                );
                if response.changed() {
                    self.index = 0
                }
                match self.input_selected {
                    true => response.request_focus(),
                    false => response.surrender_focus(),
                };
                Grid::new("Items")
                    .show(ui, |ui| {
                        let items = self.items_filtered();
                        (0..self.rows).for_each(|r| {
                            (0..self.columns)
                                .filter_map(|c| {
                                    items.get(c * self.rows + r).map(|i| {
                                        if self.index == r + self.rows * c {
                                            Label::new(
                                                RichText::new(i)
                                                    .background_color(Color32::WHITE)
                                                    .color(Color32::BLACK),
                                            )
                                        } else {
                                            Label::new(
                                                RichText::new(i)
                                                    .background_color(Color32::BLACK)
                                                    .color(Color32::WHITE),
                                            )
                                        }
                                    })
                                })
                                .for_each(|label| {
                                    ui.add_sized(Vec2 { x: sx, y: sy }, label);
                                });
                            ui.end_row();
                        });
                    })
                    .response
                    .id;
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
        Box::new(|cc| Box::new(Linch::new(cc, cmd2, 3, 15))),
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
