//! The periodic table page: the 118 elements on the usual 18-column grid, the lanthanides and
//! actinides in two rows beneath, and a card for the one you click or point at.
//!
//! This part is the data and the layout maths, which need no drawing to test.
//! shortcut: masses are the standard atomic weights to the usual five figures; an element with no
//! stable isotope shows the mass number of its longest-lived isotope in brackets, as printed tables do.

use super::{Fonts, Page};
use seiza_common::{
    draw::Canvas,
    kit,
    palette::{palette, rgba, Rgb},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    AlkaliMetal,
    AlkalineEarth,
    Transition,
    PostTransition,
    Metalloid,
    Nonmetal,
    NobleGas,
    Lanthanide,
    Actinide,
    Unknown, // 109 to 118: too few atoms ever made to tell
}

impl Cat {
    pub const ALL: [Cat; 10] = [Cat::AlkaliMetal, Cat::AlkalineEarth, Cat::Transition, Cat::PostTransition, Cat::Metalloid, Cat::Nonmetal, Cat::NobleGas, Cat::Lanthanide, Cat::Actinide, Cat::Unknown];

    pub fn label(self) -> &'static str {
        match self {
            Cat::AlkaliMetal => "alkali metal",
            Cat::AlkalineEarth => "alkaline earth",
            Cat::Transition => "transition metal",
            Cat::PostTransition => "post-transition metal",
            Cat::Metalloid => "metalloid",
            Cat::Nonmetal => "nonmetal",
            Cat::NobleGas => "noble gas",
            Cat::Lanthanide => "lanthanide",
            Cat::Actinide => "actinide",
            Cat::Unknown => "unknown properties",
        }
    }
}

