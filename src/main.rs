use std::ffi::{OsStr, OsString};
use std::fs::{read_to_string, remove_file, write, File};
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, env, io::Read, num::NonZeroUsize, os::unix::fs::PermissionsExt, path::PathBuf};

use colcon::{convert_space, convert_space_chunked, Space};
use eframe::egui::style::{ScrollStyle, Selection, Spacing, WidgetVisuals, Widgets};
use eframe::egui::{
    CentralPanel, Color32, ColorImage, Context, Frame, Grid, Image, Key, Modifiers, Sense, Stroke, Style, TextEdit,
    TextureHandle, TextureOptions, ViewportBuilder, ViewportCommand, Visuals, WindowLevel,
};
use eframe::epaint::{FontId, Rgba, Rounding, Shadow, Vec2};
use eframe::{emath::Align2, App, NativeOptions};

use clap::{Parser, Subcommand};
use lexical_sort::natural_lexical_cmp;
use regex::Regex;
use resvg::{tiny_skia, usvg};
use walkdir::WalkDir;

use rayon::prelude::*;

#[derive(Clone, PartialEq, Eq)]
struct Item {
    name: String,
    file: Option<PathBuf>,
    exec: Option<String>,
    path: Option<PathBuf>,
    icon: Option<String>,
    hidden: bool,
}

