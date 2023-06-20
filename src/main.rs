use std::{
    collections::HashMap,
    env,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

use lexical_sort::{natural_lexical_cmp, StringSort};

use iced::{
    executor,
    keyboard::{self, KeyCode},
    subscription,
    theme::Palette,
    widget::{self, text_input, Text},
    window, Application, Color, Command, Element, Theme,
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

fn scale_factor() -> f64 {
    if let Ok(val) = env::var("GDK_DPI_SCALE") {
        val.parse::<f64>().expect("Bad GDK_DPI_SCALE value")
    } else if let Ok(val) = env::var("GDK_SCALE") {
        val.parse::<f64>().expect("Bad GDK_SCALE value")
    } else {
        1.0
    }
}

macro_rules! kc {
    ($key_code:expr) => {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key_code: $key_code,
            modifiers: keyboard::Modifiers::default(),
        })
    };
}

struct LinchFlags {
    command: Arc<Mutex<Option<std::process::Command>>>,
    rows: NonZeroUsize,
    columns: NonZeroUsize,
    foreground: Color,
    background: Color,
    accent: Color,
    font_size: f32,
}

impl Default for LinchFlags {
    fn default() -> Self {
        Self {
            command: Default::default(),
            rows: NonZeroUsize::new(15).unwrap(),
            columns: NonZeroUsize::new(5).unwrap(),
            foreground: Color::from_rgb(1.0, 1.0, 1.0),
            background: Color::from_rgb(0.0, 0.0, 0.0),
            accent: Color::from_rgb(1.0, 0.7, 0.4),
            font_size: 20.0,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Input(String),
    Forward(char),
    Submit,
    Quit,
    Toggle,
    Up,
    Down,
    Left,
    Right,
}

// TODO: remove if it works well
static FOCUS: AtomicBool = AtomicBool::new(false);
const PAD_INPUT: f32 = 5.0;
const PAD_LIST: f32 = 2.0;
const WIDTH: u32 = 800;

struct Linch {
    command: Arc<Mutex<Option<std::process::Command>>>,
    columns: usize,
    rows: usize,
    input: String,
    input_selected: bool,
    index: usize,
    scroll: usize,
    items: Vec<String>,
    font_size: f32,
    fg: Color,
    bg: Color,
    acc: Color,
}

impl Linch {
    fn items_filter(&self) -> impl Iterator<Item = &String> {
        self.items.iter().filter(|s| s.starts_with(&self.input))
    }

    // fn items_filtered(&self) -> Vec<String> {
    //     self.items.clone().into_iter().filter(|s| s.starts_with(&self.input)).collect()
    // }
}

impl Application for Linch {
    type Message = Message;
    type Flags = LinchFlags;
    type Executor = executor::Default;
    type Theme = Theme;

    fn new(flags: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        let binaries = get_binaries();
        let keys = binaries.keys();
        let mut items = Vec::with_capacity(keys.len());
        for key in keys {
            items.push(key.to_string())
        }
        items.string_sort_unstable(natural_lexical_cmp);
        (
            Self {
                command: flags.command,
                input: String::new(),
                input_selected: true,
                index: 0,
                scroll: 0,
                items,
                rows: flags.rows.into(),
                columns: flags.columns.into(),
                font_size: flags.font_size,
                bg: flags.background,
                fg: flags.foreground,
                acc: flags.accent,
            },
            text_input::focus(text_input::Id::new("entry")),
        )
    }

    fn title(&self) -> String {
        String::from("Linch")
    }

    fn view(&self) -> Element<'_, Self::Message> {
        widget::column(vec![
            widget::text_input("Search", &self.input)
                .size(self.font_size)
                .padding(PAD_INPUT)
                .id(text_input::Id::new("entry"))
                .on_submit(Message::Submit)
                .on_input(Message::Input)
                .into(),
            widget::row({
                let items = &mut self.items_filter().enumerate();
                (0..self.columns)
                    .map(|_| {
                        widget::column(
                            items
                                .take(self.rows)
                                .map(|(n, s)| {
                                    if n == self.index {
                                        // can't disable wrap??
                                        Text::new(s).size(self.font_size).style(self.acc).into()
                                    } else {
                                        Text::new(s).size(self.font_size).into()
                                    }
                                })
                                .collect(),
                            // force column size cause Row doesnt expand to fill...
                        )
                        .padding(0.0)
                        .spacing(0.0)
                        .width(
                            ((WIDTH as f64 / self.scale_factor() - PAD_LIST as f64 * 2.0)
                                / self.columns as f64)
                                .floor() as u16,
                        )
                        .into()
                    })
                    .collect()
            })
            .padding(PAD_LIST)
            .spacing(0.0)
            .into(),
        ])
            .padding(0.0)
            .spacing(0.0)
        .into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::Input(s) => {
                self.index = 0;
                self.input = s
            }
            Message::Toggle => self.input_selected = !self.input_selected,
            Message::Up => {
                if self.index == 0 {
                    self.input_selected = true
                } else {
                    self.index -= 1;
                }
            }
            Message::Down => {
                if self.input_selected {
                    self.input_selected = false
                }
                if self.index + 1 < self.rows * self.columns {
                    self.index += 1;
                }
            }
            Message::Left => {
                if !self.input_selected && self.index >= self.rows {
                    self.index -= self.rows
                }
            }
            Message::Right => {
                if !self.input_selected && self.index + self.rows < self.columns * self.rows {
                    self.index += self.rows
                }
            }
            Message::Forward(c) => {
                if !self.input_selected {
                    self.input.push(c);
                    self.index = 0;
                    self.input_selected = true
                }
            }
            Message::Submit => {
                *self.command.lock().unwrap() =
                    if let Some(cmd) = self.items_filter().nth(self.index) {
                        Some(std::process::Command::new(cmd))
                    } else {
                        None
                    };
                return window::close();
            }
            Message::Quit => return window::close(),
        }
        match self.input_selected {
            true => text_input::focus(text_input::Id::new("entry")),
            false => text_input::focus::<Message>(text_input::Id::unique()),
        }
    }