/// (symbol, name, atomic weight) for atomic numbers 1 to 118, in order.
const TABLE: [(&str, &str, &str); 118] = [
    ("H", "Hydrogen", "1.008"), ("He", "Helium", "4.0026"), ("Li", "Lithium", "6.94"), ("Be", "Beryllium", "9.0122"),
    ("B", "Boron", "10.81"), ("C", "Carbon", "12.011"), ("N", "Nitrogen", "14.007"), ("O", "Oxygen", "15.999"),
    ("F", "Fluorine", "18.998"), ("Ne", "Neon", "20.180"), ("Na", "Sodium", "22.990"), ("Mg", "Magnesium", "24.305"),
    ("Al", "Aluminium", "26.982"), ("Si", "Silicon", "28.085"), ("P", "Phosphorus", "30.974"), ("S", "Sulfur", "32.06"),
    ("Cl", "Chlorine", "35.45"), ("Ar", "Argon", "39.948"), ("K", "Potassium", "39.098"), ("Ca", "Calcium", "40.078"),
    ("Sc", "Scandium", "44.956"), ("Ti", "Titanium", "47.867"), ("V", "Vanadium", "50.942"), ("Cr", "Chromium", "51.996"),
    ("Mn", "Manganese", "54.938"), ("Fe", "Iron", "55.845"), ("Co", "Cobalt", "58.933"), ("Ni", "Nickel", "58.693"),
    ("Cu", "Copper", "63.546"), ("Zn", "Zinc", "65.38"), ("Ga", "Gallium", "69.723"), ("Ge", "Germanium", "72.630"),
    ("As", "Arsenic", "74.922"), ("Se", "Selenium", "78.971"), ("Br", "Bromine", "79.904"), ("Kr", "Krypton", "83.798"),
    ("Rb", "Rubidium", "85.468"), ("Sr", "Strontium", "87.62"), ("Y", "Yttrium", "88.906"), ("Zr", "Zirconium", "91.224"),
    ("Nb", "Niobium", "92.906"), ("Mo", "Molybdenum", "95.95"), ("Tc", "Technetium", "[98]"), ("Ru", "Ruthenium", "101.07"),
    ("Rh", "Rhodium", "102.91"), ("Pd", "Palladium", "106.42"), ("Ag", "Silver", "107.87"), ("Cd", "Cadmium", "112.41"),
    ("In", "Indium", "114.82"), ("Sn", "Tin", "118.71"), ("Sb", "Antimony", "121.76"), ("Te", "Tellurium", "127.60"),
    ("I", "Iodine", "126.90"), ("Xe", "Xenon", "131.29"), ("Cs", "Caesium", "132.91"), ("Ba", "Barium", "137.33"),
    ("La", "Lanthanum", "138.91"), ("Ce", "Cerium", "140.12"), ("Pr", "Praseodymium", "140.91"), ("Nd", "Neodymium", "144.24"),
    ("Pm", "Promethium", "[145]"), ("Sm", "Samarium", "150.36"), ("Eu", "Europium", "151.96"), ("Gd", "Gadolinium", "157.25"),
    ("Tb", "Terbium", "158.93"), ("Dy", "Dysprosium", "162.50"), ("Ho", "Holmium", "164.93"), ("Er", "Erbium", "167.26"),
    ("Tm", "Thulium", "168.93"), ("Yb", "Ytterbium", "173.05"), ("Lu", "Lutetium", "174.97"), ("Hf", "Hafnium", "178.49"),
    ("Ta", "Tantalum", "180.95"), ("W", "Tungsten", "183.84"), ("Re", "Rhenium", "186.21"), ("Os", "Osmium", "190.23"),
    ("Ir", "Iridium", "192.22"), ("Pt", "Platinum", "195.08"), ("Au", "Gold", "196.97"), ("Hg", "Mercury", "200.59"),
    ("Tl", "Thallium", "204.38"), ("Pb", "Lead", "207.2"), ("Bi", "Bismuth", "208.98"), ("Po", "Polonium", "[209]"),
    ("At", "Astatine", "[210]"), ("Rn", "Radon", "[222]"), ("Fr", "Francium", "[223]"), ("Ra", "Radium", "[226]"),
    ("Ac", "Actinium", "[227]"), ("Th", "Thorium", "232.04"), ("Pa", "Protactinium", "231.04"), ("U", "Uranium", "238.03"),
    ("Np", "Neptunium", "[237]"), ("Pu", "Plutonium", "[244]"), ("Am", "Americium", "[243]"), ("Cm", "Curium", "[247]"),
    ("Bk", "Berkelium", "[247]"), ("Cf", "Californium", "[251]"), ("Es", "Einsteinium", "[252]"), ("Fm", "Fermium", "[257]"),
    ("Md", "Mendelevium", "[258]"), ("No", "Nobelium", "[259]"), ("Lr", "Lawrencium", "[266]"), ("Rf", "Rutherfordium", "[267]"),
    ("Db", "Dubnium", "[268]"), ("Sg", "Seaborgium", "[269]"), ("Bh", "Bohrium", "[270]"), ("Hs", "Hassium", "[277]"),
    ("Mt", "Meitnerium", "[278]"), ("Ds", "Darmstadtium", "[281]"), ("Rg", "Roentgenium", "[282]"), ("Cn", "Copernicium", "[285]"),
    ("Nh", "Nihonium", "[286]"), ("Fl", "Flerovium", "[289]"), ("Mc", "Moscovium", "[290]"), ("Lv", "Livermorium", "[293]"),
    ("Ts", "Tennessine", "[294]"), ("Og", "Oganesson", "[294]"),
];

pub const COUNT: u8 = 118;

#[derive(Clone, Copy, Debug)]
pub struct Element {
    pub n: u8,
    pub symbol: &'static str,
    pub name: &'static str,
    pub mass: &'static str,
}

/// The element with atomic number `n` (1 to 118).
pub fn element(n: u8) -> Option<Element> {
    let (symbol, name, mass) = *TABLE.get(usize::from(n).checked_sub(1)?)?;
    Some(Element { n, symbol, name, mass })
}

