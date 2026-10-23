//! The constellation easter egg: when stars are moved so that some of them line
//! up like a known figure, the figure's art appears. Matching is by position
//! only, ignoring which stars they are: the figure is upright, and may be moved
//! and uniformly scaled.
use std::f64::consts::PI;

pub struct Template {
    pub name: &'static str,
    /// Where the stars have to be, in figure units (the figure's radius is 1).
    pub points: Vec<(f64, f64)>,
    /// The art itself: polylines in figure units.
    pub figure: Vec<Vec<(f64, f64)>>,
}

/// A figure found among the stars: the star indices (one per template point,
/// in the template's order) and the transform `world = origin + p * scale`.
#[derive(Clone, Debug)]
pub struct Match {
    pub template: usize,
    pub stars: Vec<usize>,
    pub scale: f64,
    pub origin: (f64, f64),
}

/// How far a star may be from its place, as a fraction of the figure's radius.
pub const TOLERANCE: f64 = 0.22;
/// Smaller than this (world pixels) the stars would be sharing a territory.
pub const MIN_SCALE: f64 = 900.0;

fn arc(cx: f64, cy: f64, r: f64, from_deg: f64, to_deg: f64, n: usize) -> Vec<(f64, f64)> {
    (0..=n)
        .map(|i| {
            let a = (from_deg + (to_deg - from_deg) * i as f64 / n as f64) * PI / 180.0;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

pub fn templates() -> Vec<Template> {
    // A five-pointed star: stars on the tips, the art is its outline inside a ring.
    let tip = |i: usize, r: f64| {
        let a = (-90.0 + 36.0 * i as f64) * PI / 180.0;
        (r * a.cos(), r * a.sin())
    };
    let mut outline: Vec<(f64, f64)> = (0..10).map(|i| tip(i, if i % 2 == 0 { 1.0 } else { 0.4 })).collect();
    outline.push(outline[0]);
    let star = Template {
        name: "star",
        points: (0..5).map(|i| tip(2 * i, 1.0)).collect(),
        figure: vec![outline, arc(0.0, 0.0, 1.06, 0.0, 360.0, 64)],
    };

    // A crescent moon opening to the right: stars along its outer rim.
    let (icx, ir) = (0.5, 1.187);
    let tip_phi = (0.985f64).atan2(-0.174 - icx).to_degrees(); // inner circle's angle at the horns
    let mut rim = arc(0.0, 0.0, 1.0, 100.0, 260.0, 48);
    rim.extend(arc(icx, 0.0, ir, 360.0 - tip_phi, tip_phi, 48)); // back along the inner edge
    let moon = Template {
        name: "moon",
        points: (0..6)
            .map(|i| {
                let a = (100.0 + 32.0 * i as f64) * PI / 180.0;
                (a.cos(), a.sin())
            })
            .collect(),
        figure: vec![rim],
    };
    vec![star, moon]
}

/// The best figure formed by `stars` (world positions), if any.
pub fn find(stars: &[(f64, f64)]) -> Option<Match> {
    let mut best: Option<(f64, Match)> = None;
    for (ti, t) in templates().iter().enumerate() {
        if stars.len() < t.points.len() {
            continue;
        }
        // Anchor on two template points whose offsets differ on both axes.
        let (i0, i1) = (0, (1..t.points.len()).max_by(|&a, &b| {
            let m = |i: usize| (t.points[i].0 - t.points[0].0).abs().min((t.points[i].1 - t.points[0].1).abs());
            m(a).partial_cmp(&m(b)).unwrap()
        })?);
        let (dx, dy) = (t.points[i1].0 - t.points[i0].0, t.points[i1].1 - t.points[i0].1);
        if dx.abs() < 1e-6 || dy.abs() < 1e-6 {
            continue;
        }
        for (a, &(ax, ay)) in stars.iter().enumerate() {
            for (b, &(bx, by)) in stars.iter().enumerate() {
                if a == b {
                    continue;
                }
                let (sx, sy) = ((bx - ax) / dx, (by - ay) / dy);
                if sx <= 0.0 || sy <= 0.0 || (sx - sy).abs() / sx.max(sy) > 0.3 {
                    continue; // mirrored, or not uniformly scaled
                }
                let scale = (sx + sy) / 2.0;
                if scale < MIN_SCALE {
                    continue;
                }
                let origin = (ax - t.points[i0].0 * scale, ay - t.points[i0].1 * scale);
                let mut used = vec![];
                let mut err = 0.0;
                for p in &t.points {
                    let (ex, ey) = (origin.0 + p.0 * scale, origin.1 + p.1 * scale);
                    let near = stars
                        .iter()
                        .enumerate()
                        .filter(|(k, _)| !used.contains(k))
                        .map(|(k, s)| (k, ((s.0 - ex).powi(2) + (s.1 - ey).powi(2)).sqrt() / scale))
                        .min_by(|x, y| x.1.partial_cmp(&y.1).unwrap());
                    match near {
                        Some((k, d)) if d <= TOLERANCE => {
                            used.push(k);
                            err += d;
                        }
                        _ => break,
                    }
                }
                if used.len() == t.points.len() && best.as_ref().is_none_or(|(e, _)| err < *e) {
                    best = Some((err, Match { template: ti, stars: used, scale, origin }));
                }
            }
        }
    }
    best.map(|(_, m)| m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(t: &Template, origin: (f64, f64), scale: f64, jitter: f64) -> Vec<(f64, f64)> {
        t.points
            .iter()
            .enumerate()
            .map(|(i, p)| (origin.0 + p.0 * scale + jitter * scale * ((i % 3) as f64 - 1.0), origin.1 + p.1 * scale - jitter * scale * ((i % 2) as f64 - 0.5)))
            .collect()
    }

    #[test]
    fn finds_both_figures_anywhere_at_any_size() {
        let ts = templates();
        for (ti, t) in ts.iter().enumerate() {
            for (origin, scale) in [((960.0, 540.0), 1500.0), ((-4000.0, 9000.0), 2600.0)] {
                let m = find(&place(t, origin, scale, 0.0)).unwrap_or_else(|| panic!("{} not found", t.name));
                assert_eq!(m.template, ti);
                assert!((m.scale - scale).abs() < 1.0);
            }
        }
    }

    #[test]
    fn tolerates_sloppy_placement_and_extra_stars() {
        let t = &templates()[0];
        let mut stars = place(t, (0.0, 0.0), 2000.0, 0.1);
        stars.push((50_000.0, 50_000.0));
        stars.insert(0, (-30_000.0, 100.0));
        let m = find(&stars).expect("a loose pentagram is still a pentagram");
        assert_eq!(m.stars.len(), 5);
    }

    #[test]
    fn rejects_a_mess() {
        assert!(find(&[(0.0, 0.0), (2400.0, 0.0), (4800.0, 0.0), (0.0, 1500.0), (2400.0, 1500.0)]).is_none()); // the default grid
        let t = &templates()[0];
        assert!(find(&place(t, (0.0, 0.0), 2000.0, 0.5)).is_none()); // too sloppy
        assert!(find(&place(t, (0.0, 0.0), 300.0, 0.0)).is_none()); // too small to be real (territories would overlap)
    }

    #[test]
    fn figures_are_closed_and_inside_the_ring() {
        for t in templates() {
            for line in &t.figure {
                assert!(line.len() > 4);
                assert!(line.iter().all(|p| p.0.hypot(p.1) < 1.3), "{} strays", t.name);
            }
        }
    }
}
