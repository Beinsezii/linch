use std::{collections::HashMap, path::PathBuf};

use lexical_sort::{StringSort, natural_lexical_cmp};

use iced::{
    executor,
    widget::{
        self,
        text_input::{focus, move_cursor_to, select_all, Id},
    },
    window, Application, Command, Element, Theme,
};

fn get_binaries() -> HashMap<String, PathBuf> {
    let mut binaries = HashMap::new();
    if let Ok(paths) = std::env::var("PATH") {
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

#[derive(Debug, Clone)]
enum Message {
    Input(String),
    Submit,
    Toggle,
}

struct Linch {
    input: String,
    input_selected: bool,
    index: usize,
    items: Vec<String>,
    binaries: HashMap<String, PathBuf>,
}

impl Application for Linch {
    type Message = Message;
    type Flags = ();
    type Executor = executor::Default;
    type Theme = Theme;

    fn new(_flags: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        let binaries = get_binaries();
        let keys = binaries.keys();
        let mut items = Vec::with_capacity(keys.len());
        for key in keys {
            items.push(key.to_string())
        }
        items.string_sort_unstable(natural_lexical_cmp);
        (
            Self {
                input: String::new(),
                input_selected: true,
                index: 0,
                items,
                binaries,
            },
            focus(Id::new("entry")),
        )
    }

    fn title(&self) -> String {
        String::from("Linch")
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let widgets = widget::column![
            widget::text_input("Search", &self.input)
                .id(Id::new("entry"))
                .on_submit(Message::Submit)
                .on_input(Message::Input),
            widget::scrollable(widget::column(self.items.iter().map(|s| s.as_str().into()).collect()))
        ]
        .into();
        widgets
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::Input(s) => self.input = s,
            Message::Submit => println!("{}", self.input),
            Message::Toggle => self.input_selected = !self.input_selected,
        }
        match self.input_selected {
            true => focus(Id::new("entry")),
            false => Command::none(),
        }
    }
}

fn main() -> iced::Result {
    Linch::run(iced::Settings {
        window: window::Settings {
            resizable: false,
            always_on_top: true,
            ..Default::default()
        },
        ..Default::default()
    })
}