pub fn category(n: u8) -> Cat {
    match n {
        1 | 6..=9 | 15..=17 | 34 | 35 | 53 => Cat::Nonmetal,
        2 | 10 | 18 | 36 | 54 | 86 => Cat::NobleGas,
        3 | 11 | 19 | 37 | 55 | 87 => Cat::AlkaliMetal,
        4 | 12 | 20 | 38 | 56 | 88 => Cat::AlkalineEarth,
        5 | 14 | 32 | 33 | 51 | 52 | 85 => Cat::Metalloid,
        13 | 31 | 49 | 50 | 81..=84 => Cat::PostTransition,
        57..=71 => Cat::Lanthanide,
        89..=103 => Cat::Actinide,
        109..=118 => Cat::Unknown,
        _ => Cat::Transition, // 21 to 30, 39 to 48, 72 to 80, 104 to 108
    }
}

/// What it is at room temperature. Past fermium nobody has had enough of it to see.
pub fn state(n: u8) -> &'static str {
    match n {
        1 | 2 | 7 | 8 | 9 | 10 | 17 | 18 | 36 | 54 | 86 => "gas",
        35 | 80 => "liquid",
        100.. => "unknown",
        _ => "solid",
    }
}

/// (row, column) on the grid, both counted from 1: periods 1 to 7 in rows 1 to 7, the groups in columns 1 to 18,
/// the lanthanides in row 9 and the actinides in row 10 (row 8 is the gap).
pub fn position(n: u8) -> (u8, u8) {
    match n {
        1 => (1, 1),
        2 => (1, 18),
        3 | 4 => (2, n - 2),
        5..=10 => (2, n + 8),
        11 | 12 => (3, n - 10),
        13..=18 => (3, n),
        19..=36 => (4, n - 18),
        37..=54 => (5, n - 36),
        55 | 56 => (6, n - 54),
        57..=71 => (9, n - 54),
        72..=86 => (6, n - 68),
        87 | 88 => (7, n - 86),
        89..=103 => (10, n - 86),
        _ => (7, n - 100), // 104 to 118
    }
}

/// The group (1 to 18) of an element on the main grid; the lanthanides and actinides have none.
pub fn group(n: u8) -> Option<u8> {
    match position(n) {
        (r, c) if r <= 7 => Some(c),
        _ => None,
    }
}

pub fn period(n: u8) -> u8 {
    match position(n).0 {
        9 => 6,
        10 => 7,
        r => r,
    }
}

/// One line for the help search: "Fe, 26, transition metal, solid".
pub fn describe(n: u8) -> String {
    let e = element(n).unwrap();
    format!("{}, {}, {}, {}", e.symbol, e.n, category(n).label(), state(n))
}

/// Where the grid sits in the body: the cell size and the top-left corner.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Grid {
    pub cell: f32,
    pub x0: f32,
    pub y0: f32,
}

pub const GAP: f32 = 4.0;
const MAX_CELL: f32 = 72.0;

impl Grid {
    /// The biggest grid that fits `width`, centred, with `top` pixels above it.
    pub fn fit(width: f32, top: f32) -> Grid {
        let cell = ((width - 18.0 * GAP) / 18.0).clamp(24.0, MAX_CELL);
        let w = 18.0 * (cell + GAP) - GAP;
        Grid { cell, x0: ((width - w) / 2.0).max(0.0), y0: top }
    }

    /// Like `fit`, but also small enough that the whole table is `height` tall, down to a 30 px cell
    /// (below that the page scrolls instead).
    pub fn fit_in(width: f32, height: f32, top: f32) -> Grid {
        let by_width = Grid::fit(width, top);
        let by_height = ((height - top - 8.0 + GAP) / 9.5 - GAP).clamp(30.0, MAX_CELL); // the table is 9.5 steps tall
        let cell = by_width.cell.min(by_height);
        let w = 18.0 * (cell + GAP) - GAP;
        Grid { cell, x0: ((width - w) / 2.0).max(0.0), y0: top }
    }