    fn scale_factor(&self) -> f64 {
        scale_factor()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        subscription::events_with(|event, _status| {
            if event == kc!(KeyCode::Tab) {
                Some(Message::Toggle)
            } else if event == kc!(KeyCode::Escape) {
                Some(Message::Quit)
            } else if event == kc!(KeyCode::Enter) {
                Some(Message::Submit)
            } else if event == kc!(KeyCode::Down) {
                Some(Message::Down)
            } else if event == kc!(KeyCode::Up) {
                Some(Message::Up)
            } else if event == kc!(KeyCode::Left) {
                Some(Message::Left)
            } else if event == kc!(KeyCode::Right) {
                Some(Message::Right)
            } else if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key_code,
                modifiers,
            }) = event
            {
                let offset = if modifiers == keyboard::Modifiers::SHIFT {
                    0
                } else {
                    32
                };
                if key_code as u32 <= 35 {
                    match key_code as u32 {
                        0..=8 => Some(Message::Forward(
                            char::from_u32(key_code as u32 + 49).unwrap(),
                        )),
                        9 => Some(Message::Forward('0')),
                        10.. => Some(Message::Forward(
                            char::from_u32(key_code as u32 + 55 + offset).unwrap(),
                        )),
                    }
                } else {
                    None
                }
            } else if event == iced::Event::Window(window::Event::Focused) {
                FOCUS.store(true, std::sync::atomic::Ordering::SeqCst);
                None
            } else if event == iced::Event::Window(window::Event::Unfocused)
                && FOCUS.load(std::sync::atomic::Ordering::SeqCst)
            {
                Some(Message::Quit)
            } else {
                None
            }
        })
    }

    fn theme(&self) -> Self::Theme {
        Theme::custom(Palette {
            background: self.bg,
            text: self.fg,
            primary: self.acc,
            success: self.acc,
            danger: self.acc,
        })
    }
}

fn main() {
    let mut flags = LinchFlags::default();
    let command = flags.command.clone();
    Linch::run(iced::Settings {
        window: window::Settings {
            resizable: false,
            always_on_top: true,
            size: (
                WIDTH,
                ((flags.font_size * (usize::from(flags.rows) as f32 + 1.0)
                    + (PAD_LIST + PAD_INPUT + 1.0/* inp bord */) * 2.0)
                    * scale_factor() as f32)
                    .ceil() as u32,
            ),
            ..Default::default()
        },
        flags,
        ..Default::default()
    })
    .expect("UI died");

    if let Some(cmd) = command.lock().unwrap().as_mut() {
        cmd.spawn()
            .expect("Could not start command")
            .wait()
            .expect("Child process died");
    };
}
