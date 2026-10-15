use gpui::{Bounds, PathBuilder, Pixels, Point, point, px};

pub(crate) fn segment_may_intersect(b: Bounds<Pixels>, p: Point<Pixels>, q: Point<Pixels>) -> bool {
    let (x0, x1) = (p.x.min(q.x), p.x.max(q.x));
    let (y0, y1) = (p.y.min(q.y), p.y.max(q.y));
    !(x1 < b.left() || x0 > b.right() || y1 < b.top() || y0 > b.bottom())
}

pub(crate) fn draw_self_loop(path: &mut PathBuilder, c: Point<Pixels>, r: f32) {
    let s = r.max(6.);
    let start = point(c.x + px(r * 0.6), c.y - px(r * 0.8));
    let end = point(c.x + px(r * 0.8), c.y - px(r * 0.6));
    path.move_to(start);
    path.cubic_bezier_to(
        end,
        point(c.x + px(s * 1.2), c.y - px(s * 3.)),
        point(c.x + px(s * 3.), c.y - px(s * 1.2)),
    );
}

/// A straight or quadratic edge between two screen points.
pub(crate) struct EdgeGeometry {
    a: Point<Pixels>,
    b: Point<Pixels>,
    pub ctrl: Option<Point<Pixels>>,
}

impl EdgeGeometry {
    /// `bend` is the perpendicular offset of the curve's midpoint, in pixels.
    pub fn new(a: Point<Pixels>, b: Point<Pixels>, bend: f32) -> Self {
        let ctrl = (bend.abs() > 0.01).then(|| {
            let (dx, dy) = ((b.x - a.x).as_f32(), (b.y - a.y).as_f32());
            let len = dx.hypot(dy).max(1.);
            let (nx, ny) = (-dy / len, dx / len);
            // A quadratic's midpoint sits halfway to its control point.
            point(
                px((a.x + b.x).as_f32() / 2. + nx * bend * 2.),
                px((a.y + b.y).as_f32() / 2. + ny * bend * 2.),
            )
        });
        Self { a, b, ctrl }
    }

    pub fn eval(&self, t: f32) -> Point<Pixels> {
        match self.ctrl {
            None => self.a + (self.b - self.a) * t,
            Some(c) => {
                let u = 1. - t;
                let f = |a: Pixels, c: Pixels, b: Pixels| {
                    px(u * u * a.as_f32() + 2. * u * t * c.as_f32() + t * t * b.as_f32())
                };
                point(f(self.a.x, c.x, self.b.x), f(self.a.y, c.y, self.b.y))
            }
        }
    }

    fn approx_len(&self) -> f32 {
        match self.ctrl {
            None => dist(self.a, self.b),
            Some(c) => (dist(self.a, c) + dist(c, self.b) + dist(self.a, self.b)) / 2.,
        }
    }

    pub fn point_from_start(&self, d: f32) -> Point<Pixels> {
        self.eval((d / self.approx_len().max(1.)).min(0.5))
    }

    pub fn point_from_end(&self, d: f32) -> Point<Pixels> {
        self.eval(1. - (d / self.approx_len().max(1.)).min(0.5))
    }

    pub fn midpoint(&self) -> Point<Pixels> {
        self.eval(0.5)
    }

    /// Unit tangent at the target end.
    pub fn end_direction(&self) -> (f32, f32) {
        let from = self.ctrl.unwrap_or(self.a);
        let (dx, dy) = ((self.b.x - from.x).as_f32(), (self.b.y - from.y).as_f32());
        let len = dx.hypot(dy).max(1e-3);
        (dx / len, dy / len)
    }

    /// Approximate distance from `p` to the curve, sampled.
    pub fn distance_to(&self, p: Point<Pixels>) -> f32 {
        const STEPS: usize = 12;
        let mut prev = self.a;
        let mut best = f32::MAX;
        for k in 1..=STEPS {
            let next = self.eval(k as f32 / STEPS as f32);
            best = best.min(point_segment_distance(p, prev, next));
            prev = next;
        }
        best
    }
}

/// Convex hull (Andrew's monotone chain), counter-clockwise, without repeats.
pub(crate) fn convex_hull(mut pts: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: (f32, f32), a: (f32, f32), b: (f32, f32)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut hull: Vec<(f32, f32)> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &(f32, f32)>> =
            if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0. {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Hull around a set of circles, inflated by `padding`.
pub(crate) fn circles_hull(circles: &[(Point<Pixels>, f32)], padding: f32) -> Vec<(f32, f32)> {
    const SAMPLES: usize = 12;
    let mut pts = Vec::with_capacity(circles.len() * SAMPLES);
    for &(c, r) in circles {
        let r = r + padding;
        for k in 0..SAMPLES {
            let a = k as f32 / SAMPLES as f32 * std::f32::consts::TAU;
            pts.push((c.x.as_f32() + r * a.cos(), c.y.as_f32() + r * a.sin()));
        }
    }
    convex_hull(pts)
}

/// A closed path through the midpoints of `pts`, using each vertex as a
/// control point, which rounds every corner of the polygon.
pub(crate) fn smooth_closed(path: &mut PathBuilder, pts: &[(f32, f32)]) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let mid = |a: (f32, f32), b: (f32, f32)| point(px((a.0 + b.0) / 2.), px((a.1 + b.1) / 2.));
    path.move_to(mid(pts[n - 1], pts[0]));
    for i in 0..n {
        let next = pts[(i + 1) % n];
        path.curve_to(mid(pts[i], next), point(px(pts[i].0), px(pts[i].1)));
    }
    path.close();
}

fn dist(p: Point<Pixels>, q: Point<Pixels>) -> f32 {
    (q.x - p.x).as_f32().hypot((q.y - p.y).as_f32())
}

fn point_segment_distance(p: Point<Pixels>, a: Point<Pixels>, b: Point<Pixels>) -> f32 {
    let (abx, aby) = ((b.x - a.x).as_f32(), (b.y - a.y).as_f32());
    let (apx, apy) = ((p.x - a.x).as_f32(), (p.y - a.y).as_f32());
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0. { ((apx * abx + apy * aby) / len2).clamp(0., 1.) } else { 0. };
    (apx - abx * t).hypot(apy - aby * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_drops_interior_points() {
        let pts = vec![(0., 0.), (10., 0.), (10., 10.), (0., 10.), (5., 5.), (2., 7.)];
        let hull = convex_hull(pts);
        assert_eq!(hull.len(), 4);
        assert!(!hull.contains(&(5., 5.)));
    }

    #[test]
    fn curve_distance_hits_midpoint() {
        let g = EdgeGeometry::new(point(px(0.), px(0.)), point(px(100.), px(0.)), 20.);
        let mid = g.midpoint();
        assert!((mid.y.as_f32().abs() - 20.).abs() < 0.5);
        assert!(g.distance_to(mid) < 0.5);
        assert!(g.distance_to(point(px(50.), px(0.))) > 15.);
    }
}