    /// The rectangle (x, y, w, h) of grid cell (row, col), rows and columns from 1. Row 8 is half a cell: the gap.
    pub fn rect(&self, row: u8, col: u8) -> (f32, f32, f32, f32) {
        let step = self.cell + GAP;
        let rows_above = f32::from(row - 1) - if row > 8 { 0.5 } else { 0.0 };
        (self.x0 + f32::from(col - 1) * step, self.y0 + rows_above * step, self.cell, self.cell)
    }

    pub fn rect_of(&self, n: u8) -> (f32, f32, f32, f32) {
        let (r, c) = position(n);
        self.rect(r, c)
    }

    /// Height of the whole grid, to the bottom of the actinide row.
    pub fn height(&self) -> f32 {
        let (_, y, _, h) = self.rect(10, 1);
        y + h - self.y0
    }

    /// The element under a point.
    pub fn hit(&self, x: f32, y: f32) -> Option<u8> {
        (1..=COUNT).find(|&n| {
            let (rx, ry, rw, rh) = self.rect_of(n);
            x >= rx && x < rx + rw + GAP / 2.0 && y >= ry && y < ry + rh + GAP / 2.0
        })
    }

    /// The empty block in rows 1 to 3 between hydrogen and the boron column, big enough for a card:
    /// (x, y, w, h).
    pub fn card_area(&self) -> (f32, f32, f32, f32) {
        let (x, y, ..) = self.rect(1, 3);
        let (x2, _, w2, h3) = self.rect(3, 12);
        (x, y, x2 + w2 - x, self.rect(3, 12).1 + h3 - y)
    }
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// A category's colour, mixed from the palette's own roles so a new wallpaper recolours the table.
pub fn tint(c: Cat) -> Rgb {
    let p = palette();
    match c {
        Cat::AlkaliMetal => p.pink,
        Cat::AlkalineEarth => p.star,
        Cat::Transition => mix(p.glow, p.text, 0.2),
        Cat::PostTransition => mix(p.mist, p.text, 0.45),
        Cat::Metalloid => p.ice,
        Cat::Nonmetal => p.text,
        Cat::NobleGas => mix(p.glow, p.ice, 0.55),
        Cat::Lanthanide => mix(p.star, p.pink, 0.5),
        Cat::Actinide => mix(p.glow, p.pink, 0.55),
        Cat::Unknown => mix(p.text, p.sky, 0.6),
    }
}

/// Where the legend's chips go on a row `width` wide: (x, y, category) and the height they take.
fn legend_layout(small: &mut seiza_common::draw::Text, width: f32) -> (Vec<(f32, f32, Cat)>, f32) {
    let (mut x, mut y, mut chips) = (0.0, 0.0, vec![]);
    for c in Cat::ALL {
        let need = 16.0 + small.width(c.label()) + 22.0;
        if x > 0.0 && x + need > width {
            (x, y) = (0.0, y + 24.0);
        }
        chips.push((x, y, c));
        x += need;
    }
    (chips, y + 22.0)
}

pub struct Elements {
    grid: Grid,
    sel: Option<u8>,   // clicked: stays on show
    hover: Option<u8>, // pointed at: shows while the pointer is on it
}

impl Elements {
    pub fn new() -> Elements {
        Elements { grid: Grid::fit(900.0, 0.0), sel: None, hover: None }
    }

