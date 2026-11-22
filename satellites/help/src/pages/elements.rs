//! The periodic table page: the 118 elements on the usual 18-column grid, the lanthanides and
//! actinides in two rows beneath, and a card for the one you click or point at.
//!
//! This part is the data and the layout maths, which need no drawing to test.
//! shortcut: masses are the standard atomic weights to the usual five figures; an element with no
//! stable isotope shows the mass number of its longest-lived isotope in brackets, as printed tables do.

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
}
