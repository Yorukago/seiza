use chrono::{Datelike, Local, Months, NaiveDate};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Sense, TextEdit, Vec2};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, fs};

const BG: Color32 = Color32::from_rgb(0x1d, 0x1f, 0x2b);
const DOT: Color32 = Color32::from_rgb(0x44, 0x4a, 0x63);
const GOLD: Color32 = Color32::from_rgb(0xf2, 0xc4, 0x6d);
const DIM: Color32 = Color32::from_rgb(0x7a, 0x80, 0x9c);
const PAPER: Color32 = Color32::from_rgb(0xe6, 0xe1, 0xd3);

#[derive(PartialEq)]
enum Tab {
    Day,
    Mood,
    Habits,
}

struct App {
    dir: PathBuf,
    tab: Tab,
    day: NaiveDate,
    month: NaiveDate, // always the 1st
    text: String,
    moods: BTreeMap<NaiveDate, [u8; 3]>,
    habits: Vec<(String, BTreeSet<NaiveDate>)>,
    new_habit: String,
    err: Option<String>,
}

/// Curly quotes and bujo bullets. Every replacement keeps the char count, so the cursor stays put.
fn smarten(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = match line {
            "* " => "• ",
            "- " => "– ",
            "x " => "✕ ",
            "o " => "○ ",
            l => l,
        };
        let mut prev = ' ';
        for c in line.chars() {
            out.push(match c {
                '\'' => '’',
                '"' if prev.is_whitespace() || "([{–—".contains(prev) => '“',
                '"' => '”',
                c => c,
            });
            prev = c;
        }
    }
    out
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn unhex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim();
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

fn shift_month(d: NaiveDate, delta: i32) -> NaiveDate {
    let m = Months::new(delta.unsigned_abs());
    let r = if delta < 0 { d.checked_sub_months(m) } else { d.checked_add_months(m) };
    r.unwrap_or(d)
}

fn days_in_month(first: NaiveDate) -> u32 {
    (shift_month(first, 1) - first).num_days() as u32
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Style {
    bold: bool,
    italic: bool,
    marker: bool,
}

fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// Splits text on `**bold**` / `*italic*` markers, which stay in the text and are flagged so they can be dimmed.
/// shortcut: an unclosed marker styles the rest of its line, no escaping, upgrade if that ever bites.
fn styled(s: &str) -> Vec<(&str, Style)> {
    let mut out = Vec::new();
    let (mut bold, mut italic) = (false, false);
    let (mut start, mut i) = (0, 0);
    let st = |bold, italic, marker| Style { bold, italic, marker };
    while i < s.len() {
        let rest = &s[i..];
        let mark = if rest.starts_with("**") { 2 } else if rest.starts_with('*') { 1 } else { 0 };
        if mark > 0 || rest.starts_with('\n') {
            if start < i {
                out.push((&s[start..i], st(bold, italic, false)));
            }
            if mark == 2 {
                bold = !bold;
            } else if mark == 1 {
                italic = !italic;
            }
            let n = mark.max(1);
            let marker = mark > 0;
            if !marker {
                (bold, italic) = (false, false);
            }
            out.push((&s[i..i + n], st(bold, italic, marker)));
            i += n;
            start = i;
        } else {
            i += rest.chars().next().unwrap().len_utf8();
        }
    }
    if start < s.len() {
        out.push((&s[start..], st(bold, italic, false)));
    }
    out
}

fn layout(ui: &egui::Ui, text: &str, wrap: f32) -> Arc<egui::Galley> {
    let size = egui::TextStyle::Body.resolve(ui.style()).size;
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    for (t, st) in styled(text) {
        let family = if st.bold { bold_family() } else { FontFamily::Proportional };
        job.append(
            t,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::new(size, family),
                color: if st.marker { DIM } else { PAPER },
                italics: st.italic && !st.marker,
                ..Default::default()
            },
        );
    }
    ui.fonts_mut(|f| f.layout_job(job))
}