    /// The card for the element on show: its tile, its name and its numbers.
    fn card(&self, c: &mut Canvas, f: &mut Fonts, (x, y, w, h): (f32, f32, f32, f32)) {
        let p = palette();
        let short = h < 140.0; // a small window: no room for the tile and the facts, so two lines
        let Some(n) = self.hover.or(self.sel) else {
            kit::card(c, f.small, x, y, w, h, "elements");
            if short {
                f.body.draw(c, x + 18.0, y + kit::CARD_HEAD + 30.0, "point at one, click to keep it", rgba(p.text, 0.75));
            } else {
                f.body.draw(c, x + 18.0, y + kit::CARD_HEAD + 32.0, "point at an element to see it here,", rgba(p.text, 0.75));
                f.body.draw(c, x + 18.0, y + kit::CARD_HEAD + 56.0, "click one to keep it on show.", rgba(p.text, 0.75));
            }
            return;
        };
        let (e, cat) = (element(n).unwrap(), category(n));
        let t = tint(cat);
        kit::card(c, f.small, x, y, w, h, cat.label());
        if short {
            f.title.draw(c, x + 18.0, y + kit::CARD_HEAD + 30.0, &format!("{}  {}", e.symbol, e.name), rgba(p.text, 1.0));
            let line = format!("number {}  ·  weight {}  ·  {}", n, e.mass, state(n));
            f.small.draw(c, x + 18.0, y + kit::CARD_HEAD + 50.0, &line, rgba(p.text, 0.6));
            return;
        }
        let (tx, ty, ts) = (x + 18.0, y + kit::CARD_HEAD + 14.0, 84.0_f32.min(h - kit::CARD_HEAD - 28.0));
        c.round_rect(tx, ty, ts, ts, 12.0, rgba(t, 0.8));
        c.round_rect(tx + 1.0, ty + 1.0, ts - 2.0, ts - 2.0, 11.0, rgba(p.panel, 1.0));
        c.round_rect(tx + 1.0, ty + 1.0, ts - 2.0, ts - 2.0, 11.0, rgba(t, 0.2));
        f.small.draw(c, tx + 8.0, ty + 18.0, &n.to_string(), rgba(p.text, 0.6));
        let sw = f.title.width(e.symbol);
        f.title.draw(c, tx + (ts - sw) / 2.0, ty + ts * 0.66, e.symbol, rgba(p.text, 1.0));
        let nx = tx + ts + 22.0;
        f.title.draw(c, nx, ty + 22.0, e.name, rgba(p.text, 1.0));
        let group_text = group(n).map_or("f-block".to_string(), |g| g.to_string());
        let facts = [("atomic number", n.to_string()), ("atomic weight", e.mass.to_string()), ("period", period(n).to_string()), ("group", group_text), ("at room temperature", state(n).to_string())];
        let two = w >= 560.0 && h >= 150.0;
        let per_col = if two { 3 } else { 5 };
        let col_w = ((x + w - 18.0) - nx) / if two { 2.0 } else { 1.0 };
        for (i, (label, value)) in facts.iter().enumerate() {
            let (col, row) = (i / per_col, i % per_col);
            let (fx, fy) = (nx + col as f32 * col_w, ty + 50.0 + row as f32 * 22.0);
            if fy > y + h - 10.0 {
                continue; // a card too short for every fact shows the first ones
            }
            f.small.draw(c, fx, fy, label, rgba(p.text, 0.5));
            let (vx, vw) = (fx + f.small.width("at room temperature") + 12.0, f.body.width(value));
            f.body.draw(c, if two { vx.min(fx + col_w - vw - 8.0) } else { vx }, fy, value, rgba(p.text, 0.95));
        }
    }
}

impl Page for Elements {
    fn name(&self) -> &'static str {
        "elements"
    }

    fn blurb(&self) -> String {
        "all 118, by number. point at one, click to keep it on show.".into()
    }

    fn rows(&self) -> Vec<(String, String)> {
        (1..=COUNT).map(|n| {
            let e = element(n).unwrap();
            (e.name.to_string(), format!("{}, {}", describe(n), e.mass))
        }).collect()
    }

    fn pointer(&mut self, x: f32, y: f32, press: bool) -> bool {
        let hit = self.grid.hit(x, y);
        if press {
            if hit.is_none() {
                return false;
            }
            self.sel = if self.sel == hit { None } else { hit }; // a second click lets go
            true
        } else if self.hover != hit {
            self.hover = hit;
            true
        } else {
            false
        }
    }

