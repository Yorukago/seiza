//! The window's state and drawing, with no Wayland in it so `--dump` and the tests can run it
//! anywhere. A searchable palette list on the left; on the right the selected palette's five
//! colors and where they land on Seiza.

use crate::{
    catalog::{self, Pal},
    place::{self, hex},
};
use seiza_common::{
    draw::{composite, shared_font, Canvas, Text},
    palette::{palette, rgba},
};

pub const PANEL_W: f32 = 880.0;
const LIST_W: f32 = 360.0;
const ROW_H: f32 = 52.0;
const HEAD_H: f32 = 76.0;
const FOOT_H: f32 = 44.0;
const MAX_ROWS: usize = 7;

pub struct Ui {
    pub query: String,
    pub sel: usize, // an index into `shown`
    pub scroll: usize,
    pub w: f32,
    pub h: f32,
    pub msg: Option<String>,
    pals: Vec<Pal>,
    shown: Vec<usize>, // the palettes matching the query
    t17: Text,
    t13: Text,
    content: Option<(String, Canvas)>,
}

fn fit(t: &mut Text, s: &str, max: f32) -> String {
    if t.width(s) <= max {
        return s.to_string();
    }
    let mut out = s.to_string();
    while !out.is_empty() && t.width(&format!("{out}..")) > max {
        out.pop();
    }
    format!("{out}..")
}

fn c4(c: [u8; 3], a: f32) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, a]
}

impl Ui {
    pub fn new(pals: Vec<Pal>, w: f32, h: f32) -> Ui {
        let mut ui = Ui { query: String::new(), sel: 0, scroll: 0, w, h, msg: None, pals, shown: vec![], t17: Text::new(shared_font(), 17.0), t13: Text::new(shared_font(), 13.0), content: None };
        ui.refresh();
        ui
    }

    pub fn current(&self) -> Option<&Pal> {
        self.shown.get(self.sel).map(|&i| &self.pals[i])
    }

    pub fn refresh(&mut self) {
        let q = self.query.trim().to_lowercase();
        self.shown = (0..self.pals.len()).filter(|&i| {
            let p = &self.pals[i];
            q.is_empty() || [&p.name, &p.mood, &p.collection].iter().any(|s| s.to_lowercase().contains(&q))
        }).collect();
        (self.sel, self.scroll) = (0, 0);
    }

    /// Re-read HueHaven's files (after saving a palette to them).
    pub fn reload(&mut self) {
        self.pals = catalog::load();
        self.refresh();
    }

    /// Flip the selected palette's favorite star in HueHaven's file.
    pub fn toggle_fav(&mut self) {
        let Some(&i) = self.shown.get(self.sel) else { return };
        self.msg = Some(match catalog::toggle_fav(&self.pals[i].name) {
            Ok(now) => {
                self.pals[i].fav = now;
                format!("{} {}", self.pals[i].name, if now { "is a favorite" } else { "is not a favorite any more" })
            }
            Err(e) => format!("couldn't change favorites: {e}"),
        });
    }

    pub fn select(&mut self, i: usize) {
        self.sel = i.min(self.shown.len().saturating_sub(1));
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + MAX_ROWS {
            self.scroll = self.sel + 1 - MAX_ROWS;
        }
    }

    pub fn step(&mut self, d: i32) {
        self.select((self.sel as i32 + d).max(0) as usize);
    }

    pub fn scroll_by(&mut self, d: i32) {
        let max = self.shown.len().saturating_sub(MAX_ROWS) as i32;
        self.scroll = (self.scroll as i32 + d).clamp(0, max) as usize;
    }

    fn panel(&self) -> (f32, f32, f32, f32) {
        ((self.w - PANEL_W) / 2.0, self.h * 0.2, PANEL_W, HEAD_H + MAX_ROWS as f32 * ROW_H + FOOT_H)
    }

