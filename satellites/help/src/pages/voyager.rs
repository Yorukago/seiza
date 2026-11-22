//! A stand-in: the real page replaces this whole file (see `pages/mod.rs` for the `Page` interface).

use super::{coming_soon, Fonts, Page};
use seiza_common::draw::Canvas;

pub struct Voyager;

impl Voyager {
    pub fn new() -> Voyager {
        Voyager
    }
}

impl Page for Voyager {
    fn name(&self) -> &'static str {
        "voyager"
    }

    fn blurb(&self) -> String {
        "where Voyager 1 is right now.".into()
    }

    fn draw(&mut self, cv: &mut Canvas, f: &mut Fonts, _scroll: f32) -> f32 {
        coming_soon(cv, f, "voyager", "Voyager 1 is on its way.")
    }
}
