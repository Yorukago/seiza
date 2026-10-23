//! The constellation editor's screen: figures on the left, a canvas to draw on in the
//! middle, the current constellation's stars on the right. No Wayland in here, so the
//! tests and `--dump` can run it.

use seiza_common::{
    draw::{load_font, Canvas, Text},
    figure::{self, Figure},
    palette::{palette, rgba},
};

use crate::model::Editor;

pub const W: f32 = 1180.0;
pub const H: f32 = 680.0;

#[derive(Clone, Copy)]
struct R(f32, f32, f32, f32);

impl R {
    fn has(&self, x: f32, y: f32) -> bool {
        x >= self.0 && x < self.0 + self.2 && y >= self.1 && y < self.1 + self.3
    }
}

const CANVAS: R = R(250.0, 100.0, 560.0, 410.0);
const FIG_NAME: R = R(250.0, 534.0, 250.0, 38.0);
const SAVE: R = R(516.0, 534.0, 84.0, 38.0);
const APPLY: R = R(608.0, 534.0, 84.0, 38.0);
const DELETE: R = R(700.0, 534.0, 84.0, 38.0);
const ADD_STAR: R = R(840.0, 424.0, 150.0, 38.0);
const REMOVE_STAR: R = R(1006.0, 424.0, 150.0, 38.0);
const STAR_NAME: R = R(840.0, 480.0, 230.0, 38.0);
const RENAME: R = R(1078.0, 480.0, 78.0, 38.0);
const FIG_ROWS: usize = 14;
const STAR_ROWS: usize = 9;
const POINT_REACH: f32 = 14.0;

fn fig_row(i: usize) -> R {
    R(24.0, 100.0 + 32.0 * i as f32, 196.0, 30.0) // row 0 is "+ new"
}

