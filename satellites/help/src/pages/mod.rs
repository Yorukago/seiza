//! The pages of help that draw themselves: everything but the keybinds, which `ui.rs` lays out itself.
//! A page is a `Page`; `extra_pages` lists them in the order of their stars.
//!
//! The body canvas a page draws on is the page's own: its origin is the top-left of the body and its size
//! (`cv.w` x `cv.h`) follows the card, which follows the screen, so lay out from `cv.w` on every draw.

use seiza_common::draw::{Canvas, Text};

pub mod elements;
pub mod oc;
pub mod voyager;

/// The three sizes of text a page may use. Never make a `Text` inside `draw`: parsing a font is slow.
#[allow(dead_code)] // a page uses the sizes it needs
pub struct Fonts<'a> {
    pub title: &'a mut Text, // 24 px
    pub body: &'a mut Text,  // 15 px
    pub small: &'a mut Text, // 13 px
}

pub trait Page {
    /// The label under the page's star.
    fn name(&self) -> &'static str;

    /// The line under the page's title.
    fn blurb(&self) -> String;

    /// Draw the body onto `cv`, scrolled up by `scroll` pixels. Returns the content's full height, so the
    /// caller can scroll it and draw the scrollbar.
    fn draw(&mut self, cv: &mut Canvas, f: &mut Fonts, scroll: f32) -> f32;

    /// (name, description) pairs the help search should find on this page.
    fn rows(&self) -> Vec<(String, String)> {
        vec![]
    }

    /// Called about twice a second while the page is showing. True if what it shows has changed and it must be
    /// drawn again (the distance to Voyager).
    fn tick(&mut self) -> bool {
        false
    }

    /// The pointer at (x, y) in CONTENT coordinates: x from the body's left edge, y from the top of the content
    /// (the scroll is already added, so it matches what `draw` drew). `press` is true for a click, false for a
    /// plain move. True if the page needs drawing again (a clicked element).
    fn pointer(&mut self, _x: f32, _y: f32, _press: bool) -> bool {
        false
    }
}

/// The pages after the keybinds, in the order of their stars.
pub fn extra_pages() -> Vec<Box<dyn Page>> {
    vec![Box::new(elements::Elements::new()), Box::new(voyager::Voyager::new()), Box::new(oc::Oc::new())]
}
