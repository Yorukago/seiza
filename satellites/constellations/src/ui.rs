//! The constellation editor's screen: templates on the left, the live sky in the middle (it
//! IS the constellation: clicking it adds a star, dragging moves one, deleting removes it),
//! the stars as a list on the right. No Wayland in here, so the tests and `--dump` can run it.

use seiza_common::{
    draw::{load_font, Canvas, Text},
    figure,
    palette::{palette, rgba},
};

use crate::model::{Sky, Star, View, TERRITORY};

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
const CANVAS_SIZE: (f64, f64) = (560.0, 410.0);
const TEMPLATE_NAME: R = R(250.0, 534.0, 250.0, 38.0);
const SAVE: R = R(516.0, 534.0, 120.0, 38.0);
const DELETE: R = R(644.0, 534.0, 120.0, 38.0);
const REMOVE_STAR: R = R(840.0, 424.0, 316.0, 38.0);
const STAR_NAME: R = R(840.0, 480.0, 230.0, 38.0);
const RENAME: R = R(1078.0, 480.0, 78.0, 38.0);
const TEMPLATE_ROWS: usize = 14;
const STAR_ROWS: usize = 9;
const REACH: f64 = 16.0; // how near a click must be to a star on the canvas, in pixels

fn template_row(i: usize) -> R {
    R(24.0, 100.0 + 32.0 * i as f32, 196.0, 30.0)
}

