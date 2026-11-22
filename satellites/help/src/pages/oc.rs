//! A stand-in: the real page replaces this whole file (see `pages/mod.rs` for the `Page` interface).

use super::{coming_soon, Fonts, Page};
use seiza_common::draw::Canvas;

pub struct Oc;

impl Oc {
    pub fn new() -> Oc {
        Oc
    }
}

impl Page for Oc {
    fn name(&self) -> &'static str {
        "yoru & seiza"
    }

    fn blurb(&self) -> String {
        "who we are.".into()
    }

    fn draw(&mut self, cv: &mut Canvas, f: &mut Fonts, _scroll: f32) -> f32 {
        coming_soon(cv, f, "yoru & seiza", "this page is on its way.")
    }
}