impl Item {
    fn from_path(path: PathBuf) -> Result<Self, ()> {
        let fname = path.file_name().map(|osstr| osstr.to_string_lossy().to_string());
        fname
            .map(|name| Self {
                name,
                file: Some(path),
                path: None,
                exec: None,
                icon: None,
                hidden: false,
            })
            .ok_or(())
    }
    fn from_desktop(path: PathBuf) -> Result<Self, ()> {
        // {{{
        if path.extension() == Some(OsString::from("desktop").as_os_str()) {
            if let Ok(data) = read_to_string(&path) {
                let mut start = false;
                let mut hm = HashMap::new();
                for line in data.lines() {
                    if line.trim() == "[Desktop Entry]" {
                        start = true;
                    } else if line.trim().starts_with("[") {
                        break;
                    } else if start {
                        if let Some((a, b)) = line.split_once("=") {
                            hm.insert(a.trim().to_string(), b.trim_start().to_string());
                        }
                    }
                }
                if let Some(name) = hm.get(&String::from("Name")) {
                    Ok(Self {
                        name: name.to_string(),
                        file: Some(path),
                        exec: hm.get(&String::from("Exec")).cloned(),
                        icon: hm.get(&String::from("Icon")).cloned(),
                        path: hm.get(&String::from("Path")).cloned().map(|s| PathBuf::from(s)),
                        hidden: hm
                            .get(&String::from("Hidden"))
                            .map(|s| s.parse::<bool>().ok())
                            .flatten()
                            .unwrap_or(false)
                            | hm.get(&String::from("NoDisplay"))
                                .map(|s| s.parse::<bool>().ok())
                                .flatten()
                                .unwrap_or(false),
                    })
                } else {
                    Err(())
                }
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    } // }}}
}

impl AsRef<str> for Item {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

impl std::fmt::Display for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

fn parse_color(s: &str) -> Result<Color32, String> {
    colcon::str2space::<f32, 3>(s, Space::LRGB)
        .map(|rgb| Color32::from(Rgba::from_rgb(rgb[0], rgb[1], rgb[2])))
        .ok_or_else(|| String::from("Could not parse \"") + s + "\" as a color.")
}

// Reference:
// https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html
// https://specifications.freedesktop.org/desktop-entry-spec/desktop-entry-spec-latest.html

fn get_binaries() -> Vec<Item> {
    // {{{
    let mut binaries = Vec::new();
    if let Ok(paths) = env::var("PATH") {
        for directory in paths.split(':') {
            for entry in WalkDir::new(directory).follow_links(true) {
                if let Ok(entry) = entry {
                    if let Ok(meta) = entry.metadata() {
                        let bit = 0b1;
                        if !meta.is_dir() && meta.permissions().mode() & bit == bit {
                            let path = entry.into_path();
                            if let Ok(item) = Item::from_path(path) {
                                binaries.push(item);
                            }
                        }
                    }
                }
            }
        }
    }
    binaries
} // }}}

fn get_applications(include_hidden: bool) -> Vec<Item> {
    // {{{
    let mut paths = Vec::<PathBuf>::new();
    // add them in backwards because the desktop entry spec
    // states it should return the first found
    paths.extend(
        env::var("XDG_DATA_DIRS")
            .unwrap_or(String::from("/usr/local/share/:/usr/share/"))
            .split(':')
            .rev()
            .map(|s| s.into()),
    );
    paths.push(
        env::var_os("XDG_DATA_HOME")
            .unwrap_or(OsString::from(env::var("HOME").unwrap() + "/.local/share"))
            .into(),
    );

    let mut result = Vec::new();

    for mut path in paths {
        path.push("applications");
        for entry in WalkDir::new(path).follow_links(true) {
            if let Ok(entry) = entry {
                if let Ok(item) = Item::from_desktop(entry.into_path()) {
                    if include_hidden | !item.hidden {
                        result.push(item);
                    }
                }
            }
        }
    }

    result
} // }}}

fn get_icon_loc(name: &str) -> Option<PathBuf> {
    // {{{
    // on my system covers every app that doesn't have a stupid location
    for f in [
        name.to_string(),
        // Prefer Papirus SVGs
        format!("/usr/share/icons/Papirus/64x64/apps/{}.svg", name),
        format!("/usr/share/icons/hicolor/scalable/apps/{}.svg", name),
        // HiColor PNGs
        format!("/usr/share/icons/hicolor/64x64/apps/{}.png", name),
        format!("/usr/share/icons/hicolor/128x128/apps/{}.png", name),
        format!("/usr/share/icons/hicolor/256x256/apps/{}.png", name),
        // Check other locations
        format!("/usr/share/icons/Papirus/64x64/devices/{}.svg", name),
    ] {
        let buf = PathBuf::from(f);
        if buf.is_file() {
            return Some(buf);
        }
    }
    // fall back to scanning. Don't like this, kind of want to remove but idk how other
    // distros'/themes' layouts may differ
    let osname = Some(OsStr::new(name));
    let png = Some(OsStr::new("png"));
    let svg = Some(OsStr::new("svg"));
    // No Papirus scannign since its layout is super standardized and there's like a million files
    for entry in WalkDir::new("/usr/share/icons/hicolor")
        .into_iter()
        .chain(WalkDir::new("/usr/share/icons/Adwaita"))
        // scan all other themes as last resort
        .chain(WalkDir::new("/usr/share/icons").into_iter().filter_entry(|e| {
            e.file_name() != "Papirus"
                        && e.file_name() != "hicolor"
                        && e.file_name() != "Adwaita"
                        // also skip symbolic icons. I'm not gonna support them
                        && e.file_name() != "symbolic"
        }))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().file_stem() == osname)
        .filter(|e| e.path().extension() == png || e.path().extension() == svg)
    {
        // println!("### FOUND {} AT {}", name, entry.path().display());
        return Some(entry.path().to_owned());
    }
    // println!("### COULD NOT FIND {}", name);
    None
} // }}}

fn monochromatize(mut reference: [f32; 3], target: &mut [[f32; 4]], target_space: Space) {
    // {{{
    convert_space(Space::LRGB, Space::JZCZHZ, &mut reference);
    convert_space_chunked(target_space, Space::JZCZHZ, target);

    let [lmax, cmax, _] = Space::JZCZHZ.srgb_quants()[100];
    let [lmin, cmin, _] = Space::JZCZHZ.srgb_quants()[0];

    target.iter_mut().for_each(|chunk| {
        let l = (chunk[0] - lmin) / lmax + lmin / lmax;
        let c = (chunk[1] - cmin) / cmax + cmin / cmax;
        let h = chunk[2];

        // set hue
        chunk[2] = reference[2];

        // adjust chroma up to 50% based on proximity to middle gray
        chunk[1] = c * 0.5 + (0.5 - (l - 0.5).abs());

        // uses a reverse HK delta to exacurbate dark and light hues against the reference
        let tar_delta = colcon::hk_high2023(&[100.0, 100.0, h]);
        let ref_delta = colcon::hk_high2023(&[100.0, 100.0, reference[2]]);
        chunk[0] = l + (ref_delta - tar_delta) / 100.0 / 2.0;

        chunk[0] = (chunk[0] + lmin / lmax) * lmax;
        chunk[1] = (chunk[1] + lmin / lmax) * lmax;
    });

    convert_space_chunked(Space::JZCZHZ, target_space, target);
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

// ### Cache FNS {{{

fn cache_file(name: &str) -> PathBuf {
    assert!(!name.is_empty());
    let fname = String::from("/linch_") + name;
    if let Ok(xdg_cache) = env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg_cache + &fname)
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home + "/.cache" + &fname)
    } else {
        panic!("Could not find cache directory.\nOne of the following must environment variables be set\nXDG_CACHE_HOME\nHOME")
    }
}