    fn draw(&mut self, cv: &mut Canvas, f: &mut Fonts, scroll: f32) -> f32 {
        let (w, p) = (cv.w as f32, palette());
        let (chips, legend_h) = legend_layout(f.small, w);
        let top = legend_h + 14.0;
        self.grid = Grid::fit_in(w, cv.h as f32, top);
        let g = self.grid;
        let height = (top + g.height() + 8.0).ceil();
        let mut c = Canvas { w: cv.w, h: height as usize, px: vec![0; cv.w * height as usize] };

        for (x, y, cat) in chips {
            let t = tint(cat);
            c.round_rect(x, y + 5.0, 10.0, 10.0, 5.0, rgba(t, 0.95));
            f.small.draw(&mut c, x + 16.0, y + 15.0, cat.label(), rgba(p.text, 0.6));
        }

        for n in 1..=COUNT {
            let (x, y, s, _) = g.rect_of(n);
            let t = tint(category(n));
            let (sel, hot) = (self.sel == Some(n), self.hover == Some(n));
            c.round_rect(x, y, s, s, 8.0, rgba(t, if sel { 1.0 } else if hot { 0.9 } else { 0.55 }));
            c.round_rect(x + 1.0, y + 1.0, s - 2.0, s - 2.0, 7.0, rgba(p.panel, 1.0));
            c.round_rect(x + 1.0, y + 1.0, s - 2.0, s - 2.0, 7.0, rgba(t, if sel { 0.45 } else if hot { 0.34 } else { 0.2 }));
            let e = element(n).unwrap();
            if s >= 44.0 {
                f.small.draw(&mut c, x + 5.0, y + 14.0, &n.to_string(), rgba(p.text, 0.5));
            }
            let sw = f.body.width(e.symbol);
            f.body.draw(&mut c, x + (s - sw) / 2.0, y + s * 0.66 + 4.0, e.symbol, rgba(p.text, 1.0));
        }
        // the two cells that say where the lanthanides and actinides went
        for (row, text, cat) in [(6, "57-71", Cat::Lanthanide), (7, "89-103", Cat::Actinide)] {
            let (x, y, s, _) = g.rect(row, 3);
            c.round_rect(x, y, s, s, 8.0, rgba(tint(cat), 0.3));
            c.round_rect(x + 1.0, y + 1.0, s - 2.0, s - 2.0, 7.0, rgba(p.panel, 1.0));
            if s >= 44.0 {
                let tw = f.small.width(text);
                f.small.draw(&mut c, x + (s - tw) / 2.0, y + s / 2.0 + 5.0, text, rgba(p.text, 0.45));
            }
        }
        let (ax, ay, aw, ah) = g.card_area();
        if aw >= 300.0 && ah >= 96.0 {
            self.card(&mut c, f, (ax, ay, aw, ah));
        }

        cv.over(&c, 0, -(scroll as i32));
        height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_118_elements_in_order() {
        assert_eq!(TABLE.len(), 118);
        assert_eq!((element(1).unwrap().symbol, element(118).unwrap().symbol), ("H", "Og"));
        assert!(element(0).is_none() && element(119).is_none());
        for n in 1..=COUNT {
            let e = element(n).unwrap();
            assert!(!e.symbol.is_empty() && e.symbol.len() <= 2 && e.symbol.chars().next().unwrap().is_uppercase(), "{n}");
            assert!(e.mass.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '['), "{n} has a mass");
        }
    }

    #[test]
    fn well_known_elements_are_where_they_should_be() {
        let name = |s: &str| (1..=COUNT).find(|&n| element(n).unwrap().symbol == s).unwrap();
        assert_eq!(position(name("Fe")), (4, 8));
        assert_eq!(position(name("Hg")), (6, 12));
        assert_eq!(position(name("Og")), (7, 18));
        assert_eq!(position(name("He")), (1, 18));
        assert_eq!(position(name("B")), (2, 13));
        assert_eq!(position(name("Al")), (3, 13));
        assert_eq!(position(name("Lu")), (9, 17));
        assert_eq!(position(name("Lr")), (10, 17));
        assert_eq!((group(name("Fe")), period(name("Fe"))), (Some(8), 4));
        assert_eq!((group(name("La")), period(name("La"))), (None, 6));
        assert_eq!((group(name("Hf")), period(name("Hf"))), (Some(4), 6));
        assert_eq!((group(name("Ba")), group(name("Ra"))), (Some(2), Some(2)));
    }