impl App {
    fn new(dir: PathBuf) -> Self {
        let today = Local::now().date_naive();
        let mut moods = BTreeMap::new();
        for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "mood") {
                let date = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok());
                let col = fs::read_to_string(&p).ok().and_then(|s| unhex(&s));
                if let (Some(d), Some(c)) = (date, col) {
                    moods.insert(d, c);
                }
            }
        }
        let habits = fs::read_to_string(dir.join("habits.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (name, days) = l.split_once('\t')?;
                Some((name.to_string(), days.split(',').filter_map(|d| d.parse().ok()).collect()))
            })
            .collect();
        let mut app = App {
            dir,
            tab: Tab::Day,
            day: today,
            month: today.with_day(1).unwrap(),
            text: String::new(),
            moods,
            habits,
            new_habit: String::new(),
            err: None,
        };
        app.load_day();
        app
    }

    fn write(&mut self, name: String, content: String) {
        match fs::write(self.dir.join(&name), content) {
            Ok(()) => self.err = None,
            Err(e) => self.err = Some(format!("could not save {name}: {e}")),
        }
    }

    fn load_day(&mut self) {
        self.text = fs::read_to_string(self.dir.join(format!("{}.txt", self.day))).unwrap_or_default();
    }

    fn open_day(&mut self, d: NaiveDate) {
        self.day = d;
        self.load_day();
    }

    fn save_habits(&mut self) {
        let s: String = self
            .habits
            .iter()
            .map(|(n, days)| {
                let days: Vec<_> = days.iter().map(|d| d.to_string()).collect();
                format!("{n}\t{}\n", days.join(","))
            })
            .collect();
        self.write("habits.txt".into(), s);
    }

    fn month_nav(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("◀").clicked() {
                self.month = shift_month(self.month, -1);
            }
            ui.heading(self.month.format("%B %Y").to_string());
            if ui.button("▶").clicked() {
                self.month = shift_month(self.month, 1);
            }
        });
    }

    fn day_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("◀").clicked() {
                self.open_day(self.day.pred_opt().unwrap_or(self.day));
            }
            ui.heading(self.day.format("%a %e %b %Y").to_string());
            if ui.button("▶").clicked() {
                self.open_day(self.day.succ_opt().unwrap_or(self.day));
            }
            if ui.button("today").clicked() {
                self.open_day(Local::now().date_naive());
            }
            ui.separator();
            ui.label("mood");
            let mut col = self.moods.get(&self.day).copied().unwrap_or([0x6b, 0x7f, 0xd7]);
            if ui.color_edit_button_srgb(&mut col).changed() {
                self.moods.insert(self.day, col);
                self.write(format!("{}.mood", self.day), hex(col));
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| layout(ui, buf.as_str(), wrap);
            let r = ui.add(
                TextEdit::multiline(&mut self.text)
                    .layouter(&mut layouter)
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .desired_rows(30)
                    .hint_text("* task  - note  x done  o event  > moved  **bold**  *italic*  ... or just vent"),
            );
            if r.changed() {
                self.text = smarten(&self.text);
                self.write(format!("{}.txt", self.day), self.text.clone());
            }
        });
    }

    fn mood_tab(&mut self, ui: &mut egui::Ui) {
        self.month_nav(ui);
        ui.add_space(8.0);
        let lead = self.month.weekday().num_days_from_monday();
        let n = days_in_month(self.month);
        let mut open = None;
        egui::Grid::new("mood").spacing([6.0, 6.0]).show(ui, |ui| {
            for d in ["M", "T", "W", "T", "F", "S", "S"] {
                ui.label(d);
            }
            ui.end_row();
            for i in 0..(lead + n).div_ceil(7) * 7 {
                if i >= lead && i - lead < n {
                    let date = self.month.with_day(i - lead + 1).unwrap();
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
                    let col = self.moods.get(&date).map_or(DOT, |c| Color32::from_rgb(c[0], c[1], c[2]));
                    ui.painter().circle_filled(r.center(), 19.0, col);
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, date.day().to_string(), egui::FontId::proportional(13.0), BG);
                    if resp.clicked() {
                        open = Some(date);
                    }
                } else {
                    ui.allocate_space(Vec2::splat(44.0));
                }
                if i % 7 == 6 {
                    ui.end_row();
                }
            }
        });
        if let Some(d) = open {
            self.open_day(d);
            self.tab = Tab::Day;
        }
    }

    fn habits_tab(&mut self, ui: &mut egui::Ui) {
        self.month_nav(ui);
        ui.add_space(8.0);
        let n = days_in_month(self.month);
        let mut toggled = false;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("habits").spacing([4.0, 6.0]).show(ui, |ui| {
                ui.label("");
                for d in 1..=n {
                    ui.label(d.to_string());
                }
                ui.end_row();
                for (name, days) in &mut self.habits {
                    ui.label(name.as_str());
                    for d in 1..=n {
                        let date = self.month.with_day(d).unwrap();
                        let on = days.contains(&date);
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        let c = r.center();
                        if on {
                            ui.painter().circle_filled(c, 8.0, GOLD);
                        } else {
                            ui.painter().circle_stroke(c, 8.0, egui::Stroke::new(1.5, DOT));
                        }
                        if resp.clicked() {
                            if !days.remove(&date) {
                                days.insert(date);
                            }
                            toggled = true;
                        }
                    }
                    ui.end_row();
                }
            });
        });
        if toggled {
            self.save_habits();
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut self.new_habit).hint_text("new habit"));
            let name = self.new_habit.replace('\t', " ").trim().to_string();
            if ui.button("add").clicked() && !name.is_empty() && !self.habits.iter().any(|(n, _)| *n == name) {
                self.habits.push((name, BTreeSet::new()));
                self.new_habit.clear();
                self.save_habits();
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        let bg = ctx.layer_painter(egui::LayerId::background());
        bg.rect_filled(screen, 0.0, BG);
        let mut y = screen.top();
        while y < screen.bottom() {
            let mut x = screen.left();
            while x < screen.right() {
                bg.circle_filled(egui::pos2(x, y), 1.2, DOT);
                x += 28.0;
            }
            y += 28.0;
        }

        let clear = egui::Frame::NONE.inner_margin(10.0);
        egui::Panel::top("tabs").frame(clear).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Day, "day");
                ui.selectable_value(&mut self.tab, Tab::Mood, "mood");
                ui.selectable_value(&mut self.tab, Tab::Habits, "habits");
                if let Some(e) = &self.err {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
            });
        });
        egui::CentralPanel::default().frame(clear).show(ui, |ui| match self.tab {
            Tab::Day => self.day_tab(ui),
            Tab::Mood => self.mood_tab(ui),
            Tab::Habits => self.habits_tab(ui),
        });
    }
}