fn cache_get(name: &str) -> Vec<(usize, String)> {
    let mut result = Vec::new();
    if let Ok(data) = read_to_string(cache_file(name)) {
        let re = Regex::new(r"^(\d+) +(.+)$").unwrap();
        for line in data.lines() {
            if let Some(captures) = re.captures(line.trim()) {
                result.push((captures[1].parse::<usize>().unwrap(), captures[2].to_string()))
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0).reverse().then(a.1.cmp(&b.1)));
    result
}

fn cache_set(name: &str, lines: Vec<(usize, String)>) {
    write(
        cache_file(name),
        lines
            .into_iter()
            .map(|(n, s)| format!("{} {}", n, s))
            .fold(String::new(), |a, b| a + &b + "\n"),
    )
    .unwrap();
}

fn cache_apply(name: &str, items: &mut [Item]) {
    let map: HashMap<String, usize> = HashMap::from_iter(cache_get(name).into_iter().map(|(n, s)| (s, n)));
    items.sort_by(|a, b| {
        map.get(&a.name.clone())
            .unwrap_or(&0)
            .cmp(map.get(&b.name.clone()).unwrap_or(&0))
            .reverse()
            .then(natural_lexical_cmp(a.as_ref(), b.as_ref()))
    });
}

fn cache_add(name: &str, item: &Item) {
    let mut cache = cache_get(name);
    let mut set = false;
    for line in cache.iter_mut() {
        if line.1 == item.as_ref() {
            line.0 = line.0.saturating_add(1); //optimistic lol
            set = true;
        }
    }
    if !set {
        cache.push((1, item.name.clone()))
    }
    cache_set(name, cache);
}

fn cache_del(name: &str, item: &Item) {
    cache_set(
        name,
        cache_get(name)
            .into_iter()
            .filter(|(_n, s)| s != item.as_ref())
            .collect(),
    );
}

// ### Cache FNS }}}

struct Linch {
    input: String,
    input_compiled: Option<Regex>,
    input_selected: bool,
    index: usize,
    scroll: usize,
    hover: Option<usize>,
    focused: bool,
    images: HashMap<String, TextureHandle>,

    response: Arc<Mutex<Option<Item>>>,
    items: Vec<Item>,
    custom: bool,
    cache: String,
    prompt: String,
    columns: usize,
    rows: usize,
    fg: Color32,
    bg: Color32,
    acc: Color32,
    scale: f32,
    literal: bool,
    exit_unfocus: bool,
    icons: bool,
}

impl Linch {
    // {{{
    fn new(
        cc: &eframe::CreationContext<'_>,
        mut items: Vec<Item>,
        response: Arc<Mutex<Option<Item>>>,
        custom: bool,
        cache: String,
        prompt: String,
        mut columns: usize,
        rows: usize,
        fg: Color32,
        bg: Color32,
        acc: Color32,
        opacity: f32,
        scale: f32,
        literal: bool,
        exit_unfocus: bool,
        icons: bool,
        monochrome: bool,
        size: [f32; 2],
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
                    stroke: Stroke {
                        width: 1.0, // seems fixed?
                        color: acc,
                    },
                },
                window_fill: bg.gamma_multiply(opacity),
                window_shadow: Shadow::NONE,
                window_stroke: Stroke::new(3.0 * scale, acc),
                window_rounding: Rounding::ZERO,
                ..style.visuals
            },
            spacing: Spacing {
                item_spacing: (0.0, 0.0).into(),
                window_margin: (4.0 * scale).into(),
                button_padding: (0.0, 0.0).into(),
                menu_margin: 0.0.into(),
                indent: 0.0,
                interact_size: (0.0, 0.0).into(),
                slider_width: 0.0,
                slider_rail_height: 0.0,
                combo_width: 0.0,
                text_edit_width: 0.0,
                icon_width: 0.0,
                icon_width_inner: 0.0,
                icon_spacing: 0.0,
                tooltip_width: 0.0,
                menu_width: 0.0,
                menu_spacing: 0.0,
                indent_ends_with_horizontal_line: false,
                combo_height: 0.0,
                scroll: ScrollStyle::default(),
            },
            ..style
        });

        if !cache.is_empty() {
            cache_apply(&cache, &mut items);
        } else {
            items.sort_unstable_by(|a, b| natural_lexical_cmp(a.as_ref(), b.as_ref()))
        }

        let color_images = Mutex::new(HashMap::new());
        let acc_pixel = Rgba::from(acc);
        let acc_pixel = [acc_pixel[0], acc_pixel[1], acc_pixel[2]];
        let w = (size[1] * scale / (rows + 1) as f32 / 16.0).ceil() as u32 * 16;
        let h = w;
        if icons {
            #[cfg(debug_assertions)]
            let now = std::time::Instant::now();

            items.par_iter().filter_map(|i| i.icon.as_ref()).for_each(|icon| {
                if !color_images.lock().unwrap().contains_key(icon) {
                    if let Some(path) = get_icon_loc(&icon) {
                        if let Ok(mut file) = File::open(&path) {
                            let mut data = Vec::new();
                            if file.read_to_end(&mut data).is_ok() {
                                let mut color_image = None;
                                if path.extension() == Some(&OsStr::new("svg")) {
                                    if let Ok(data) = usvg::Tree::from_data(&data, &usvg::Options::default()) {
                                        let scale =
                                            (w as f32 / data.size().width()).min(h as f32 / data.size().height());
                                        let mut pixbuf = tiny_skia::Pixmap::new(w, h).unwrap();
                                        resvg::render(
                                            &data,
                                            tiny_skia::Transform::from_scale(scale, scale),
                                            &mut pixbuf.as_mut(),
                                        );
                                        color_image = Some(ColorImage::from_rgba_unmultiplied(
                                            [pixbuf.width() as usize, pixbuf.height() as usize],
                                            &pixbuf.take(),
                                        ));
                                    }
                                } else {
                                    if let Some(image) =
                                        image::io::Reader::open(path).map(|r| r.decode().ok()).ok().flatten()
                                    {
                                        color_image = Some(ColorImage::from_rgba_unmultiplied(
                                            [image.width() as usize, image.height() as usize],
                                            &image.into_rgba8(),
                                        ));
                                    };
                                }
                                if let Some(mut ci) = color_image {
                                    if monochrome {
                                        let mut pixels: Vec<[f32; 4]> = ci
                                            .pixels
                                            .into_iter()
                                            .map(|c32| Rgba::from(c32).to_rgba_unmultiplied())
                                            .collect();

                                        monochromatize(acc_pixel, &mut pixels, Space::LRGB);

                                        ci.pixels = pixels
                                            .into_iter()
                                            .map(|p| {
                                                Color32::from(Rgba::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                                            })
                                            .collect();
                                    }
                                    color_images.lock().unwrap().insert(icon.to_string(), ci);
                                }
                            }
                        }
                    }
                }
            });
            #[cfg(debug_assertions)]
            println!("Icons loaded in {:?}", now.elapsed());
        }

        let mut images = HashMap::new();
        for (k, v) in color_images.into_inner().unwrap().into_iter() {
            let th = cc.egui_ctx.load_texture(&k, v, TextureOptions::default());
            images.insert(k, th);
        }

        columns = ((items.len() as f32 / rows as f32).ceil() as usize).min(columns).max(1);

        Self {
            input: String::new(),
            input_compiled: None,
            input_selected: false,
            index: 0,
            scroll: 0,
            hover: None,
            focused: false,
            images,

            items,
            custom,
            response,
            cache,
            prompt,
            columns,
            rows,
            bg,
            fg,
            acc,
            scale,
            literal,
            exit_unfocus,
            icons,
        }
    }

    fn items_filter(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|s| {
            if let Some(re) = &self.input_compiled {
                re.is_match(s.as_ref())
            } else {
                s.as_ref().starts_with(&self.input)
            }
        })
    }

    fn items_filtered(&self, count: usize, skip: usize) -> Vec<Item> {
        self.items_filter().skip(skip).take(count).cloned().collect()
    }

    fn selected(&self) -> Option<Item> {
        self.items_filter()
            .nth(self.index + self.scroll * self.rows * self.columns)
            .cloned()
    }

    fn compile(&mut self) {
        if !self.literal {
            self.input_compiled = Regex::new(&(String::from("(?i)") + &self.input)).ok()
        }
    }

    fn set(&self) {
        let mut item = self.selected();
        if let Some(item) = item.as_ref() {
            if !self.cache.is_empty() {
                cache_add(&self.cache, item)
            }
        }
        if self.custom && item.is_none() && !self.input.is_empty() {
            item = Some(Item {
                name: self.input.clone(),
                file: None,
                exec: None,
                path: None,
                icon: None,
                hidden: false,
            })
        }
        *self.response.lock().unwrap() = item
    }

    fn del(&mut self) {
        if !self.cache.is_empty() {
            if let Some(item) = self.selected() {
                cache_del(&self.cache, &item);
                cache_apply(&self.cache, &mut self.items)
            }
        }
    }
} // }}}