fn star_row(i: usize) -> R {
    R(840.0, 100.0 + 34.0 * i as f32, 316.0, 32.0)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Field {
    Template,
    Star,
}

pub enum Effect {
    None,
    Run(Vec<String>), // commands for Seiza, in order; the first that fails stops the rest
}

#[derive(PartialEq)]
enum Target {
    Template(usize),
    Canvas,
    Field(Field),
    Save,
    Delete,
    RemoveStar,
    Rename,
    Star(usize),
    Nothing,
}

struct Drag {
    star: usize,
    moved: bool,
    at: (f64, f64), // where it is now, in the world
}

pub struct Ui {
    pub sky: Sky,
    view: View,
    fitted: bool,
    pub names: Vec<String>, // the templates there are
    pub name: String,       // what the template being saved is called
    armed: Option<String>,  // a template picked once; picking it again lays the stars out on it
    pub refit_after: bool,  // the next news of the sky should refit the view: the stars were just laid out somewhere else
    pub focus: Option<Field>,
    pub selected: Option<usize>,
    pub star_name: String,
    pub msg: String,
    drag: Option<Drag>,
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
            sky: Sky::default(),
            view: View::fit(&[], CANVAS_SIZE),
            fitted: false,
            names: figure::list(),
            name: String::new(),
            armed: None,
            refit_after: false,
            focus: None,
            selected: None,
            star_name: String::new(),
            msg: "click the dark to add a star..".into(),
            drag: None,
            t22: Text::new(font.clone(), 24.0),
            t17: Text::new(font.clone(), 17.0),
            t13: Text::new(font, 13.0),
        }
    }

    /// What Seiza says the sky is now. The canvas keeps its view unless a star would fall off it.
    pub fn set_sky(&mut self, sky: Sky) {
        let off = sky.stars.iter().any(|s| !self.view.holds(s.x, s.y));
        self.sky = sky;
        if !self.fitted || off || std::mem::take(&mut self.refit_after) {
            self.view = View::fit(&self.sky.stars, CANVAS_SIZE);
            self.fitted = !self.sky.stars.is_empty();
        }
        if self.selected.is_some_and(|s| s >= self.sky.stars.len()) {
            self.selected = None;
        }
    }

    fn origin(w: f32, h: f32) -> (f32, f32) {
        ((w - W) / 2.0, (h - H) / 2.0)
    }

    fn target(&self, x: f32, y: f32) -> Target {
        for i in 0..self.names.len().min(TEMPLATE_ROWS) {
            if template_row(i).has(x, y) {
                return Target::Template(i);
            }
        }
        for i in 0..self.sky.stars.len().min(STAR_ROWS) {
            if star_row(i).has(x, y) {
                return Target::Star(i);
            }
        }
        for (r, t) in [(TEMPLATE_NAME, Target::Field(Field::Template)), (STAR_NAME, Target::Field(Field::Star)), (SAVE, Target::Save), (DELETE, Target::Delete), (REMOVE_STAR, Target::RemoveStar), (RENAME, Target::Rename), (CANVAS, Target::Canvas)] {
            if r.has(x, y) {
                return t;
            }
        }
        Target::Nothing
    }

    fn canvas_xy(x: f32, y: f32) -> (f64, f64) {
        ((x - CANVAS.0) as f64, (y - CANVAS.1) as f64)
    }

    fn world(&self, x: f32, y: f32) -> (f64, f64) {
        let (cx, cy) = Self::canvas_xy(x, y);
        self.view.to_world(cx, cy, CANVAS_SIZE)
    }

    fn star_at_canvas(&self, x: f32, y: f32) -> Option<usize> {
        let (wx, wy) = self.world(x, y);
        self.sky.star_near(wx, wy, REACH / self.view.scale(CANVAS_SIZE))
    }

    fn select(&mut self, i: usize) {
        self.selected = Some(i);
        if let Some(s) = self.sky.stars.get(i) {
            self.star_name = s.name.clone();
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
        if !matches!(target, Target::Template(_)) {
            self.armed = None;
        }
        match target {
            Target::Template(i) => {
                let name = self.names[i].clone();
                self.name = name.clone();
                if self.armed.as_deref() == Some(&name) {
                    self.armed = None;
                    self.refit_after = true;
                    return Effect::Run(vec![format!("seiza figure {name}")]);
                }
                self.msg = format!("{name}: click it again to lay your stars out on it (it makes stars for any it needs)");
                self.armed = Some(name);
            }
            Target::Canvas => match (self.star_at_canvas(x, y), right) {
                (Some(i), true) => return self.remove_star(i),
                (Some(i), false) => {
                    let at = (self.sky.stars[i].x, self.sky.stars[i].y);
                    self.drag = Some(Drag { star: i, moved: false, at });
                }
                (None, false) => {
                    let (wx, wy) = self.world(x, y);
                    if self.sky.too_near(None, wx, wy) {
                        self.msg = "too close to another star: they each need their own room..".into();
                    } else {
                        let mut cmds = vec![format!("seiza add-star {wx:.0} {wy:.0}")];
                        let new = self.sky.stars.len();
                        if let Some(s) = self.selected {
                            cmds.push(format!("seiza link {} {}", s + 1, new + 1)); // clicking along draws a chain
                        }
                        self.selected = Some(new);
                        self.star_name = format!("star {new}");
                        return Effect::Run(cmds);
                    }
                }
                (None, true) => self.selected = None,
            },
            Target::Field(f) => self.focus = Some(f),
            Target::Save => {
                let f = self.sky.to_figure();
                self.msg = match figure::save(&self.name, &f) {
                    Ok(()) => {
                        self.names = figure::list();
                        format!("saved {} as a template..", self.name)
                    }
                    Err(e) => format!("can't save: {e}"),
                };
            }
            Target::Delete => match figure::delete(&self.name) {
                Ok(()) => {
                    self.names = figure::list();
                    self.msg = format!("{} is gone..", self.name);
                }
                Err(_) => self.msg = "only templates you saved can be deleted..".into(),
            },
            Target::RemoveStar => {
                if let Some(i) = self.selected {
                    return self.remove_star(i);
                }
                self.msg = "pick a star first..".into();
            }
            Target::Rename => match self.selected {
                Some(i) if valid_star_name(self.star_name.trim()) => return Effect::Run(vec![format!("seiza rename-star {} {}", i + 1, self.star_name.trim())]),
                Some(_) => self.msg = "a star's name is 1 to 31 letters..".into(),
                None => self.msg = "pick a star first..".into(),
            },
            Target::Star(i) => self.select(i),
            Target::Nothing => {}
        }
        Effect::None
    }

    fn remove_star(&mut self, i: usize) -> Effect {
        if self.sky.stars.get(i).is_some_and(|s| s.windows > 0) {
            self.msg = "that star has apps in it. close them or move them first..".into();
            return Effect::None;
        }
        if self.sky.stars.len() <= 1 {
            self.msg = "a constellation needs at least one star..".into();
            return Effect::None;
        }
        self.selected = None;
        Effect::Run(vec![format!("seiza delete-star {}", i + 1)])
    }

    pub fn drag_to(&mut self, px: f32, py: f32, w: f32, h: f32) {
        let (ox, oy) = Self::origin(w, h);
        let (x, y) = ((px - ox).clamp(CANVAS.0 + 6.0, CANVAS.0 + CANVAS.2 - 6.0), (py - oy).clamp(CANVAS.1 + 6.0, CANVAS.1 + CANVAS.3 - 6.0));
        let at = self.world(x, y);
        if let Some(d) = &mut self.drag {
            d.at = at;
            d.moved = true;
        }
    }

    /// A drag that moved puts the star there (if there is room); one that never moved was a click.
    pub fn release(&mut self) -> Effect {
        let Some(d) = self.drag.take() else { return Effect::None };
        if d.moved {
            if self.sky.too_near(Some(d.star), d.at.0, d.at.1) {
                self.msg = "no room there: stars keep their distance..".into();
                return Effect::None;
            }
            self.select(d.star);
            return Effect::Run(vec![format!("seiza move-star {} {:.0} {:.0}", d.star + 1, d.at.0, d.at.1)]);
        }
        match self.selected {
            Some(s) if s == d.star => self.selected = None,
            Some(s) => {
                self.select(d.star);
                return Effect::Run(vec![format!("seiza link {} {}", s + 1, d.star + 1)]); // joins them, or cuts the thread that was there
            }
            None => self.select(d.star),
        }
        Effect::None
    }

    pub fn type_text(&mut self, s: &str) {
        match self.focus {
            Some(Field::Template) => self.name.extend(s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(31usize.saturating_sub(self.name.len()))),
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
            Some(Field::Template) => self.name.pop().is_some(),
            Some(Field::Star) => self.star_name.pop().is_some(),
            None => false,
        }
    }

    /// Delete with nothing being typed: the selected star goes.
    pub fn delete_selected(&mut self) -> Effect {
        match self.selected {
            Some(i) => self.remove_star(i),
            None => Effect::None,
        }
    }

    /// Enter in a field does what its button does.
    pub fn enter(&mut self, w: f32, h: f32) -> Effect {
        let (ox, oy) = Self::origin(w, h);
        let r = match self.focus {
            Some(Field::Template) => SAVE,
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

    /// Where star `i` is on the screen (the dragged one follows the pointer).
    fn star_screen(&self, i: usize, ox: f32, oy: f32) -> (f32, f32) {
        let s: &Star = &self.sky.stars[i];
        let (wx, wy) = match &self.drag {
            Some(d) if d.star == i && d.moved => d.at,
            _ => (s.x, s.y),
        };
        let (cx, cy) = self.view.to_canvas(wx, wy, CANVAS_SIZE);
        (ox + CANVAS.0 + cx as f32, oy + CANVAS.1 + cy as f32)
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
        self.t13.draw(&mut cv, ox + 250.0, oy + 80.0, &format!("{}: click to add a star, drag to move it, click two to join them", self.view_name()), rgba(p.text, 0.5));

        // the templates
        self.t17.draw(&mut cv, ox + 24.0, oy + 80.0, "templates", rgba(p.text, 0.7));
        let names = self.names.clone();
        for (i, name) in names.iter().take(TEMPLATE_ROWS).enumerate() {
            let r = template_row(i);
            let armed = self.armed.as_deref() == Some(name);
            if armed {
                cv.round_rect(ox + r.0, oy + r.1, r.2, r.3, 12.0, rgba(p.dust, 0.9));
                cv.round_rect(ox + r.0, oy + r.1, r.2, r.3, 12.0, rgba(p.star, 0.12));
            }
            let mark = if figure::is_built_in(name) { "" } else { "  *" };
            self.t17.draw(&mut cv, ox + r.0 + 14.0, oy + r.1 + 21.0, &format!("{name}{mark}"), rgba(p.text, if armed { 1.0 } else { 0.7 }));
        }
        self.t13.draw(&mut cv, ox + 24.0, oy + 100.0 + 32.0 * TEMPLATE_ROWS as f32 + 18.0, "* saved by you", rgba(p.text, 0.35));

        // the sky
        cv.round_rect(ox + CANVAS.0 - 1.0, oy + CANVAS.1 - 1.0, CANVAS.2 + 2.0, CANVAS.3 + 2.0, 21.0, rgba(p.star, 0.18));
        cv.round_rect(ox + CANVAS.0, oy + CANVAS.1, CANVAS.2, CANVAS.3, 20.0, rgba(p.sky, 0.75));
        let n = self.sky.stars.len();
        let scale = self.view.scale(CANVAS_SIZE) as f32;
        let (tw, th) = (TERRITORY.0 as f32 * scale, TERRITORY.1 as f32 * scale);
        let pos: Vec<(f32, f32)> = (0..n).map(|i| self.star_screen(i, ox, oy)).collect();
        for &(x, y) in &pos { // the room each star keeps for itself
            cv.round_rect(x - tw / 2.0, y - th / 2.0, tw, th, 5.0, rgba(p.star, 0.10));
            cv.round_rect(x - tw / 2.0 + 1.0, y - th / 2.0 + 1.0, tw - 2.0, th - 2.0, 4.0, rgba(p.sky, 0.75));
        }
        let links = self.sky.links.clone();
        for (k, &(a, b)) in links.iter().enumerate() {
            if a < n && b < n {
                Self::thread(&mut cv, pos[a], pos[b], k);
            }
        }
        let names: Vec<String> = self.sky.stars.iter().map(|s| s.name.clone()).collect();
        for i in 0..n {
            let (x, y) = pos[i];
            let sel = self.selected == Some(i);
            let col = if sel { p.ice } else { p.star };
            let r = if sel { 11.0 } else { 8.0 };
            for k in (1..8).rev() {
                let rr = r * (1.0 + k as f32 * 0.5);
                cv.round_rect(x - rr, y - rr, rr * 2.0, rr * 2.0, rr, rgba(col, 0.05));
            }
            cv.sparkle(x, y, r, rgba(col, 1.0));
            let label: String = names[i].chars().take(14).collect();
            let lw = self.t13.width(&label);
            self.t13.draw(&mut cv, x - lw / 2.0, y + r + 16.0, &label, rgba(p.text, if sel { 0.9 } else { 0.5 }));
        }
        if n == 0 {
            self.t17.draw(&mut cv, ox + CANVAS.0 + 150.0, oy + CANVAS.1 + CANVAS.3 / 2.0, "waiting for seiza..", rgba(p.text, 0.3));
        }

        // saving the sky as a template
        let (name, focus) = (self.name.clone(), self.focus);
        self.field(&mut cv, TEMPLATE_NAME, ox, oy, &name, "name this layout", focus == Some(Field::Template));
        let ready = figure::name_ok(&self.name) && n > 0;
        self.button(&mut cv, SAVE, ox, oy, "save template", ready);
        self.button(&mut cv, DELETE, ox, oy, "delete it", false);

        // the stars as a list
        self.t17.draw(&mut cv, ox + 840.0, oy + 80.0, "stars", rgba(p.text, 0.7));
        let stars = self.sky.stars.clone();
        for (i, s) in stars.iter().take(STAR_ROWS).enumerate() {
            let r = star_row(i);
            if self.selected == Some(i) {
                cv.round_rect(ox + r.0, oy + r.1, r.2, r.3, 12.0, rgba(p.dust, 0.9));
            }
            if s.name == self.sky.current {
                cv.sparkle(ox + r.0 + 18.0, oy + r.1 + 16.0, 6.0, rgba(p.star, 1.0));
            }
            let shown = if s.name.len() > 20 { format!("{}..", &s.name[..18]) } else { s.name.clone() };
            self.t17.draw(&mut cv, ox + r.0 + 36.0, oy + r.1 + 22.0, &shown, rgba(p.text, 1.0));
            if s.windows > 0 {
                let t = format!("{} app{}", s.windows, if s.windows == 1 { "" } else { "s" });
                let tw = self.t13.width(&t);
                self.t13.draw(&mut cv, ox + r.0 + r.2 - 14.0 - tw, oy + r.1 + 21.0, &t, rgba(p.text, 0.5));
            }
        }
        if stars.len() > STAR_ROWS {
            self.t13.draw(&mut cv, ox + 840.0, oy + 100.0 + 34.0 * STAR_ROWS as f32 + 12.0, &format!("and {} more on the sky..", stars.len() - STAR_ROWS), rgba(p.text, 0.4));
        }
        self.button(&mut cv, REMOVE_STAR, ox, oy, "remove the selected star", false);
        let (sn, focus) = (self.star_name.clone(), self.focus);
        self.field(&mut cv, STAR_NAME, ox, oy, &sn, "rename this star", focus == Some(Field::Star));
        self.button(&mut cv, RENAME, ox, oy, "ok", false);

        self.t13.draw(&mut cv, ox + 250.0, oy + H - 30.0, &self.msg.clone(), rgba(p.text, 0.55));
        let ew = self.t13.width("esc to close");
        self.t13.draw(&mut cv, ox + W - 32.0 - ew, oy + H - 30.0, "esc to close", rgba(p.text, 0.35));
        cv
    }

    fn view_name(&self) -> String {
        if self.sky.stars.is_empty() { "the sky".into() } else { "your sky".into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f32, f32) = (1920.0, 1080.0);

    fn star(name: &str, x: f64, y: f64, windows: u64) -> Star {
        Star { name: name.into(), x, y, windows }
    }

    /// Press at a point given in panel coordinates.
    fn press(ui: &mut Ui, x: f32, y: f32, right: bool) -> Effect {
        let (ox, oy) = Ui::origin(SCREEN.0, SCREEN.1);
        ui.press(ox + x, oy + y, SCREEN.0, SCREEN.1, right)
    }

    fn drag(ui: &mut Ui, x: f32, y: f32) {
        let (ox, oy) = Ui::origin(SCREEN.0, SCREEN.1);
        ui.drag_to(ox + x, oy + y, SCREEN.0, SCREEN.1);
    }

    fn cmds(e: Effect) -> Vec<String> {
        match e {
            Effect::Run(c) => c,
            Effect::None => vec![],
        }
    }

    /// A sky of three stars in a row, and the panel position of each.
    fn sky3() -> (Ui, Vec<(f32, f32)>) {
        let mut ui = Ui::new();
        ui.set_sky(Sky { stars: vec![star("a", 960.0, 540.0, 0), star("b", 4800.0, 540.0, 2), star("c", 8640.0, 540.0, 0)], links: vec![(0, 1)], current: "a".into() });
        let at = (0..3).map(|i| ui.star_screen(i, 0.0, 0.0)).collect();
        (ui, at)
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
    fn clicking_the_dark_adds_a_star_there_and_clicking_along_chains_them() {
        at_config(|| {
            let (mut ui, _) = sky3();
            // an empty spot: a star is added, with nothing to join it to yet
            let c = cmds(press(&mut ui, 400.0, 400.0, false));
            assert_eq!(c.len(), 1);
            let nums: Vec<f64> = c[0].strip_prefix("seiza add-star ").unwrap().split(' ').map(|n| n.parse().unwrap()).collect();
            let want = ui.world(400.0, 400.0);
            assert!((nums[0] - want.0).abs() < 1.0 && (nums[1] - want.1).abs() < 1.0, "{nums:?} vs {want:?}");
            assert_eq!(ui.selected, Some(3), "the new star is selected");
            // the sky has not changed yet (Seiza has not answered): a second click along joins the new star to the selected one
            let mut sky = ui.sky.clone();
            sky.stars.push(star("star 3", nums[0], nums[1], 0));
            ui.set_sky(sky);
            let c = cmds(press(&mut ui, 500.0, 470.0, false));
            assert_eq!(c.len(), 2);
            assert!(c[0].starts_with("seiza add-star ") && c[1] == "seiza link 4 5", "{c:?}");
        });
    }

    #[test]
    fn a_star_cannot_be_added_on_top_of_another() {
        at_config(|| {
            let (mut ui, at) = sky3();
            // a click a little beside star c (on the canvas, but inside its territory, and not on the star itself)
            let c = cmds(press(&mut ui, at[2].0 + 22.0, at[2].1 + 8.0, false));
            assert!(c.is_empty(), "{c:?}");
            assert!(ui.msg.contains("too close"));
        });
    }

    #[test]
    fn clicking_two_stars_links_them_and_dragging_moves_one() {
        at_config(|| {
            let (mut ui, at) = sky3();
            press(&mut ui, at[0].0, at[0].1, false);
            assert!(cmds(ui.release()).is_empty() && ui.selected == Some(0), "a click selects");
            press(&mut ui, at[2].0, at[2].1, false);
            assert_eq!(cmds(ui.release()), ["seiza link 1 3"]);
            assert_eq!(ui.selected, Some(2));
            // dragging star 3 up and a little left (room enough): one move command, on release
            press(&mut ui, at[2].0, at[2].1, false);
            drag(&mut ui, at[2].0 - 10.0, at[2].1 - 40.0);
            let want = ui.drag.as_ref().unwrap().at;
            let c = cmds(ui.release());
            assert_eq!(c, [format!("seiza move-star 3 {:.0} {:.0}", want.0, want.1)]);
            // dropping it on star 1 is refused, with nothing sent
            press(&mut ui, at[2].0, at[2].1, false);
            drag(&mut ui, at[0].0 + 3.0, at[0].1 + 3.0);
            assert!(cmds(ui.release()).is_empty() && ui.msg.contains("no room"));
        });
    }

    #[test]
    fn removing_a_star_works_from_the_canvas_and_the_list_but_not_for_one_with_apps() {
        at_config(|| {
            let (mut ui, at) = sky3();
            assert_eq!(cmds(press(&mut ui, at[2].0, at[2].1, true)), ["seiza delete-star 3"], "right-click on the canvas");
            assert!(cmds(press(&mut ui, at[1].0, at[1].1, true)).is_empty() && ui.msg.contains("apps"), "star b has two apps");
            press(&mut ui, 900.0, 100.0 + 10.0, false); // star a, in the list
            assert_eq!(ui.selected, Some(0));
            assert_eq!(cmds(press(&mut ui, 1000.0, 440.0, false)), ["seiza delete-star 1"]);
            let (mut ui, _) = sky3();
            ui.set_sky(Sky { stars: vec![star("only", 960.0, 540.0, 0)], ..Default::default() });
            ui.selected = Some(0);
            assert!(cmds(ui.delete_selected()).is_empty() && ui.msg.contains("at least one"));
        });
    }

    #[test]
    fn a_template_needs_two_clicks_and_saving_stores_the_sky_as_one() {
        at_config(|| {
            let (mut ui, _) = sky3();
            assert!(cmds(press(&mut ui, 60.0, 100.0 + 32.0 * 1.0 + 10.0, false)).is_empty(), "the first click only picks it");
            assert_eq!(ui.name, "dipper");
            assert_eq!(cmds(press(&mut ui, 60.0, 100.0 + 32.0 * 1.0 + 10.0, false)), ["seiza figure dipper"]);
            // saving: name it, press save
            press(&mut ui, 300.0, 550.0, false);
            ui.name.clear();
            ui.type_text("my row!");
            press(&mut ui, 560.0, 550.0, false);
            let f = figure::load("myrow").expect("saved");
            assert_eq!((f.points.len(), f.links.clone()), (3, vec![(0, 1)]));
            assert!(ui.names.contains(&"myrow".to_string()));
            press(&mut ui, 700.0, 550.0, false); // delete it again
            assert!(figure::load("myrow").is_none());
        });
    }

    #[test]
    fn renaming_sends_the_selected_stars_number() {
        at_config(|| {
            let (mut ui, _) = sky3();
            press(&mut ui, 900.0, 100.0 + 34.0 + 10.0, false);
            assert_eq!(ui.star_name, "b");
            press(&mut ui, 900.0, 500.0, false);
            ui.star_name.clear();
            ui.type_text("north");
            assert_eq!(cmds(press(&mut ui, 1110.0, 500.0, false)), ["seiza rename-star 2 north"]);
        });
    }

    #[test]
    fn it_draws_empty_and_with_a_sky() {
        at_config(|| {
            let mut ui = Ui::new();
            let empty = ui.render(1920, 1080);
            let (mut ui, _) = sky3();
            assert_ne!(empty.px, ui.render(1920, 1080).px);
        });
    }
}
