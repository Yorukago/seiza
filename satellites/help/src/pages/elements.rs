//! A stand-in: the real page replaces this whole file (see `pages/mod.rs` for the `Page` interface).

use super::{coming_soon, Fonts, Page};
use seiza_common::draw::Canvas;

pub struct Elements;

impl Elements {
    pub fn new() -> Elements {
        Elements
    }
}

impl Page for Elements {
    fn name(&self) -> &'static str {
        "elements"
    }

    fn blurb(&self) -> String {
        "every element, by number.".into()
    }

    fn draw(&mut self, cv: &mut Canvas, f: &mut Fonts, _scroll: f32) -> f32 {
        coming_soon(cv, f, "elements", "the element table is on its way.")
    }
}
