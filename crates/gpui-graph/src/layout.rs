//! A d3-force style simulation: link, many-body (Barnes–Hut), collide and
//! centering forces, integrated with velocity Verlet and an annealing `alpha`.

/// Tuning knobs, named after their d3-force counterparts.
///
/// ```ignore
/// ForceConfig::default().with_link_distance(90.).with_charge(-400.)
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ForceConfig {
    /// `forceLink().distance()`: rest length of an edge, in world units.
    pub link_distance: f32,
    /// `forceManyBody().strength()`: negative repels.
    pub charge: f32,
    /// `forceManyBody().theta()`: Barnes–Hut accuracy, lower is more exact.
    pub theta: f32,
    /// Ignore charge beyond this distance; keeps far components from drifting apart.
    pub charge_distance_max: f32,
    /// `forceCollide()` padding added to each node's radius.
    pub collide_padding: f32,
    pub collide_strength: f32,
    /// `forceX/forceY` pull toward the origin; holds disconnected components together.
    pub gravity: f32,
    /// Pull of each group (community) member toward its group's centroid, so
    /// communities form visible clusters. 0 disables.
    pub group_strength: f32,
    pub velocity_decay: f32,
    pub alpha_min: f32,
    pub alpha_decay: f32,
}

impl Default for ForceConfig {
    fn default() -> Self {
        let alpha_min: f32 = 0.001;
        Self {
            link_distance: 60.,
            charge: -220.,
            theta: 0.9,
            charge_distance_max: 2000.,
            collide_padding: 4.,
            collide_strength: 0.7,
            gravity: 0.04,
            group_strength: 0.12,
            velocity_decay: 0.4,
            alpha_min,
            alpha_decay: 1. - alpha_min.powf(1. / 300.),
        }
    }
}

macro_rules! with_fields {
    ($ty:ty { $($field:ident: $t:ty => $with:ident),* $(,)? }) => {
        impl $ty {
            $(
                pub fn $with(mut self, value: $t) -> Self {
                    self.$field = value;
                    self
                }
            )*
        }
    };
}
pub(crate) use with_fields;

with_fields!(ForceConfig {
    link_distance: f32 => with_link_distance,
    charge: f32 => with_charge,
    theta: f32 => with_theta,
    charge_distance_max: f32 => with_charge_distance_max,
    collide_padding: f32 => with_collide_padding,
    collide_strength: f32 => with_collide_strength,
    gravity: f32 => with_gravity,
    group_strength: f32 => with_group_strength,
    velocity_decay: f32 => with_velocity_decay,
});