impl App for Linch {
    // {{{
    fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
        Color32::TRANSPARENT.to_normalized_gamma_f32()
    }
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let mut close = false;
        let area = self.rows * self.columns;
        let count = self.items_filter().count() - self.scroll * area;
        ctx.input_mut(|i| {
            match i.viewport().focused {
                Some(true) => self.focused = true,
                Some(false) => {
                    if self.focused && self.exit_unfocus {
                        close = true
                    }
                }
                None => (),
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                self.set();
                close = true
            } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                close = true
            } else if i.consume_key(Modifiers::NONE, Key::Tab) {
                self.input_selected = !self.input_selected;
            } else if i.consume_key(Modifiers::NONE, Key::Delete) {
                self.del()
            } else if i.raw_scroll_delta.y < 0.0 && count > area {
                self.scroll += 1;
                self.index = self.index.min(count - area - 1)
            } else if i.raw_scroll_delta.y > 0.0 && self.scroll > 0 {
                self.scroll -= 1
            }
            if !self.input_selected {
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    if self.index % self.rows != 0 {
                        self.index -= 1
                    } else if self.scroll > 0 {
                        self.scroll -= 1;
                        self.index += self.rows - 1
                    }
                } else if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    if self.index % self.rows < self.rows - 1 && self.index < count.saturating_sub(1) {
                        self.index += 1
                    } else if count > area {
                        self.scroll += 1;
                        self.index = (self.index + 1 - self.rows).min(count - area - 1)
                    }
                } else if i.consume_key(Modifiers::NONE, Key::ArrowRight) && self.index + self.rows < count.min(area) {
                    self.index += self.rows
                } else if i.consume_key(Modifiers::NONE, Key::ArrowLeft) && self.index >= self.rows {
                    self.index -= self.rows
                }
            }
        });
        CentralPanel::default()
            .frame(Frame::window(&ctx.style()))
            .show(ctx, |ui| {
                // idk why but it works
                // weirdly the trailing edge is fatter?
                // also bottom doesnt scale properly with -s 0.5...
                let marg = ui.spacing().window_margin.top / 2.0;
                let (x, y) = match ui.available_size() {
                    // it works though
                    Vec2 { x, y } => (x - marg, y - marg),
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
                        width: 2.0 * self.scale,
                        color: tecol,
                    })
                    .outer_margin(1.0 * self.scale)
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
                            self.index = 0;
                            self.scroll = 0;
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
                        let items =
                            self.items_filtered(self.rows * self.columns, self.scroll * self.rows * self.columns);
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
                                            width: 2.0 * self.scale,
                                        };
                                        text = self.acc;
                                    }
                                    let response = Frame::none()
                                        .stroke(stroke)
                                        .fill(fill)
                                        .inner_margin(2.0 * self.scale)
                                        .show(ui, |ui| {
                                            let mut shrink2 = Vec2 { x: 0.0, y: 0.0 };
                                            if self.icons {
                                                shrink2 = Vec2 {
                                                    x: ui.available_height(),
                                                    y: 0.0,
                                                };
                                                match i.icon.as_ref().map(|i| self.images.get(i)).flatten() {
                                                    Some(image) => drop(ui.add(Image::new(image).fit_to_exact_size(
                                                        (ui.available_height(), ui.available_height()).into(),
                                                    ))),
                                                    None => drop(ui.allocate_space(Vec2::splat(ui.available_height()))),
                                                }
                                            }
                                            // manually paint text to avoid overallocation
                                            ui.allocate_painter(
                                                ui.available_size(),
                                                Sense::hover(), // 3 false
                                            )
                                            .1
                                            .text(
                                                ui.max_rect().shrink2(shrink2).left_center(),
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
                                            close = true
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
        if close {
            ctx.send_viewport_cmd(ViewportCommand::Close)
        }
    }
} // }}}

#[derive(Subcommand)]
enum LinchCmd {
    /// Launch a binary directly. Scans PATH by default
    Bin,
    /// Launch a desktop application.
    App {
        /// Show all entries, including hidden and technical
        #[arg(long)]
        all: bool,

        /// Recolor icons to monochrome style accent. Strongly recommended to have a scalable
        /// theme, as PNGs take 10x longer to recolor than SVGs
        #[arg(long)]
        monochrome: bool,
    },
    /// dmenu-like choices from stdin lines. No choices will allow custom input
    Dmenu,
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

    /// Window width. Affected by scale
    #[arg(short = 'x', long, default_value = "800.0")]
    width: f32,

    /// Window height. Affected by scale
    #[arg(short = 'y', long, default_value = "400.0")]
    height: f32,

    /// Foreground color in #hex or color space
    #[arg(short, long, default_value = "#ffffff", value_parser=parse_color)]
    foreground: Color32,

    /// Background color in #hex or color space
    #[arg(short, long, default_value = "#000000", value_parser=parse_color)]
    background: Color32,

    /// Accent color in #hex or color space
    #[arg(short, long, default_value = "oklch 70% 60% 95", value_parser=parse_color)]
    accent: Color32,

    /// Background opacity 0.0 -> 1.0
    #[arg(short, long, default_value = "0.8")]
    opacity: f32,

    /// Override scale factor from environment variables.
    /// Applies on top of desktop/system scale factor.
    /// Currently reads GDK_DPI_SCALE, GDK_SCALE
    #[arg(short, long)]
    scale: Option<f32>,

    /// Match literal text as opposed to regular expressions
    #[arg(short, long)]
    literal: bool,

    /// Close linch on focus loss
    #[arg(short, long)]
    exit_unfocus: bool,

    /// Override cache name.
    /// If unset defaults to command name.
    /// If set to nothing "" caching isn't used
    #[arg(long)]
    cache: Option<String>,

    /// Removes all cached entries for given cache
    #[arg(long)]
    clear_cache: bool,
} // }}}

fn response(
    items: Vec<Item>,
    custom: bool,
    cache: String,
    args: LinchArgs,
    icons: bool,
    monochrome: bool,
) -> Option<Item> {
    // {{{
    let result: Arc<Mutex<Option<Item>>> = Arc::new(Mutex::new(None));
    let res_send = result.clone();
    let scale = args.scale.unwrap_or(scale_factor());
    if args.clear_cache {
        remove_file(cache_file(&cache)).unwrap();
    }
    eframe::run_native(
        "Linch",
        NativeOptions {
            viewport: ViewportBuilder::default()
                .with_decorations(false)
                .with_inner_size((args.width * scale, args.height * scale))
                .with_resizable(false)
                .with_transparent(if args.opacity < 1.0 { true } else { false })
                .with_window_level(WindowLevel::AlwaysOnTop),
            centered: true,
            ..Default::default()
        },
        Box::new(move |cc| {
            Box::new(Linch::new(
                cc,
                items,
                res_send,
                custom,
                cache,
                args.prompt,
                args.columns.into(),
                args.rows.into(),
                args.foreground,
                args.background,
                args.accent,
                args.opacity,
                scale,
                args.literal,
                args.exit_unfocus,
                icons,
                monochrome,
                [args.width, args.height],
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
            #[cfg(debug_assertions)]
            let now = std::time::Instant::now();
            let items = get_binaries();
            #[cfg(debug_assertions)]
            println!("{} items found in {:?}", items.len(), now.elapsed());
            if let Some(item) = response(
                items,
                false,
                args.cache.clone().unwrap_or(String::from("bin")),
                args,
                false,
                false,
            ) {
                let mut command = std::process::Command::new(item.as_ref());
                if let Err(e) = command.spawn() {
                    panic!(
                        "Could not start process {}\n{}",
                        command.get_program().to_string_lossy(),
                        e
                    );
                };
            }
        }
        LinchCmd::App { all, monochrome } => {
            #[cfg(debug_assertions)]
            let now = std::time::Instant::now();
            let items = get_applications(all);
            #[cfg(debug_assertions)]
            println!("{} items found in {:?}", items.len(), now.elapsed());
            if let Some(item) = response(
                items,
                false,
                args.cache.clone().unwrap_or(String::from("app")),
                args,
                true,
                monochrome,
            ) {
                let file = item.file.unwrap();
                for launcher in [
                    std::process::Command::new("dex").arg(&file),
                    std::process::Command::new("gio").arg("launch").arg(&file),
                    std::process::Command::new("exo-open").arg(&file),
                ] {
                    if launcher.spawn().is_ok() {
                        return;
                    }
                }
                eprintln!("All featured launchers failed. Falling back to gtk-launch");
                match std::process::Command::new("gtk-launch")
                    .arg(file.file_stem().unwrap())
                    .spawn()
                {
                    Ok(mut child) => {
                        if child.wait().unwrap().success() {
                            return;
                        }
                    }
                    Err(e) => eprintln!("{}", e),
                }
                eprintln!("Falling back to manual desktop entry launching");
                if let Some(exec) = item.exec.as_ref() {
                    let items = exec.split_whitespace().collect::<Vec<&str>>();
                    let mut command = if let Some(mut path) = item.path.clone() {
                        path.push(items[0]);
                        std::process::Command::new(path)
                    } else {
                        std::process::Command::new(items[0])
                    };
                    if let Some(args) = items.get(1..) {
                        command.args(args);
                    }
                    if let Err(err_exec) = command.spawn() {
                        eprintln!("Starting application directly failed: {}", err_exec);
                    }
                }
            }
        }
        LinchCmd::Dmenu => {
            let items: Vec<Item> = std::io::stdin()
                .lines()
                .filter_map(|r| match r.ok() {
                    Some(l) => {
                        if l.trim().is_empty() {
                            None
                        } else {
                            Some(Item {
                                name: l,
                                file: None,
                                exec: None,
                                path: None,
                                icon: None,
                                hidden: false,
                            })
                        }
                    }
                    None => None,
                })
                .collect();

            let custom = items.is_empty();
            if let Some(item) = response(items, custom, "".to_string(), args, false, false) {
                print!("{}", item);
            }
        }
    };
} // }}}