    #[test]
    fn every_element_has_its_own_cell() {
        let mut seen = std::collections::HashSet::new();
        for n in 1..=COUNT {
            let (r, c) = position(n);
            assert!((1..=10).contains(&r) && r != 8 && (1..=18).contains(&c), "{n} at {r},{c}");
            assert!(seen.insert((r, c)), "{n} shares a cell");
        }
        // the main grid is 7 rows of up to 18, minus the f-block (2 x 15 placed beneath) and the two period-1 gaps
        assert_eq!(seen.len(), 118);
    }

    #[test]
    fn categories_add_up_and_agree_with_the_usual_counts() {
        let count = |c: Cat| (1..=COUNT).filter(|&n| category(n) == c).count();
        assert_eq!(Cat::ALL.iter().map(|&c| count(c)).sum::<usize>(), 118);
        assert_eq!((count(Cat::AlkaliMetal), count(Cat::AlkalineEarth), count(Cat::NobleGas)), (6, 6, 6));
        assert_eq!((count(Cat::Lanthanide), count(Cat::Actinide), count(Cat::Unknown)), (15, 15, 10));
        assert_eq!((count(Cat::Metalloid), count(Cat::Nonmetal)), (7, 11));
        assert_eq!(category(26), Cat::Transition);
        assert_eq!(category(82), Cat::PostTransition);
    }

    #[test]
    fn room_temperature_states() {
        let gases = (1..=COUNT).filter(|&n| state(n) == "gas").count();
        assert_eq!((gases, state(35), state(80), state(26), state(118)), (11, "liquid", "liquid", "solid", "unknown"));
    }

    #[test]
    fn the_grid_fits_its_width_and_finds_what_you_point_at() {
        for w in [600.0, 1000.0, 1400.0, 2400.0] {
            let g = Grid::fit(w, 10.0);
            let (x, _, cw, _) = g.rect(1, 18);
            assert!(x + cw <= w + 0.5 && g.x0 >= 0.0, "width {w}");
            for n in 1..=COUNT {
                let (rx, ry, rw, rh) = g.rect_of(n);
                assert_eq!(g.hit(rx + rw / 2.0, ry + rh / 2.0), Some(n), "centre of {n} at width {w}");
            }
        }
        let g = Grid::fit(1400.0, 10.0);
        assert_eq!(g.hit(2.0, 2.0), None);
        let (x, y, w, h) = g.card_area();
        assert!(g.hit(x + w / 2.0, y + h / 2.0).is_none(), "the card sits where there are no elements");
        let (rx, ry, ..) = g.rect(8, 1);
        assert_eq!(g.hit(rx + 5.0, ry + 5.0), None, "row 8 is the gap");
    }

    #[test]
    fn the_f_block_rows_sit_under_the_table() {
        let g = Grid::fit(1400.0, 0.0);
        let (_, lan_y, ..) = g.rect_of(57);
        let (_, p7_y, ..) = g.rect_of(87);
        assert!(lan_y > p7_y + g.cell, "lanthanides are below period 7");
        assert!((lan_y - p7_y - 1.5 * (g.cell + GAP)).abs() < 0.01, "with half a cell of gap");
        assert!(g.height() > 9.0 * g.cell);
    }

    #[test]
    fn describe_is_searchable() {
        assert_eq!(describe(26), "Fe, 26, transition metal, solid");
        assert_eq!(describe(35), "Br, 35, nonmetal, liquid");
    }