fn star_row(i: usize) -> R {
    R(840.0, 100.0 + 34.0 * i as f32, 316.0, 32.0)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Field {
    Figure,
    Star,
}

pub enum Effect {
    None,
    Run(Vec<String>), // commands for Seiza, in order
}

#[derive(PartialEq)]
enum Target {
    Figure(usize),
    NewFigure,
    Canvas,
    Field(Field),
    Save,
    Apply,
    Delete,
    AddStar,
    RemoveStar,
    Rename,
    Star(usize),
    Nothing,
}

pub struct Ui {
    pub editor: Editor,
    pub names: Vec<String>, // the figures there are
    pub name: String,       // what the figure is called (the field under the canvas)
    pub focus: Option<Field>,
    pub stars: Vec<(String, u64)>, // the current constellation's stars: name, windows
    pub current: String,
    pub star_sel: usize,
    pub star_name: String,
    pub msg: String,
    drag: Option<(usize, bool)>, // the point being dragged, and whether it moved
    t22: Text,
    t17: Text,
    t13: Text,
}

fn valid_star_name(s: &str) -> bool {
    !s.is_empty() && s.len() < 32 && !s.chars().any(char::is_control)
}

impl Ui {
    pub fn new() -> Ui {
        let font = load_font();
        Ui {
            editor: Editor::default(),
            names: figure::list(),
            name: String::new(),
            focus: None,
            stars: vec![],
            current: String::new(),
            star_sel: 0,
            star_name: String::new(),
            msg: "click the dark to place a star..".into(),
            drag: None,
            t22: Text::new(font.clone(), 24.0),
            t17: Text::new(font.clone(), 17.0),
            t13: Text::new(font, 13.0),
        }
    }

    pub fn set_stars(&mut self, stars: Vec<(String, u64)>, current: String) {
        self.star_sel = self.star_sel.min(stars.len().saturating_sub(1));
        self.stars = stars;
        self.current = current;
    }

    fn origin(w: f32, h: f32) -> (f32, f32) {
        ((w - W) / 2.0, (h - H) / 2.0)
    }

    fn target(&self, x: f32, y: f32) -> Target {
        for (i, r) in [(0usize, fig_row(0))].into_iter().chain((0..self.names.len().min(FIG_ROWS - 1)).map(|i| (i + 1, fig_row(i + 1)))) {
            if r.has(x, y) {
                return if i == 0 { Target::NewFigure } else { Target::Figure(i - 1) };
            }
        }
        for i in 0..self.stars.len().min(STAR_ROWS) {
            if star_row(i).has(x, y) {
                return Target::Star(i);
            }
        }
        for (r, t) in [(FIG_NAME, Target::Field(Field::Figure)), (STAR_NAME, Target::Field(Field::Star)), (SAVE, Target::Save), (APPLY, Target::Apply), (DELETE, Target::Delete), (ADD_STAR, Target::AddStar), (REMOVE_STAR, Target::RemoveStar), (RENAME, Target::Rename), (CANVAS, Target::Canvas)] {
            if r.has(x, y) {
                return t;
            }
        }
        Target::Nothing
    }

    /// A screen point on the canvas as figure coordinates; both axes use the canvas width, so a shape keeps its shape.
    fn to_uv(x: f32, y: f32) -> (f32, f32) {
        ((x - CANVAS.0) / CANVAS.2, (y - CANVAS.1) / CANVAS.2)
    }

    fn to_screen(u: f32, v: f32) -> (f32, f32) {
        (CANVAS.0 + u * CANVAS.2, CANVAS.1 + v * CANVAS.2)
    }

    fn point_near_screen(&self, x: f32, y: f32) -> Option<usize> {
        let (u, v) = Self::to_uv(x, y);
        self.editor.point_near(u, v, POINT_REACH / CANVAS.2)
    }

    pub fn load_figure(&mut self, name: &str) {
        if let Some(f) = figure::load(name) {
            self.editor = Editor::from_figure(f);
            let margin = 56.0 / CANVAS.2; // keep the stars off the canvas edge
            self.editor.fit((CANVAS.2 - 112.0) / CANVAS.2, (CANVAS.3 - 112.0) / CANVAS.2);
            for p in &mut self.editor.fig.points {
                *p = (p.0 + margin, p.1 + margin);
            }
            self.name = name.to_string();
            self.msg = format!("{name}: drag the stars, click one then another to join them");
        }
    }

    fn save(&mut self) -> bool {
        match figure::save(&self.name, &self.editor.fig) {
            Ok(()) => {
                self.names = figure::list();
                self.msg = format!("saved {}..", self.name);
                true
            }
            Err(e) => {
                self.msg = format!("can't save: {e}");
                false
            }
        }
    }

    /// A press at a screen point (`right` for the right button).
    pub fn press(&mut self, px: f32, py: f32, w: f32, h: f32, right: bool) -> Effect {
        let (ox, oy) = Self::origin(w, h);
        let (x, y) = (px - ox, py - oy);
        let target = self.target(x, y);
        if !matches!(target, Target::Field(_)) {
            self.focus = None;
        }
        match target {
            Target::Figure(i) => {
                if let Some(n) = self.names.get(i).cloned() {
                    self.load_figure(&n);
                }
            }
            Target::NewFigure => {
                self.editor = Editor::default();
                self.name.clear();
                self.msg = "a new sky. click to place the first star..".into();
            }
            Target::Canvas => match (self.point_near_screen(x, y), right) {
                (Some(i), true) => self.editor.remove_point(i),
                (Some(i), false) => self.drag = Some((i, false)),
                (None, false) => {
                    let (u, v) = Self::to_uv(x, y);
                    if self.editor.add_point(u, v).is_none() {
                        self.msg = "that's as many stars as a figure can hold..".into();
                    }
                }
                (None, true) => self.editor.selected = None,
            },
            Target::Field(f) => self.focus = Some(f),
            Target::Save => {
                self.save();
            }
            Target::Apply => {
                if self.save() {
                    return Effect::Run(vec![format!("seiza figure {}", self.name)]);
                }
            }
            Target::Delete => match figure::delete(&self.name) {
                Ok(()) => {
                    self.names = figure::list();
                    self.msg = format!("{} is gone..", self.name);
                }
                Err(_) => self.msg = "only figures you saved can be deleted..".into(),
            },
            Target::AddStar => return Effect::Run(vec!["seiza new-star".into()]),
            Target::RemoveStar => {
                if !self.stars.is_empty() {
                    return Effect::Run(vec![format!("seiza delete-star {}", self.star_sel + 1)]);
                }
            }
            Target::Rename => {
                if valid_star_name(self.star_name.trim()) && !self.stars.is_empty() {
                    return Effect::Run(vec![format!("seiza rename-star {} {}", self.star_sel + 1, self.star_name.trim())]);
                }
                self.msg = "a star's name is 1 to 31 letters..".into();
            }
            Target::Star(i) => {
                self.star_sel = i;
                self.star_name = self.stars[i].0.clone();
            }
            Target::Nothing => {}
        }
        Effect::None
    }

    pub fn drag_to(&mut self, px: f32, py: f32, w: f32, h: f32) {
        let Some((i, _)) = self.drag else { return };
        let (ox, oy) = Self::origin(w, h);
        let (x, y) = ((px - ox).clamp(CANVAS.0 + 8.0, CANVAS.0 + CANVAS.2 - 8.0), (py - oy).clamp(CANVAS.1 + 8.0, CANVAS.1 + CANVAS.3 - 8.0));
        let (u, v) = Self::to_uv(x, y);
        self.editor.move_point(i, u, v);
        self.drag = Some((i, true));
    }

    /// A drag that never moved was a click.
    pub fn release(&mut self) {
        if let Some((i, moved)) = self.drag.take() {
            if !moved {
                self.editor.click_point(i);
            }
        }
    }

    pub fn type_text(&mut self, s: &str) {
        match self.focus {
            Some(Field::Figure) => self.name.extend(s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(31usize.saturating_sub(self.name.len()))),
            Some(Field::Star) => {
                if self.star_name.len() + s.len() < 32 {
                    self.star_name.push_str(s);
                }
            }
            None => {}
        }
    }

    pub fn backspace(&mut self) -> bool {
        match self.focus {
            Some(Field::Figure) => self.name.pop().is_some(),
            Some(Field::Star) => self.star_name.pop().is_some(),
            None => false,
        }
    }

    /// Delete with nothing being typed: the selected point goes.
    pub fn delete_selected(&mut self) {
        if let Some(s) = self.editor.selected {
            self.editor.remove_point(s);
        }
    }

    /// Enter in a field does what its button does.
    pub fn enter(&mut self, w: f32, h: f32) -> Effect {
        let (ox, oy) = Self::origin(w, h);
        let r = match self.focus {
            Some(Field::Figure) => SAVE,
            Some(Field::Star) => RENAME,
            None => return Effect::None,
        };
        self.press(ox + r.0 + 4.0, oy + r.1 + 4.0, w, h, false)
    }

    fn button(&mut self, cv: &mut Canvas, r: R, ox: f32, oy: f32, label: &str, lit: bool) {
        let p = palette();
        let (x, y) = (ox + r.0, oy + r.1);
        cv.round_rect(x, y, r.2, r.3, r.3 / 2.0, if lit { rgba(p.star, 0.95) } else { rgba(p.dust, 0.9) });
        let tw = self.t17.width(label);
        self.t17.draw(cv, x + r.2 / 2.0 - tw / 2.0, y + r.3 / 2.0 + 6.0, label, if lit { rgba(p.sky, 1.0) } else { rgba(p.text, 0.9) });
    }

    fn field(&mut self, cv: &mut Canvas, r: R, ox: f32, oy: f32, text: &str, placeholder: &str, focused: bool) {
        let p = palette();
        let (x, y) = (ox + r.0, oy + r.1);
        cv.round_rect(x - 1.0, y - 1.0, r.2 + 2.0, r.3 + 2.0, r.3 / 2.0 + 1.0, rgba(p.star, if focused { 0.7 } else { 0.2 }));
        cv.round_rect(x, y, r.2, r.3, r.3 / 2.0, rgba(p.sky, 0.8));
        let (shown, dim) = if text.is_empty() && !focused { (placeholder, 0.3) } else { (text, 1.0) };
        let end = self.t17.draw(cv, x + 18.0, y + r.3 / 2.0 + 6.0, shown, rgba(p.text, dim));
        if focused {
            cv.round_rect(end + 1.0, y + 9.0, 1.5, r.3 - 18.0, 0.0, rgba(p.text, 0.9));
        }
    }

    /// An inked thread: a gentle arc drawn twice, a little wobbly.
    fn thread(cv: &mut Canvas, a: (f32, f32), b: (f32, f32), seed: usize) {
        let p = palette();
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy).max(1.0);
        let bend = len * (0.06 + (seed % 4) as f32 * 0.015) * if seed % 2 == 0 { 1.0 } else { -1.0 };
        let ctrl = ((a.0 + b.0) / 2.0 - dy / len * bend, (a.1 + b.1) / 2.0 + dx / len * bend);
        for pass in 0..2 {
            let steps = (len / 1.6).ceil() as i32;
            for s in 0..=steps {
                let t = s as f32 / steps as f32;
                let (u, v) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t));
                let wob = if pass == 0 { 0.0 } else { 1.5 * (t * 9.0 + seed as f32).sin() + 1.2 };
                let (x, y) = (u * a.0 + v * ctrl.0 + t * t * b.0 - dy / len * wob, u * a.1 + v * ctrl.1 + t * t * b.1 + dx / len * wob);
                cv.round_rect(x - 0.8, y - 0.8, 1.6, 1.6, 0.8, rgba(p.star, if pass == 0 { 0.8 } else { 0.35 }));
            }
        }
    }

    pub fn render(&mut self, w: usize, h: usize) -> Canvas {
        let p = palette();
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let a = 0.86;
        let ch = |v: f32| (v * a * 255.0) as u32;
        cv.px.fill(((a * 255.0) as u32) << 24 | ch(p.sky[0]) << 16 | ch(p.sky[1]) << 8 | ch(p.sky[2]));
        let (ox, oy) = Self::origin(w as f32, h as f32);
        cv.round_rect(ox - 1.0, oy - 1.0, W + 2.0, H + 2.0, 25.0, rgba(p.star, 0.3));
        cv.round_rect(ox, oy, W, H, 24.0, rgba(p.panel, 1.0));

        self.t22.draw(&mut cv, ox + 24.0, oy + 54.0, "constellations", rgba(p.text, 1.0));
        self.t13.draw(&mut cv, ox + 250.0, oy + 80.0, "draw your own, then lay your stars out on it.", rgba(p.text, 0.5));

        // the figures
        let rows: Vec<String> = std::iter::once("+ new".to_string()).chain(self.names.iter().take(FIG_ROWS - 1).cloned()).collect();
        for (i, name) in rows.iter().enumerate() {
            let r = fig_row(i);
            let on = i > 0 && *name == self.name;
            if on {
                cv.round_rect(ox + r.0, oy + r.1, r.2, r.3, 12.0, rgba(p.dust, 0.9));
            }
            let fit = if figure::is_built_in(name) { "" } else if i > 0 { "  *" } else { "" };
            self.t17.draw(&mut cv, ox + r.0 + 14.0, oy + r.1 + 21.0, &format!("{name}{fit}"), rgba(p.text, if on || i == 0 { 1.0 } else { 0.7 }));
        }
        self.t13.draw(&mut cv, ox + 24.0, oy + 100.0 + 32.0 * FIG_ROWS as f32 + 18.0, "* drawn by you", rgba(p.text, 0.35));

        // the canvas
        cv.round_rect(ox + CANVAS.0 - 1.0, oy + CANVAS.1 - 1.0, CANVAS.2 + 2.0, CANVAS.3 + 2.0, 21.0, rgba(p.star, 0.18));
        cv.round_rect(ox + CANVAS.0, oy + CANVAS.1, CANVAS.2, CANVAS.3, 20.0, rgba(p.sky, 0.75));
        let fig: Figure = self.editor.fig.clone();
        let at = |i: usize| {
            let (x, y) = Self::to_screen(fig.points[i].0, fig.points[i].1);
            (ox + x, oy + y)
        };
        for (n, &(a, b)) in fig.links.iter().enumerate() {
            Self::thread(&mut cv, at(a), at(b), n);
        }
        for i in 0..fig.points.len() {
            let (x, y) = at(i);
            let sel = self.editor.selected == Some(i);
            let col = if sel { p.ice } else { p.star };
            let r = if sel { 11.0 } else { 8.0 };
            for k in (1..8).rev() {
                let rr = r * (1.0 + k as f32 * 0.5);
                cv.round_rect(x - rr, y - rr, rr * 2.0, rr * 2.0, rr, rgba(col, 0.05));
            }
            cv.sparkle(x, y, r, rgba(col, 1.0));
            self.t13.draw(&mut cv, x + 12.0, y - 10.0, &(i + 1).to_string(), rgba(p.text, 0.45));
        }
        if fig.points.is_empty() {
            let s = "click anywhere to place a star";
            let sw = self.t17.width(s);
            self.t17.draw(&mut cv, ox + CANVAS.0 + CANVAS.2 / 2.0 - sw / 2.0, oy + CANVAS.1 + CANVAS.3 / 2.0, s, rgba(p.text, 0.3));
        }

        // name, save, apply, delete
        let (name, focus) = (self.name.clone(), self.focus);
        self.field(&mut cv, FIG_NAME, ox, oy, &name, "name this figure", focus == Some(Field::Figure));
        let ready = figure::name_ok(&self.name) && !self.editor.fig.points.is_empty();
        self.button(&mut cv, SAVE, ox, oy, "save", ready);
        self.button(&mut cv, APPLY, ox, oy, "apply", false);
        self.button(&mut cv, DELETE, ox, oy, "delete", false);

        // the stars of the constellation
        self.t17.draw(&mut cv, ox + 840.0, oy + 80.0, "stars", rgba(p.text, 0.7));
        let stars = self.stars.clone();
        for (i, (name, windows)) in stars.iter().take(STAR_ROWS).enumerate() {
            let r = star_row(i);
            if i == self.star_sel {
                cv.round_rect(ox + r.0, oy + r.1, r.2, r.3, 12.0, rgba(p.dust, 0.9));
            }
            if *name == self.current {
                cv.sparkle(ox + r.0 + 18.0, oy + r.1 + 16.0, 6.0, rgba(p.star, 1.0));
            }
            let shown = if name.len() > 20 { format!("{}..", &name[..18]) } else { name.clone() };
            self.t17.draw(&mut cv, ox + r.0 + 36.0, oy + r.1 + 22.0, &shown, rgba(p.text, 1.0));
            if *windows > 0 {
                let s = format!("{windows} app{}", if *windows == 1 { "" } else { "s" });
                let sw = self.t13.width(&s);
                self.t13.draw(&mut cv, ox + r.0 + r.2 - 14.0 - sw, oy + r.1 + 21.0, &s, rgba(p.text, 0.5));
            }
        }
        if stars.is_empty() {
            self.t13.draw(&mut cv, ox + 840.0, oy + 124.0, "waiting for seiza..", rgba(p.text, 0.4));
        }
        self.button(&mut cv, ADD_STAR, ox, oy, "add a star", true);
        self.button(&mut cv, REMOVE_STAR, ox, oy, "remove it", false);
        let (sn, focus) = (self.star_name.clone(), self.focus);
        self.field(&mut cv, STAR_NAME, ox, oy, &sn, "rename this star", focus == Some(Field::Star));
        self.button(&mut cv, RENAME, ox, oy, "ok", false);

        self.t13.draw(&mut cv, ox + 250.0, oy + H - 30.0, &self.msg.clone(), rgba(p.text, 0.55));
        let ew = self.t13.width("esc to close");
        self.t13.draw(&mut cv, ox + W - 32.0 - ew, oy + H - 30.0, "esc to close", rgba(p.text, 0.35));
        cv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Press at a point given in panel coordinates, on a 1920x1080 screen.
    fn press(ui: &mut Ui, x: f32, y: f32, right: bool) -> Effect {
        let (ox, oy) = Ui::origin(1920.0, 1080.0);
        ui.press(ox + x, oy + y, 1920.0, 1080.0, right)
    }

    fn at_config<R>(f: impl FnOnce() -> R) -> R {
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(()); // tests share one environment variable and one directory
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("seiza-constellations-test-{}", std::process::id()));
        // SAFETY: the mutex above keeps the tests from touching the environment at once.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        let r = f();
        let _ = std::fs::remove_dir_all(dir);
        r
    }

    #[test]
    fn drawing_a_figure_saving_it_and_applying_it() {
        at_config(|| {
            let mut ui = Ui::new();
            // three clicks on the canvas make a chain
            for (x, y) in [(300.0, 150.0), (420.0, 260.0), (560.0, 180.0)] {
                press(&mut ui, x, y, false);
            }
            assert_eq!((ui.editor.fig.points.len(), ui.editor.fig.links.len()), (3, 2));
            // no name yet: saving says no, and applying does not ask Seiza for anything
            assert!(matches!(press(&mut ui, 560.0, 550.0, false), Effect::None));
            assert!(ui.msg.starts_with("can't save"));
            // type a name in its field, then apply
            press(&mut ui, 300.0, 550.0, false);
            ui.type_text("my sky!");
            assert_eq!(ui.name, "mysky", "only letters, digits, _ and - go in a figure's name");
            let Effect::Run(cmds) = press(&mut ui, 650.0, 550.0, false) else { panic!("apply should ask Seiza to lay the stars out") };
            assert_eq!(cmds, ["seiza figure mysky"]);
            assert_eq!(figure::load("mysky").unwrap().points.len(), 3);
            assert!(figure::list().contains(&"mysky".to_string()));
            // delete takes the file away again
            press(&mut ui, 740.0, 550.0, false);
            assert!(figure::load("mysky").is_none());
        });
    }

    #[test]
    fn dragging_moves_a_point_and_a_plain_click_selects_it() {
        at_config(|| {
            let mut ui = Ui::new();
            press(&mut ui, 400.0, 300.0, false);
            ui.editor.selected = None;
            press(&mut ui, 400.0, 300.0, false); // on the point: starts a drag
            ui.drag_to(1920.0 / 2.0 - W / 2.0 + 500.0, 1080.0 / 2.0 - H / 2.0 + 350.0, 1920.0, 1080.0);
            ui.release();
            let (x, y) = Ui::to_screen(ui.editor.fig.points[0].0, ui.editor.fig.points[0].1);
            assert!((x - 500.0).abs() < 1.0 && (y - 350.0).abs() < 1.0);
            assert_eq!(ui.editor.selected, None, "a drag is not a click");
            press(&mut ui, 500.0, 350.0, false);
            ui.release();
            assert_eq!(ui.editor.selected, Some(0));
            // right-click removes
            press(&mut ui, 500.0, 350.0, true);
            assert!(ui.editor.fig.points.is_empty());
        });
    }

    #[test]
    fn star_buttons_send_the_commands_for_the_chosen_star() {
        at_config(|| {
            let mut ui = Ui::new();
            ui.set_stars(vec![("star 0".into(), 2), ("star 1".into(), 0), ("star 2".into(), 0)], "star 0".into());
            press(&mut ui, 900.0, 100.0 + 34.0 + 10.0, false); // the second star
            assert_eq!((ui.star_sel, ui.star_name.as_str()), (1, "star 1"));
            let Effect::Run(c) = press(&mut ui, 1080.0, 440.0, false) else { panic!() };
            assert_eq!(c, ["seiza delete-star 2"]);
            press(&mut ui, 900.0, 500.0, false);
            ui.star_name.clear();
            ui.type_text("north");
            let Effect::Run(c) = press(&mut ui, 1110.0, 500.0, false) else { panic!() };
            assert_eq!(c, ["seiza rename-star 2 north"]);
            let Effect::Run(c) = press(&mut ui, 900.0, 440.0, false) else { panic!() };
            assert_eq!(c, ["seiza new-star"]);
        });
    }

    #[test]
    fn it_draws_without_a_figure_and_with_one() {
        at_config(|| {
            let mut ui = Ui::new();
            let empty = ui.render(1920, 1080);
            ui.load_figure("dipper");
            let drawn = ui.render(1920, 1080);
            assert_ne!(empty.px, drawn.px);
        });
    }
}
