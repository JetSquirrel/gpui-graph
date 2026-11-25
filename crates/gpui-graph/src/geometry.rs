use gpui::{
    Bounds, FillOptions, FillRule, Path, PathBuilder, PathStyle, Pixels, Point, point, px,
};

pub(crate) fn segment_may_intersect(b: Bounds<Pixels>, p: Point<Pixels>, q: Point<Pixels>) -> bool {
    let (x0, x1) = (p.x.min(q.x), p.x.max(q.x));
    let (y0, y1) = (p.y.min(q.y), p.y.max(q.y));
    !(x1 < b.left() || x0 > b.right() || y1 < b.top() || y0 > b.bottom())
}

/// Edges and arrowheads as triangles, gathered into one fill path per colour
/// so thousands of them paint in a single draw. Every triangle is wound the
/// same way and filled by the non-zero rule, so where strokes and arrowheads
/// overlap they add up instead of cancelling into holes.
pub(crate) struct TriangleBatch {
    path: PathBuilder,
    contours: usize,
    width: f32,
    #[cfg(test)]
    points: Vec<Point<Pixels>>,
}

impl TriangleBatch {
    pub fn new(width: f32) -> Self {
        Self {
            path: PathBuilder::fill().with_style(PathStyle::Fill(FillOptions {
                fill_rule: FillRule::NonZero,
            })),
            contours: 0,
            width,
            #[cfg(test)]
            points: Vec::new(),
        }
    }

    /// Adds a simple polygon as one contour, wound positively.
    fn polygon(&mut self, points: &mut [Point<Pixels>]) {
        let n = points.len();
        let area: f32 = (0..n)
            .map(|i| {
                let (p, q) = (points[i], points[(i + 1) % n]);
                p.x.as_f32() * q.y.as_f32() - q.x.as_f32() * p.y.as_f32()
            })
            .sum();
        if !area.is_finite() || area.abs() < 1e-6 {
            return;
        }
        if area < 0. {
            points.reverse();
        }
        self.path.add_polygon(points, true);
        self.contours += 1;
        #[cfg(test)]
        self.points.extend_from_slice(points);
    }

    pub fn triangle(&mut self, a: Point<Pixels>, b: Point<Pixels>, c: Point<Pixels>) {
        self.polygon(&mut [a, b, c]);
    }

    /// A straight stroke from `a` to `b`, as a quad.
    pub fn line(&mut self, a: Point<Pixels>, b: Point<Pixels>) {
        self.polyline([a, b]);
    }

