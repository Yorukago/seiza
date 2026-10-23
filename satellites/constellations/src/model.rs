//! The figure being drawn: points, threads, and which point is selected. No drawing
//! or Wayland here, so the rules of drawing a constellation are plain tests.

use seiza_common::figure::Figure;

/// The compositor's limits (FIG_POINTS_MAX, FIG_LINKS_MAX in figure.h); a figure over them would not load.
pub const MAX_POINTS: usize = 24;
pub const MAX_LINKS: usize = 40;

#[derive(Default)]
pub struct Editor {
    pub fig: Figure,
    pub selected: Option<usize>,
}

impl Editor {
    pub fn from_figure(fig: Figure) -> Editor {
        Editor { fig, selected: None }
    }

    /// Shrinks or grows the figure to sit centered in a `w` by `h` box (in figure units),
    /// keeping its shape. The compositor rescales every figure to fit its stars anyway,
    /// so only the shape matters.
    pub fn fit(&mut self, w: f32, h: f32) {
        let pts = &mut self.fig.points;
        if pts.is_empty() {
            return;
        }
        let (u0, u1) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
        let (v0, v1) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
        let (bw, bh) = (u1 - u0, v1 - v0);
        let s = match (bw > 1e-6, bh > 1e-6) {
            (true, true) => (w / bw).min(h / bh),
            (true, false) => w / bw,
            (false, true) => h / bh,
            (false, false) => 1.0,
        };
        for p in pts.iter_mut() {
            *p = ((p.0 - u0) * s + (w - bw * s) / 2.0, (p.1 - v0) * s + (h - bh * s) / 2.0);
        }
    }

    fn link_at(&self, a: usize, b: usize) -> Option<usize> {
        self.fig.links.iter().position(|&(x, y)| (x, y) == (a, b) || (x, y) == (b, a))
    }

    /// A new point; if one is selected, the new one is threaded to it and selected in its place,
    /// so clicking along draws a chain. None when the figure is full.
    pub fn add_point(&mut self, u: f32, v: f32) -> Option<usize> {
        if self.fig.points.len() >= MAX_POINTS {
            return None;
        }
        self.fig.points.push((u, v));
        let i = self.fig.points.len() - 1;
        if let Some(s) = self.selected {
            if self.fig.links.len() < MAX_LINKS {
                self.fig.links.push((s, i));
            }
        }
        self.selected = Some(i);
        Some(i)
    }

    pub fn move_point(&mut self, i: usize, u: f32, v: f32) {
        if let Some(p) = self.fig.points.get_mut(i) {
            *p = (u, v);
        }
    }

    /// A plain click on a point: select it; with another point already selected, join them
    /// (or cut the thread that was there) and move the selection on; on the selected point, let go of it.
    pub fn click_point(&mut self, i: usize) {
        match self.selected {
            Some(s) if s == i => self.selected = None,
            Some(s) => {
                match self.link_at(s, i) {
                    Some(l) => {
                        self.fig.links.remove(l);
                    }
                    None if self.fig.links.len() < MAX_LINKS => self.fig.links.push((s, i)),
                    None => {}
                }
                self.selected = Some(i);
            }
            None => self.selected = Some(i),
        }
    }

    /// Takes the point and its threads away; the points after it move up one.
    pub fn remove_point(&mut self, i: usize) {
        if i >= self.fig.points.len() {
            return;
        }
        self.fig.points.remove(i);
        let shift = |p: usize| if p > i { p - 1 } else { p };
        self.fig.links = self.fig.links.iter().filter(|&&(a, b)| a != i && b != i).map(|&(a, b)| (shift(a), shift(b))).collect();
        self.selected = match self.selected {
            Some(s) if s == i => None,
            Some(s) => Some(shift(s)),
            None => None,
        };
    }

    /// The point nearest to (u, v) within `reach`, if any.
    pub fn point_near(&self, u: f32, v: f32, reach: f32) -> Option<usize> {
        self.fig
            .points
            .iter()
            .enumerate()
            .map(|(i, &(pu, pv))| (i, (pu - u).hypot(pv - v)))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicking_along_draws_a_chain() {
        let mut e = Editor::default();
        e.add_point(0.1, 0.1);
        e.add_point(0.3, 0.2);
        e.add_point(0.5, 0.1);
        assert_eq!(e.fig.links, [(0, 1), (1, 2)]);
        assert_eq!(e.selected, Some(2));
    }

    #[test]
    fn clicking_two_points_joins_and_cuts() {
        let mut e = Editor::default();
        for p in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5)] {
            e.selected = None;
            e.add_point(p.0, p.1);
        }
        assert!(e.fig.links.is_empty());
        e.selected = None; // adding a point selects it
        e.click_point(0);
        e.click_point(2);
        assert_eq!((e.fig.links.clone(), e.selected), (vec![(0, 2)], Some(2)));
        e.click_point(0); // 2 is selected: join 2-0 again means cut it
        assert!(e.fig.links.is_empty());
        e.click_point(0); // 0 is selected now: clicking it lets go
        assert_eq!(e.selected, None);
    }

    #[test]
    fn removing_a_point_renumbers_the_threads() {
        let mut e = Editor::default();
        for i in 0..4 {
            e.add_point(i as f32 * 0.2, 0.0);
        }
        assert_eq!(e.fig.links, [(0, 1), (1, 2), (2, 3)]);
        e.remove_point(1);
        assert_eq!(e.fig.points.len(), 3);
        assert_eq!(e.fig.links, [(1, 2)]); // 0-1 and 1-2 went with the point; 2-3 became 1-2
        assert_eq!(e.selected, Some(2)); // the selected point (3) moved up to 2
        e.remove_point(9); // not there: nothing happens
        assert_eq!(e.fig.points.len(), 3);
    }

    #[test]
    fn a_figure_stops_at_what_the_compositor_can_load() {
        let mut e = Editor::default();
        for i in 0..MAX_POINTS + 5 {
            e.add_point((i % 6) as f32 * 0.1, (i / 6) as f32 * 0.1);
        }
        assert_eq!(e.fig.points.len(), MAX_POINTS);
        assert!(e.fig.links.len() <= MAX_LINKS);
        assert!(Figure::parse(&e.fig.format()).is_some(), "what the editor makes must load");
    }

    #[test]
    fn fitting_keeps_the_shape_and_lands_inside_the_box() {
        let mut e = Editor::from_figure(Figure { points: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 2.0)], links: vec![(0, 1), (1, 2)] });
        e.fit(0.8, 0.5);
        let p = &e.fig.points;
        assert!(p.iter().all(|&(u, v)| (0.0..=0.8001).contains(&u) && (0.0..=0.5001).contains(&v)));
        let ratio = (p[2].1 - p[1].1) / (p[1].0 - p[0].0);
        assert!((ratio - 2.0).abs() < 1e-4, "a figure twice as tall as wide stays so: {ratio}");
        assert_eq!(e.fig.links, [(0, 1), (1, 2)]);
        Editor::default().fit(1.0, 1.0); // nothing to fit: nothing happens
        let mut one = Editor::from_figure(Figure { points: vec![(5.0, 5.0)], links: vec![] });
        one.fit(0.8, 0.5);
        assert_eq!(one.fig.points[0], (0.4, 0.25));
    }

    #[test]
    fn finds_the_nearest_point_within_reach() {
        let mut e = Editor::default();
        e.add_point(0.2, 0.2);
        e.add_point(0.25, 0.2);
        assert_eq!(e.point_near(0.24, 0.2, 0.05), Some(1));
        assert_eq!(e.point_near(0.9, 0.9, 0.05), None);
    }
}