fn setup(ctx: &egui::Context) {
    let mut f = FontDefinitions::default();
    let mut add = |name: &str, d: FontData| f.font_data.insert(name.into(), Arc::new(d));
    add("kalam", FontData::from_static(include_bytes!("../fonts/Kalam-Regular.ttf")));
    add("kalam-bold", FontData::from_static(include_bytes!("../fonts/Kalam-Bold.ttf")));
    // DejaVu has the bullet glyphs (✕ ○ •) Kalam lacks; fine to skip if absent.
    if let Ok(bytes) = fs::read("/usr/share/fonts/TTF/DejaVuSans.ttf") {
        add("sym", FontData::from_owned(bytes));
        f.families.entry(FontFamily::Proportional).or_default().push("sym".into());
    }
    let mut bold = f.families[&FontFamily::Proportional].clone();
    bold.insert(0, "kalam-bold".into());
    f.families.insert(bold_family(), bold);
    f.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "kalam".into());
    ctx.set_fonts(f);
    ctx.global_style_mut(|s| {
        for f in s.text_styles.values_mut() {
            f.size *= 1.3;
        }
        s.visuals.override_text_color = Some(PAPER);
    });
}

fn main() -> eframe::Result {
    let dir = PathBuf::from(env::var("HOME").expect("HOME not set")).join("diary-data");
    fs::create_dir_all(&dir).expect("could not create ~/diary-data");
    eframe::run_native(
        "diary",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            setup(&cc.egui_ctx);
            let mut app = App::new(dir);
            // `seiza-diary 2026-11-01` opens that day; anything unparsable opens today.
            if let Some(d) = env::args().nth(1).and_then(|a| a.parse().ok()) {
                app.open_day(d);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_text() {
        assert_eq!(smarten("don't \"quote\" me"), "don’t “quote” me");
        assert_eq!(smarten("* \n- \nx \no \nox "), "• \n– \n✕ \n○ \nox ");
        assert_eq!(smarten("a\n\nb\n"), "a\n\nb\n");
        assert_eq!(smarten("\nhi"), "\nhi");
    }

    #[test]
    fn styled_spans() {
        let s = |bold, italic, marker| Style { bold, italic, marker };
        assert_eq!(
            styled("a **b** *c*\nd *e"),
            vec![
                ("a ", s(false, false, false)),
                ("**", s(true, false, true)),
                ("b", s(true, false, false)),
                ("**", s(false, false, true)),
                (" ", s(false, false, false)),
                ("*", s(false, true, true)),
                ("c", s(false, true, false)),
                ("*", s(false, false, true)),
                ("\n", s(false, false, false)),
                ("d ", s(false, false, false)),
                ("*", s(false, true, true)),
                ("e", s(false, true, false)),
            ]
        );
        assert!(styled("").is_empty());
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(unhex(&hex([1, 171, 255])), Some([1, 171, 255]));
        assert_eq!(unhex("zzzzzz"), None);
    }
}