    /// A stroke through `points`, as one ribbon-shaped contour: GPUI bins
    /// every curve of a path on the CPU, and a ribbon has two long sides
    /// where a chain of quads, or of triangle pairs, has many.
    pub fn polyline(&mut self, points: impl IntoIterator<Item = Point<Pixels>>) {
        // A bend sharper than this gets a clipped join, so its offset stays
        // within twice the half width.
        const MITER_LIMIT: f32 = 2.;
        let mut centre: Vec<(f32, f32)> = Vec::new();
        for p in points {
            let p = (p.x.as_f32(), p.y.as_f32());
            if centre
                .last()
                .is_none_or(|q: &(f32, f32)| (p.0 - q.0).hypot(p.1 - q.1) >= 1e-3)
            {
                centre.push(p);
            }
        }
        let n = centre.len();
        if n < 2 {
            return;
        }
        let half = self.width / 2.;
        let normal = |i: usize| {
            let (a, b) = (centre[i], centre[i + 1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = dx.hypot(dy);
            (-dy / len, dx / len)
        };
        let mut outline = vec![point(px(0.), px(0.)); 2 * n];
        for i in 0..n {
            let (nx, ny) = match (i.checked_sub(1).map(normal), (i + 1 < n).then(|| normal(i))) {
                (Some(m0), Some(m1)) => {
                    let (sx, sy) = (m0.0 + m1.0, m0.1 + m1.1);
                    // Miter: the summed normal's length is 2·cos(θ/2).
                    let cos = (sx.hypot(sy) / 2.).max(1. / MITER_LIMIT);
                    let len = sx.hypot(sy).max(1e-6);
                    (sx / len / cos, sy / len / cos)
                }
                (Some(m), None) | (None, Some(m)) => m,
                (None, None) => unreachable!("n >= 2"),
            };
            let (x, y) = centre[i];
            outline[i] = point(px(x + nx * half), px(y + ny * half));
            outline[2 * n - 1 - i] = point(px(x - nx * half), px(y - ny * half));
        }
        self.polygon(&mut outline);
    }

    pub fn build(self) -> Option<Path> {
        if self.contours == 0 {
            return None;
        }
        self.path.build().ok()
    }
}

/// Segments to flatten a curve of about `len` screen pixels into.
pub(crate) fn curve_steps(len: f32) -> usize {
    ((len / 12.).ceil() as usize).clamp(4, 24)
}

pub(crate) fn draw_self_loop(batch: &mut TriangleBatch, c: Point<Pixels>, r: f32) {
    let s = r.max(6.);
    let (cx, cy) = (c.x.as_f32(), c.y.as_f32());
    let p0 = (cx + r * 0.6, cy - r * 0.8);
    let p1 = (cx + s * 1.2, cy - s * 3.);
    let p2 = (cx + s * 3., cy - s * 1.2);
    let p3 = (cx + r * 0.8, cy - r * 0.6);
    let steps = curve_steps(s * 6.);
    batch.polyline((0..=steps).map(|k| {
        let t = k as f32 / steps as f32;
        let u = 1. - t;
        let f = |a: f32, b: f32, c: f32, d: f32| {
            u * u * u * a + 3. * u * u * t * b + 3. * u * t * t * c + t * t * t * d
        };
        point(px(f(p0.0, p1.0, p2.0, p3.0)), px(f(p0.1, p1.1, p2.1, p3.1)))
    }));
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

    /// Curve parameter about `d` pixels in from the start.
    pub fn t_from_start(&self, d: f32) -> f32 {
        (d / self.approx_len().max(1.)).min(0.5)
    }

    /// Curve parameter about `d` pixels in from the end.
    pub fn t_from_end(&self, d: f32) -> f32 {
        1. - self.t_from_start(d)
    }

    pub fn point_from_end(&self, d: f32) -> Point<Pixels> {
        self.eval(self.t_from_end(d))
    }

    /// Strokes the curve from parameter `t0` to `t1` into `batch`.
    pub fn stroke(&self, batch: &mut TriangleBatch, t0: f32, t1: f32) {
        match self.ctrl {
            None => batch.line(self.eval(t0), self.eval(t1)),
            Some(_) => {
                let steps = curve_steps(self.approx_len() * (t1 - t0));
                batch.polyline(
                    (0..=steps).map(|k| self.eval(t0 + (t1 - t0) * k as f32 / steps as f32)),
                );
            }
        }
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
    let cross = |o: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut hull: Vec<(f32, f32)> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &(f32, f32)>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.
            {
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
    let t = if len2 > 0. {
        ((apx * abx + apy * aby) / len2).clamp(0., 1.)
    } else {
        0.
    };
    (apx - abx * t).hypot(apy - aby * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_drops_interior_points() {
        let pts = vec![
            (0., 0.),
            (10., 0.),
            (10., 10.),
            (0., 10.),
            (5., 5.),
            (2., 7.),
        ];
        let hull = convex_hull(pts);
        assert_eq!(hull.len(), 4);
        assert!(!hull.contains(&(5., 5.)));
    }

    #[test]
    fn large_batches_are_not_dropped() {
        // Past lyon's u16 index limit, which used to drop the whole batch.
        let mut batch = TriangleBatch::new(1.2);
        for i in 0..20_000 {
            let y = px((i % 900) as f32);
            batch.line(point(px(0.), y), point(px(1600.), y));
            batch.triangle(
                point(px(0.), y),
                point(px(8.), y),
                point(px(4.), y + px(6.)),
            );
        }
        assert_eq!(batch.contours, 20_000 * 2);
        assert!(batch.build().is_some(), "non-empty batch builds");
    }

    #[test]
    fn stroke_covers_trimmed_curve() {
        let g = EdgeGeometry::new(point(px(0.), px(0.)), point(px(100.), px(0.)), 20.);
        let mut batch = TriangleBatch::new(2.);
        g.stroke(&mut batch, g.t_from_start(10.), g.t_from_end(10.));
        // One ribbon through several points, bulging toward the control point.
        assert_eq!(batch.contours, 1);
        assert!(batch.points.len() >= 2 * 5);
        let ys = || batch.points.iter().map(|p| p.y.as_f32());
        let min_y = ys().fold(f32::MAX, f32::min);
        let max_y = ys().fold(f32::MIN, f32::max);
        assert!(batch.build().is_some());
        assert!(max_y - min_y > 15., "{min_y}..{max_y}");
        assert!(TriangleBatch::new(1.).build().is_none());
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