impl ForceConfig {
    /// Sets `alpha_min` and derives `alpha_decay` so the layout settles in about `ticks` steps.
    pub fn with_settle_ticks(mut self, ticks: u32, alpha_min: f32) -> Self {
        self.alpha_min = alpha_min;
        self.alpha_decay = 1. - alpha_min.powf(1. / ticks.max(1) as f32);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Body {
    pub pos: Vec2,
    pub vel: Vec2,
    /// Pinned position (while dragged, or pinned by the user).
    pub fixed: Option<Vec2>,
    pub radius: f32,
}

pub struct Simulation {
    pub config: ForceConfig,
    pub alpha: f32,
    pub alpha_target: f32,
    quadtree: QuadTree,
    rng: u32,
}

impl Simulation {
    pub fn new(config: ForceConfig) -> Self {
        Self {
            config,
            alpha: 1.,
            alpha_target: 0.,
            quadtree: QuadTree::default(),
            rng: 0x9e37_79b9,
        }
    }

    pub fn is_active(&self) -> bool {
        self.alpha >= self.config.alpha_min || self.alpha_target > self.config.alpha_min
    }

    /// Restart the annealing from `alpha` (d3 `simulation.alpha(a).restart()`).
    pub fn reheat(&mut self, alpha: f32) {
        self.alpha = self.alpha.max(alpha);
    }

    /// Place bodies without a position on a phyllotaxis spiral, like d3 does.
    pub fn initialize_positions(bodies: &mut [Body], start: usize) {
        const INITIAL_RADIUS: f32 = 10.;
        let angle = std::f32::consts::PI * (3. - 5f32.sqrt());
        for (i, body) in bodies.iter_mut().enumerate().skip(start) {
            let r = INITIAL_RADIUS * (0.5 + i as f32).sqrt();
            let a = i as f32 * angle;
            body.pos = Vec2::new(r * a.cos(), r * a.sin());
            body.vel = Vec2::default();
        }
    }

    /// Advance one step. `groups` lists body indices per cluster for the group force.
    pub fn tick(
        &mut self,
        bodies: &mut [Body],
        links: &[(usize, usize)],
        degrees: &[usize],
        groups: &[Vec<usize>],
    ) {
        if bodies.is_empty() {
            return;
        }
        self.alpha += (self.alpha_target - self.alpha) * self.config.alpha_decay;
        let alpha = self.alpha;

        self.apply_links(bodies, links, degrees, alpha);
        self.apply_many_body(bodies, alpha);
        self.apply_collide(bodies);
        self.apply_groups(bodies, groups, alpha);
        let gravity = self.config.gravity * alpha;
        for b in bodies.iter_mut() {
            b.vel.x -= b.pos.x * gravity;
            b.vel.y -= b.pos.y * gravity;
        }

        let keep = 1. - self.config.velocity_decay;
        for b in bodies.iter_mut() {
            if let Some(fixed) = b.fixed {
                b.pos = fixed;
                b.vel = Vec2::default();
            } else {
                b.vel.x *= keep;
                b.vel.y *= keep;
                b.pos.x += b.vel.x;
                b.pos.y += b.vel.y;
            }
        }
    }

    fn jiggle(&mut self) -> f32 {
        // xorshift; only used to separate coincident points.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32 - 0.5) * 1e-6
    }

    fn apply_links(
        &mut self,
        bodies: &mut [Body],
        links: &[(usize, usize)],
        degrees: &[usize],
        alpha: f32,
    ) {
        for &(s, t) in links {
            if s == t {
                continue;
            }
            let (ds, dt) = (degrees[s].max(1) as f32, degrees[t].max(1) as f32);
            let strength = 1. / ds.min(dt);
            let bias = ds / (ds + dt);
            let (bs, bt) = (&bodies[s], &bodies[t]);
            let mut x = bt.pos.x + bt.vel.x - bs.pos.x - bs.vel.x;
            let mut y = bt.pos.y + bt.vel.y - bs.pos.y - bs.vel.y;
            if x == 0. {
                x = self.jiggle();
            }
            if y == 0. {
                y = self.jiggle();
            }
            let len = (x * x + y * y).sqrt();
            let l = (len - self.config.link_distance) / len * alpha * strength;
            x *= l;
            y *= l;
            bodies[t].vel.x -= x * bias;
            bodies[t].vel.y -= y * bias;
            bodies[s].vel.x += x * (1. - bias);
            bodies[s].vel.y += y * (1. - bias);
        }
    }

    fn apply_groups(&self, bodies: &mut [Body], groups: &[Vec<usize>], alpha: f32) {
        let k = self.config.group_strength * alpha;
        if k == 0. {
            return;
        }
        for members in groups.iter().filter(|m| m.len() > 1) {
            let n = members.len() as f32;
            let (cx, cy) = members.iter().fold((0., 0.), |(x, y), &i| {
                (x + bodies[i].pos.x, y + bodies[i].pos.y)
            });
            let (cx, cy) = (cx / n, cy / n);
            for &i in members {
                bodies[i].vel.x += (cx - bodies[i].pos.x) * k;
                bodies[i].vel.y += (cy - bodies[i].pos.y) * k;
            }
        }
    }

    fn apply_many_body(&mut self, bodies: &mut [Body], alpha: f32) {
        self.quadtree.build(bodies);
        let theta2 = self.config.theta * self.config.theta;
        let max2 = self.config.charge_distance_max * self.config.charge_distance_max;
        let strength = self.config.charge;
        for (i, body) in bodies.iter_mut().enumerate() {
            let (fx, fy) = self.quadtree.force(i, strength * alpha, theta2, max2);
            body.vel.x += fx;
            body.vel.y += fy;
        }
    }

    /// Spatial-hash collision, so large result sets stay O(n).
    fn apply_collide(&mut self, bodies: &mut [Body]) {
        let pad = self.config.collide_padding;
        let max_r = bodies.iter().map(|b| b.radius).fold(0., f32::max) + pad;
        let cell = (max_r * 2.).max(1.);
        let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
        for (i, b) in bodies.iter().enumerate() {
            let key = (
                (b.pos.x / cell).floor() as i32,
                (b.pos.y / cell).floor() as i32,
            );
            grid.entry(key).or_default().push(i);
        }
        let strength = self.config.collide_strength;
        for i in 0..bodies.len() {
            let (cx, cy) = (
                (bodies[i].pos.x / cell).floor() as i32,
                (bodies[i].pos.y / cell).floor() as i32,
            );
            for gx in cx - 1..=cx + 1 {
                for gy in cy - 1..=cy + 1 {
                    let Some(cell_items) = grid.get(&(gx, gy)) else {
                        continue;
                    };
                    for &j in cell_items {
                        if j <= i {
                            continue;
                        }
                        let (a, b) = (&bodies[i], &bodies[j]);
                        let r = a.radius + b.radius + pad * 2.;
                        let mut x = a.pos.x + a.vel.x - b.pos.x - b.vel.x;
                        let mut y = a.pos.y + a.vel.y - b.pos.y - b.vel.y;
                        let mut l = x * x + y * y;
                        if l >= r * r {
                            continue;
                        }
                        if x == 0. {
                            x = self.jiggle();
                            l += x * x;
                        }
                        if y == 0. {
                            y = self.jiggle();
                            l += y * y;
                        }
                        let len = l.sqrt();
                        let k = (r - len) / len * strength;
                        let (ra, rb) = (a.radius * a.radius, b.radius * b.radius);
                        let w = rb / (ra + rb).max(f32::EPSILON);
                        x *= k;
                        y *= k;
                        bodies[i].vel.x += x * w;
                        bodies[i].vel.y += y * w;
                        bodies[j].vel.x -= x * (1. - w);
                        bodies[j].vel.y -= y * (1. - w);
                    }
                }
            }
        }
    }
}

const EMPTY: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Cell {
    x0: f32,
    y0: f32,
    size: f32,
    /// Number of bodies below this cell (each has unit charge).
    count: f32,
    cx: f32,
    cy: f32,
    /// Leaf payload: first body, chained through `QuadTree::next`.
    body: u32,
    /// Index of the first of four consecutive children, or `EMPTY` for a leaf.
    children: u32,
}

impl Cell {
    fn new(x0: f32, y0: f32, size: f32) -> Self {
        Self {
            x0,
            y0,
            size,
            count: 0.,
            cx: 0.,
            cy: 0.,
            body: EMPTY,
            children: EMPTY,
        }
    }

    fn is_leaf(&self) -> bool {
        self.children == EMPTY
    }

    fn quadrant(&self, p: Vec2) -> u32 {
        let half = self.size / 2.;
        let right = (p.x >= self.x0 + half) as u32;
        let bottom = (p.y >= self.y0 + half) as u32;
        right | (bottom << 1)
    }
}

/// Barnes–Hut quadtree over unit charges. Rebuilt every tick; buffers are reused.
#[derive(Default)]
struct QuadTree {
    cells: Vec<Cell>,
    /// Intrusive linked list of bodies sharing a leaf.
    next: Vec<u32>,
    pos: Vec<Vec2>,
    stack: Vec<u32>,
}

impl QuadTree {
    const MAX_DEPTH: u32 = 24;

    fn build(&mut self, bodies: &[Body]) {
        self.cells.clear();
        self.next.clear();
        self.next.resize(bodies.len(), EMPTY);
        self.pos.clear();
        self.pos.extend(bodies.iter().map(|b| b.pos));
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &self.pos {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        let size = (x1 - x0).max(y1 - y0).max(1.) * 1.0001;
        self.cells.push(Cell::new(x0, y0, size));
        for i in 0..self.pos.len() as u32 {
            self.insert(i);
        }
        self.accumulate(0);
    }

    fn insert(&mut self, body: u32) {
        let p = self.pos[body as usize];
        let mut cell = 0usize;
        let mut depth = 0;
        loop {
            let c = self.cells[cell];
            if c.is_leaf() {
                if c.body == EMPTY || depth >= Self::MAX_DEPTH {
                    // Empty leaf, or (nearly) coincident points: chain here.
                    self.next[body as usize] = c.body;
                    self.cells[cell].body = body;
                    return;
                }
                self.split(cell);
            }
            let c = self.cells[cell];
            cell = (c.children + c.quadrant(p)) as usize;
            depth += 1;
        }
    }

    /// Turn a leaf into an inner cell, moving its bodies into the new children.
    fn split(&mut self, cell: usize) {
        let c = self.cells[cell];
        let half = c.size / 2.;
        let first = self.cells.len() as u32;
        for q in 0..4 {
            let x = c.x0 + if q & 1 == 1 { half } else { 0. };
            let y = c.y0 + if q & 2 == 2 { half } else { 0. };
            self.cells.push(Cell::new(x, y, half));
        }
        self.cells[cell].children = first;
        self.cells[cell].body = EMPTY;
        let mut b = c.body;
        while b != EMPTY {
            let following = self.next[b as usize];
            let child = (first + c.quadrant(self.pos[b as usize])) as usize;
            self.next[b as usize] = self.cells[child].body;
            self.cells[child].body = b;
            b = following;
        }
    }

    fn accumulate(&mut self, cell: usize) -> (f32, f32, f32) {
        let c = self.cells[cell];
        let (mut n, mut sx, mut sy) = (0., 0., 0.);
        if c.is_leaf() {
            let mut b = c.body;
            while b != EMPTY {
                let p = self.pos[b as usize];
                n += 1.;
                sx += p.x;
                sy += p.y;
                b = self.next[b as usize];
            }
        } else {
            for q in 0..4 {
                let (cn, cx, cy) = self.accumulate((c.children + q) as usize);
                n += cn;
                sx += cx * cn;
                sy += cy * cn;
            }
        }
        let (cx, cy) = if n > 0. { (sx / n, sy / n) } else { (0., 0.) };
        let cell = &mut self.cells[cell];
        cell.count = n;
        cell.cx = cx;
        cell.cy = cy;
        (n, cx, cy)
    }

    /// Velocity change on body `i` from all other charges (d3 `forceManyBody`).
    fn force(&mut self, i: usize, strength: f32, theta2: f32, max2: f32) -> (f32, f32) {
        const DISTANCE_MIN2: f32 = 1.;
        let p = self.pos[i];
        let (mut fx, mut fy) = (0., 0.);
        self.stack.clear();
        self.stack.push(0);
        while let Some(ix) = self.stack.pop() {
            let c = self.cells[ix as usize];
            if c.count == 0. {
                continue;
            }
            if !c.is_leaf() {
                let (x, y) = (c.cx - p.x, c.cy - p.y);
                let l = x * x + y * y;
                if c.size * c.size / theta2 < l {
                    // Far enough: treat the whole cell as one aggregate charge.
                    if l < max2 {
                        let k = strength * c.count / l.max(DISTANCE_MIN2);
                        fx += x * k;
                        fy += y * k;
                    }
                } else {
                    self.stack.extend((0..4).map(|q| c.children + q));
                }
                continue;
            }
            let mut b = c.body;
            while b != EMPTY {
                if b as usize != i {
                    let q = self.pos[b as usize];
                    let (x, y) = (q.x - p.x, q.y - p.y);
                    let l = x * x + y * y;
                    if l < max2 {
                        let k = strength / l.max(DISTANCE_MIN2);
                        fx += x * k;
                        fy += y * k;
                    }
                }
                b = self.next[b as usize];
            }
        }
        (fx, fy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bodies(n: usize) -> Vec<Body> {
        let mut b = vec![
            Body {
                radius: 8.,
                ..Default::default()
            };
            n
        ];
        Simulation::initialize_positions(&mut b, 0);
        b
    }

    #[test]
    fn barnes_hut_matches_brute_force() {
        let b = bodies(300);
        let mut tree = QuadTree::default();
        tree.build(&b);
        for i in [0, 17, 150, 299] {
            // theta = 0 forces exact summation through the tree.
            let exact = tree.force(i, -1., 0., f32::MAX);
            let (mut fx, mut fy, mut scale) = (0., 0., 0.);
            for (j, o) in b.iter().enumerate() {
                if j != i {
                    let (x, y) = (o.pos.x - b[i].pos.x, o.pos.y - b[i].pos.y);
                    let l = (x * x + y * y).max(1.);
                    fx -= x / l;
                    fy -= y / l;
                    scale += x.hypot(y) / l;
                }
            }
            assert!((exact.0 - fx).abs() < 1e-3 && (exact.1 - fy).abs() < 1e-3);
            let approx = tree.force(i, -1., 0.81, f32::MAX);
            // Relative to the summed pair magnitudes: the net force can cancel to ~0.
            let err = (approx.0 - fx).hypot(approx.1 - fy) / scale;
            assert!(err < 0.02, "relative error {err}");
        }
    }

    #[test]
    fn coincident_points_do_not_overflow() {
        let mut b = vec![
            Body {
                radius: 5.,
                ..Default::default()
            };
            50
        ];
        let mut sim = Simulation::new(ForceConfig::default());
        let degrees = vec![0; 50];
        for _ in 0..20 {
            sim.tick(&mut b, &[], &degrees, &[]);
        }
        assert!(b.iter().all(|b| b.pos.x.is_finite() && b.pos.y.is_finite()));
    }

    #[test]
    fn simulation_settles_with_links_near_rest_length() {
        let n = 60;
        let mut b = bodies(n);
        let links: Vec<_> = (1..n).map(|i| (i, (i - 1) / 2)).collect();
        let mut degrees = vec![0; n];
        for &(s, t) in &links {
            degrees[s] += 1;
            degrees[t] += 1;
        }
        let mut sim = Simulation::new(ForceConfig::default());
        let mut ticks = 0;
        while sim.is_active() {
            sim.tick(&mut b, &links, &degrees, &[]);
            ticks += 1;
        }
        assert!(ticks < 400);
        let mean: f32 = links
            .iter()
            .map(|&(s, t)| (b[s].pos.x - b[t].pos.x).hypot(b[s].pos.y - b[t].pos.y))
            .sum::<f32>()
            / links.len() as f32;
        assert!(mean > 30. && mean < 150., "mean link length {mean}");
    }
}