    fn fonts() -> (seiza_common::draw::Text, seiza_common::draw::Text, seiza_common::draw::Text) {
        let font = seiza_common::draw::shared_font();
        (seiza_common::draw::Text::new(font.clone(), 24.0), seiza_common::draw::Text::new(font.clone(), 15.0), seiza_common::draw::Text::new(font, 13.0))
    }

    fn draw(page: &mut Elements, w: usize, h: usize, scroll: f32) -> (Canvas, f32) {
        let (mut t, mut b, mut s) = fonts();
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let height = page.draw(&mut cv, &mut Fonts { title: &mut t, body: &mut b, small: &mut s }, scroll);
        (cv, height)
    }

    #[test]
    fn the_page_draws_at_every_size_and_scrolls_when_it_cannot_fit() {
        let mut page = Elements::new();
        for (w, h) in [(1420, 760), (900, 500), (640, 300), (2400, 1300)] {
            let (cv, height) = draw(&mut page, w, h, 0.0);
            assert!(height > 100.0, "{w}x{h}");
            assert!(cv.px.iter().any(|&p| p != 0), "something is drawn at {w}x{h}");
            if h >= 500 {
                assert!(height <= h as f32 + 1.0, "{w}x{h} fits without scrolling: {height}");
            }
        }
        let (_, tall) = draw(&mut page, 640, 300, 0.0);
        assert!(tall > 300.0, "a small body scrolls");
        let (_, _) = draw(&mut page, 640, 300, 120.0); // scrolled: no panic, blitted higher
    }

    #[test]
    fn pointing_hovers_and_clicking_pins_then_lets_go() {
        let mut page = Elements::new();
        draw(&mut page, 1420, 760, 0.0);
        let g = page.grid;
        let (x, y, s, _) = g.rect_of(26); // iron
        let (cx, cy) = (x + s / 2.0, y + s / 2.0);
        assert!(page.pointer(cx, cy, false), "hover changes");
        assert!(!page.pointer(cx, cy, false), "the same hover is not a change");
        assert_eq!(page.hover, Some(26));
        assert!(page.pointer(cx, cy, true));
        assert_eq!(page.sel, Some(26));
        assert!(page.pointer(cx, cy, true));
        assert_eq!(page.sel, None, "clicking it again lets go");
        assert!(!page.pointer(2.0, 2.0, true), "a click on nothing changes nothing");
        // with one on show the card draws, hovered or pinned, at a small size too
        page.pointer(cx, cy, true);
        for (w, h) in [(1420, 760), (900, 420)] {
            draw(&mut page, w, h, 0.0);
        }
    }

    #[test]
    fn the_search_finds_elements_by_name_symbol_number_and_kind() {
        let rows = Elements::new().rows();
        assert_eq!(rows.len(), 118);
        let find = |q: &str| rows.iter().filter(|(n, d)| format!("{n} {d}").to_lowercase().contains(q)).count();
        assert_eq!(find("iron"), 1);
        assert_eq!(find("noble gas"), 6);
        assert_eq!(find("liquid"), 2);
        assert!(find("55.845") >= 1, "by weight");
    }

    #[test]
    fn every_category_has_its_own_colour() {
        let colours: Vec<[u32; 3]> = Cat::ALL.iter().map(|&c| tint(c).map(|v| (v * 255.0) as u32)).collect();
        let distinct: std::collections::HashSet<_> = colours.iter().collect();
        assert_eq!(distinct.len(), 10, "{colours:?}");
    }

    #[test]
    fn a_short_body_makes_a_smaller_table_not_a_scrolling_one() {
        let (tall, short) = (Grid::fit_in(1400.0, 900.0, 40.0), Grid::fit_in(1400.0, 460.0, 40.0));
        assert!(short.cell < tall.cell);
        assert!(short.y0 + short.height() <= 460.0 + 1.0, "{}", short.y0 + short.height());
        assert_eq!(Grid::fit_in(1400.0, 100.0, 40.0).cell, 30.0, "never below 30 px");
    }
}