    /// The `shown` index under a point, if a row is there.
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        let (px, py, _, _) = self.panel();
        let ry = y - py - HEAD_H;
        if x < px || x >= px + LIST_W || ry < 0.0 || ry >= MAX_ROWS as f32 * ROW_H {
            return None;
        }
        Some(self.scroll + (ry / ROW_H) as usize).filter(|&i| i < self.shown.len())
    }

    /// True when a click at this point is outside the panel.
    pub fn outside(&self, x: f32, y: f32) -> bool {
        let (px, py, pw, ph) = self.panel();
        x < px || x >= px + pw || y < py || y >= py + ph
    }

    fn put(&mut self, cv: &mut Canvas, x: f32, cy: f32, s: &str, big: bool, c: [f32; 4]) -> f32 {
        let (t, px) = if big { (&mut self.t17, 17.0) } else { (&mut self.t13, 13.0) };
        t.draw(cv, x, cy + px * 0.33, s, c)
    }

    /// One frame at visibility `e` (0 = gone, 1 = open); the content is drawn once per state change.
    pub fn render(&mut self, e: f32) -> Canvas {
        let p = palette();
        let key = format!("{}|{}|{}|{:?}|{}x{}", self.query, self.sel, self.scroll, self.msg, self.w, self.h);
        if self.content.as_ref().is_none_or(|(k, _)| *k != key) {
            let (w, h) = (self.w as usize, self.h as usize);
            let mut cv = Canvas { w, h, px: vec![0; w * h] };
            self.draw(&mut cv);
            self.content = Some((key, cv));
        }
        let content = &self.content.as_ref().unwrap().1;
        let mut out = Canvas { w: content.w, h: content.h, px: vec![0; content.w * content.h] };
        let a = 0.86 * e;
        let ch = |v: f32| (v * a * 255.0) as u32;
        out.px.fill(((a * 255.0) as u32) << 24 | ch(p.sky[0]) << 16 | ch(p.sky[1]) << 8 | ch(p.sky[2]));
        let (px, py, pw, ph) = self.panel();
        composite(&mut out, content, (px + pw / 2.0, py + ph / 2.0), 0.9 + 0.1 * e, 0.0, e, Some((px - 60.0, py - 60.0, pw + 120.0, ph + 120.0)));
        out
    }

    fn draw(&mut self, cv: &mut Canvas) {
        let p = palette();
        let (px, py, pw, ph) = self.panel();
        let (text, dim) = (rgba(p.text, 1.0), rgba(p.text, 0.55));
        cv.round_rect(px - 1.0, py - 1.0, pw + 2.0, ph + 2.0, 21.0, rgba(p.star, 0.35));
        cv.round_rect(px, py, pw, ph, 20.0, rgba(p.panel, 1.0));

        // search pill
        let cy = py + HEAD_H / 2.0;
        cv.round_rect(px + 14.0, cy - 19.0, 38.0, 38.0, 19.0, rgba(p.star, 0.35));
        cv.round_rect(px + 15.0, cy - 18.0, 36.0, 36.0, 18.0, rgba(p.panel, 1.0));
        cv.sparkle(px + 33.0, cy, 8.0, rgba(p.star, 1.0));
        cv.round_rect(px + 62.0, cy - 19.0, pw - 76.0, 38.0, 19.0, rgba(p.sky, 0.55));
        let shown_q = if self.query.is_empty() { "search palettes, moods, collections..".to_string() } else { self.query.clone() };
        let shown_q = fit(&mut self.t17, &shown_q, pw - 110.0);
        let end = self.put(cv, px + 78.0, cy, &shown_q, true, if self.query.is_empty() { rgba(p.text, 0.35) } else { text });
        cv.round_rect(if self.query.is_empty() { px + 76.0 } else { end + 1.0 }, cy - 9.0, 1.5, 18.0, 0.0, rgba(p.text, 0.9));
        cv.round_rect(px, py + HEAD_H - 1.0, pw, 1.0, 0.0, rgba(p.text, 0.08));

        // list
        if self.shown.is_empty() {
            self.put(cv, px + 24.0, py + HEAD_H + ROW_H / 2.0, "nothing by that name..", true, dim);
        } else {
            let hy = py + HEAD_H + (self.sel - self.scroll) as f32 * ROW_H;
            cv.round_rect(px + 6.0, hy + 3.0, LIST_W - 12.0, ROW_H - 6.0, 16.0, rgba(p.star, 0.4));
            cv.round_rect(px + 7.0, hy + 4.0, LIST_W - 14.0, ROW_H - 8.0, 15.0, rgba(p.dust, 0.9));
        }
        for r in 0..self.shown.len().saturating_sub(self.scroll).min(MAX_ROWS) {
            let pal = &self.pals[self.shown[self.scroll + r]];
            let cy = py + HEAD_H + r as f32 * ROW_H + ROW_H / 2.0;
            for (k, c) in pal.colors.iter().enumerate() {
                cv.round_rect(px + 18.0 + k as f32 * 13.0, cy - 8.0, 12.0, 16.0, 4.0, c4(*c, 1.0));
            }
            let name = format!("{}{}", if pal.fav { "★ " } else { "" }, pal.name);
            let (name, coll) = (fit(&mut self.t17, &name, LIST_W - 110.0), pal.collection.clone());
            self.put(cv, px + 96.0, cy - 9.0, &name, true, text);
            self.put(cv, px + 96.0, cy + 11.0, &coll, false, dim);
        }
        cv.round_rect(px + LIST_W, py + HEAD_H, 1.0, MAX_ROWS as f32 * ROW_H, 0.0, rgba(p.text, 0.08));

        // the selected palette
        if let Some(pal) = self.current() {
            let (name, mood, coll, colors) = (pal.name.clone(), pal.mood.clone(), pal.collection.clone(), pal.colors);
            let (x0, w0) = (px + LIST_W + 28.0, pw - LIST_W - 56.0);
            let y0 = py + HEAD_H + 28.0;
            let name = fit(&mut self.t17, &name, w0);
            self.put(cv, x0, y0, &name, true, text);
            let mood = fit(&mut self.t13, &mood, w0);
            self.put(cv, x0, y0 + 24.0, &mood, false, dim);
            let _ = coll;
            let sw = (w0 - 4.0 * 10.0) / 5.0;
            for (k, c) in colors.iter().enumerate() {
                let x = x0 + k as f32 * (sw + 10.0);
                cv.round_rect(x, y0 + 50.0, sw, 64.0, 14.0, c4(*c, 1.0));
                self.put(cv, x, y0 + 128.0, &hex(*c), false, dim);
            }
            self.put(cv, x0, y0 + 166.0, "on seiza", false, dim);
            let roles = place::place(&colors);
            for (k, (role, c)) in roles.list().into_iter().enumerate() {
                let x = x0 + k as f32 * (sw + 10.0);
                cv.round_rect(x, y0 + 184.0, sw, 40.0, 12.0, c4(c, 1.0));
                self.put(cv, x, y0 + 244.0, role, false, text);
            }
            let note = if roles.bright_sky() { "the sky here is bright; seiza looks best dark" } else { "stars stay as they are" };
            self.put(cv, x0, y0 + 276.0, note, false, rgba(p.text, 0.4));
        }

        // footer
        cv.round_rect(px, py + ph - FOOT_H, pw, 1.0, 0.0, rgba(p.text, 0.08));
        let fy = py + ph - FOOT_H / 2.0;
        let note = self.msg.clone().unwrap_or_else(|| format!("{} palettes", self.shown.len()));
        self.put(cv, px + 18.0, fy, &note, false, rgba(p.text, 0.8));
        let hint = "enter: apply  -  tab: save my sky  -  f2: favorite  -  esc";
        let hw = self.t13.width(hint);
        self.put(cv, px + pw - 18.0 - hw, fy, hint, false, rgba(p.text, 0.4));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal(name: &str, coll: &str) -> Pal {
        Pal { name: name.into(), mood: format!("{name} mood"), collection: coll.into(), colors: [[1, 2, 3]; 5], fav: false }
    }

    #[test]
    fn search_filters_names_moods_and_collections() {
        let mut ui = Ui::new(vec![pal("Matcha", "Cafe"), pal("Neon", "Retro"), pal("Plain", "Retro")], 1920.0, 1080.0);
        ui.query = "retro".into();
        ui.refresh();
        assert_eq!(ui.current().unwrap().name, "Neon");
        ui.step(5);
        assert_eq!(ui.current().unwrap().name, "Plain"); // clamped to the last match
        ui.query = "mood".into();
        ui.refresh();
        assert_eq!(ui.shown.len(), 3);
        ui.query = "zzz".into();
        ui.refresh();
        assert!(ui.current().is_none());
    }

    #[test]
    fn rows_are_hit_where_they_are_drawn() {
        let ui = Ui::new((0..20).map(|i| pal(&format!("p{i}"), "c")).collect(), 1920.0, 1080.0);
        let (px, py, _, _) = ui.panel();
        assert_eq!(ui.hit(px + 50.0, py + HEAD_H + ROW_H * 2.5), Some(2));
        assert_eq!(ui.hit(px + LIST_W + 10.0, py + HEAD_H + 10.0), None);
        assert!(ui.outside(0.0, 0.0));
    }
}
