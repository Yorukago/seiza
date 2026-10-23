//! The sky the editor mirrors: Seiza's stars and threads, the window that fits them onto
//! the canvas, and the rules that keep stars apart. No drawing and no Wayland here, so the
//! rules are plain tests.

use seiza_common::figure::Figure;

/// A star's territory, as the compositor sees it (STAR_W, STAR_H in seiza.c): two stars
/// may not be closer than this in both directions at once.
pub const TERRITORY: (f64, f64) = (1920.0, 1080.0);
/// Roughly the spacing the compositor lays figures out with (STAR_DX, STAR_DY).
const SPACING: (f64, f64) = (2400.0, 1500.0);

#[derive(Clone, Debug, PartialEq)]
pub struct Star {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub windows: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sky {
    pub stars: Vec<Star>,
    pub links: Vec<(usize, usize)>,
    pub current: String,
}

/// The part of the world the canvas shows, in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl View {
    /// A view of the whole sky with a star's room around the edge (and never smaller than a few
    /// stars across), grown on one axis so it has the canvas's shape and nothing is squashed.
    pub fn fit(stars: &[Star], canvas: (f64, f64)) -> View {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for s in stars {
            (x0, y0, x1, y1) = (x0.min(s.x), y0.min(s.y), x1.max(s.x), y1.max(s.y));
        }
        if stars.is_empty() {
            (x0, y0, x1, y1) = (960.0, 540.0, 960.0, 540.0);
        }
        let (mut w, mut h) = ((x1 - x0) + 2.0 * SPACING.0, (y1 - y0) + 2.0 * SPACING.1);
        (w, h) = (w.max(3.0 * SPACING.0), h.max(3.0 * SPACING.1));
        let aspect = canvas.0 / canvas.1;
        if w / h < aspect { w = h * aspect } else { h = w / aspect }
        View { x: (x0 + x1) / 2.0 - w / 2.0, y: (y0 + y1) / 2.0 - h / 2.0, w, h }
    }

    pub fn scale(&self, canvas: (f64, f64)) -> f64 {
        canvas.0 / self.w
    }

    pub fn to_canvas(&self, wx: f64, wy: f64, canvas: (f64, f64)) -> (f64, f64) {
        ((wx - self.x) * self.scale(canvas), (wy - self.y) * self.scale(canvas))
    }

    pub fn to_world(&self, cx: f64, cy: f64, canvas: (f64, f64)) -> (f64, f64) {
        (self.x + cx / self.scale(canvas), self.y + cy / self.scale(canvas))
    }

    pub fn holds(&self, wx: f64, wy: f64) -> bool {
        wx >= self.x && wx <= self.x + self.w && wy >= self.y && wy <= self.y + self.h
    }
}

impl Sky {
    /// Would a star at (x, y) share a territory with a star other than `except`?
    pub fn too_near(&self, except: Option<usize>, x: f64, y: f64) -> bool {
        self.stars.iter().enumerate().any(|(i, s)| Some(i) != except && (s.x - x).abs() < TERRITORY.0 && (s.y - y).abs() < TERRITORY.1)
    }

    /// The star nearest to a world point within `reach`, if any.
    pub fn star_near(&self, x: f64, y: f64, reach: f64) -> Option<usize> {
        self.stars
            .iter()
            .enumerate()
            .map(|(i, s)| (i, (s.x - x).hypot(s.y - y)))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The sky as a figure to save: star positions on a 0..1 scale (the compositor rescales
    /// figures to fit anyway), and the threads that join them.
    pub fn to_figure(&self) -> Figure {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for s in &self.stars {
            (x0, y0, x1, y1) = (x0.min(s.x), y0.min(s.y), x1.max(s.x), y1.max(s.y));
        }
        let span = (x1 - x0).max(y1 - y0).max(1.0);
        Figure {
            points: self.stars.iter().map(|s| (((s.x - x0) / span) as f32, ((s.y - y0) / span) as f32)).collect(),
            links: self.links.iter().copied().filter(|&(a, b)| a != b && a < self.stars.len() && b < self.stars.len()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn star(x: f64, y: f64) -> Star {
        Star { name: "s".into(), x, y, windows: 0 }
    }

    #[test]
    fn the_view_holds_every_star_and_has_the_canvas_shape() {
        let canvas = (560.0, 410.0);
        for stars in [vec![], vec![star(960.0, 540.0)], vec![star(960.0, 540.0), star(30000.0, 2000.0)], vec![star(0.0, 0.0), star(0.0, 40000.0)]] {
            let v = View::fit(&stars, canvas);
            assert!(stars.iter().all(|s| v.holds(s.x, s.y)));
            assert!((v.w / v.h - canvas.0 / canvas.1).abs() < 1e-9);
            assert!(v.w >= 3.0 * SPACING.0 - 1e-9);
        }
    }

    #[test]
    fn canvas_and_world_map_back_and_forth() {
        let (canvas, v) = ((560.0, 410.0), View::fit(&[star(960.0, 540.0), star(9000.0, 5000.0)], (560.0, 410.0)));
        let (cx, cy) = v.to_canvas(4000.0, 3000.0, canvas);
        let (wx, wy) = v.to_world(cx, cy, canvas);
        assert!((wx - 4000.0).abs() < 1e-6 && (wy - 3000.0).abs() < 1e-6);
        assert!(cx > 0.0 && cx < canvas.0 && cy > 0.0 && cy < canvas.1);
    }

    #[test]
    fn territories_may_not_overlap_unless_it_is_the_star_itself() {
        let sky = Sky { stars: vec![star(960.0, 540.0), star(5000.0, 540.0)], ..Default::default() };
        assert!(sky.too_near(None, 1500.0, 900.0), "inside the first star's territory");
        assert!(!sky.too_near(None, 960.0 + 1920.0, 540.0), "exactly a territory away is room enough");
        assert!(!sky.too_near(None, 960.0, 540.0 + 1080.0));
        assert!(!sky.too_near(Some(0), 1100.0, 600.0), "a star moving a little does not collide with itself");
        assert!(sky.too_near(Some(1), 1100.0, 600.0), "but it does with the other one");
    }

    #[test]
    fn finds_the_star_under_a_point() {
        let sky = Sky { stars: vec![star(0.0, 0.0), star(100.0, 0.0)], ..Default::default() };
        assert_eq!(sky.star_near(60.0, 0.0, 50.0), Some(1));
        assert_eq!(sky.star_near(500.0, 0.0, 50.0), None);
    }

    #[test]
    fn a_sky_becomes_a_figure_that_loads() {
        let sky = Sky { stars: vec![star(960.0, 540.0), star(3360.0, 540.0), star(3360.0, 2040.0)], links: vec![(0, 1), (1, 2), (2, 2), (0, 9)], current: String::new() };
        let f = sky.to_figure();
        assert_eq!(f.points.len(), 3);
        assert_eq!(f.links, [(0, 1), (1, 2)], "threads to nothing, or to themselves, are left out");
        assert!(f.points.iter().all(|&(u, v)| (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)));
        assert_eq!(Figure::parse(&f.format()), Some(f));
        assert_eq!(Sky::default().to_figure().points.len(), 0);
    }
}
