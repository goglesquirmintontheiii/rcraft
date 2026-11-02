//! Axis-aligned collision and vanilla-style entity movement.
//!
//! Constants follow the publicly documented player motion values
//! (minecraft.wiki "Entity" / "Transportation" pages): gravity 0.08 b/t²,
//! vertical drag 0.98, ground friction `slipperiness × 0.91`, jump 0.42,
//! step height 0.6.

#![forbid(unsafe_code)]
// Renderer/UI entry points take many plain parameters by design.
#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::needless_range_loop
)]

/// An axis-aligned box `[min_x, min_y, min_z, max_x, max_y, max_z]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Aabb {
    #[must_use]
    pub fn new(min: [f64; 3], max: [f64; 3]) -> Self {
        Self { min, max }
    }

    /// Box of `width`×`height` with its bottom centre at `pos`.
    #[must_use]
    pub fn at_feet(pos: [f64; 3], width: f64, height: f64) -> Self {
        let h = width / 2.0;
        Self {
            min: [pos[0] - h, pos[1], pos[2] - h],
            max: [pos[0] + h, pos[1] + height, pos[2] + h],
        }
    }

    #[must_use]
    pub fn offset(&self, d: [f64; 3]) -> Self {
        Self {
            min: [self.min[0] + d[0], self.min[1] + d[1], self.min[2] + d[2]],
            max: [self.max[0] + d[0], self.max[1] + d[1], self.max[2] + d[2]],
        }
    }

    /// Grow toward `d` (the swept volume).
    #[must_use]
    pub fn expand_towards(&self, d: [f64; 3]) -> Self {
        let mut r = *self;
        for k in 0..3 {
            if d[k] < 0.0 {
                r.min[k] += d[k];
            } else {
                r.max[k] += d[k];
            }
        }
        r
    }

    #[must_use]
    pub fn inflate(&self, e: f64) -> Self {
        Self {
            min: [self.min[0] - e, self.min[1] - e, self.min[2] - e],
            max: [self.max[0] + e, self.max[1] + e, self.max[2] + e],
        }
    }

    #[must_use]
    pub fn intersects(&self, o: &Aabb) -> bool {
        (0..3).all(|k| self.min[k] < o.max[k] && self.max[k] > o.min[k])
    }

    /// Clip movement `d` along `axis` so `self` does not enter `o`.
    #[must_use]
    pub fn clip(&self, o: &Aabb, axis: usize, d: f64) -> f64 {
        let (a, b) = match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        if self.max[a] <= o.min[a]
            || self.min[a] >= o.max[a]
            || self.max[b] <= o.min[b]
            || self.min[b] >= o.max[b]
        {
            return d;
        }
        const EPS: f64 = 1e-7;
        if d > 0.0 && self.max[axis] <= o.min[axis] + EPS {
            d.min(o.min[axis] - self.max[axis])
        } else if d < 0.0 && self.min[axis] >= o.max[axis] - EPS {
            d.max(o.max[axis] - self.min[axis])
        } else {
            d
        }
    }

    /// Ray intersection; returns (t, face normal axis index with sign).
    #[must_use]
    pub fn ray(&self, origin: [f64; 3], dir: [f64; 3]) -> Option<(f64, usize, bool)> {
        let mut tmin = f64::NEG_INFINITY;
        let mut tmax = f64::INFINITY;
        let mut axis = 0;
        let mut neg = false;
        for k in 0..3 {
            if dir[k].abs() < 1e-12 {
                if origin[k] < self.min[k] || origin[k] > self.max[k] {
                    return None;
                }
                continue;
            }
            let inv = 1.0 / dir[k];
            let mut t0 = (self.min[k] - origin[k]) * inv;
            let mut t1 = (self.max[k] - origin[k]) * inv;
            let mut n = true;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
                n = false;
            }
            if t0 > tmin {
                tmin = t0;
                axis = k;
                neg = n;
            }
            tmax = tmax.min(t1);
            if tmin > tmax {
                return None;
            }
        }
        (tmax >= 0.0).then_some((tmin.max(0.0), axis, neg))
    }
}

/// Collision boxes of the world around an entity.
pub trait Collider {
    /// Append world-space collision boxes intersecting `area`.
    fn boxes(&self, area: &Aabb, out: &mut Vec<Aabb>);
}

/// Result of [`move_and_collide`].
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveResult {
    pub moved: [f64; 3],
    pub hit_x: bool,
    pub hit_y: bool,
    pub hit_z: bool,
    pub on_ground: bool,
}

fn collide_axes(bb: &Aabb, d: [f64; 3], boxes: &[Aabb]) -> [f64; 3] {
    let mut d = d;
    let mut cur = *bb;
    // Y first, then the larger horizontal component.
    let order: [usize; 3] = if d[0].abs() < d[2].abs() {
        [1, 2, 0]
    } else {
        [1, 0, 2]
    };
    for &axis in &order {
        if d[axis] == 0.0 {
            continue;
        }
        let mut v = d[axis];
        for b in boxes {
            v = cur.clip(b, axis, v);
        }
        d[axis] = v;
        let mut off = [0.0; 3];
        off[axis] = v;
        cur = cur.offset(off);
    }
    d
}

/// Move `bb` by `d` against the world with step-up of `step` when grounded.
pub fn move_and_collide(
    bb: &Aabb,
    d: [f64; 3],
    step: f64,
    was_on_ground: bool,
    world: &dyn Collider,
    scratch: &mut Vec<Aabb>,
) -> MoveResult {
    scratch.clear();
    world.boxes(
        &bb.expand_towards(d)
            .inflate(1e-3)
            .expand_towards([0.0, step, 0.0]),
        scratch,
    );
    let mut r = collide_axes(bb, d, scratch);
    let hit_x = (r[0] - d[0]).abs() > 1e-9;
    let hit_z = (r[2] - d[2]).abs() > 1e-9;
    let falling_onto = d[1] < 0.0 && (r[1] - d[1]).abs() > 1e-9;
    if step > 0.0 && (hit_x || hit_z) && (was_on_ground || falling_onto) {
        // Try stepping up.
        let up = collide_axes(bb, [d[0], step, d[2]], scratch);
        let lifted = bb.offset(up);
        let down = collide_axes(&lifted, [0.0, -up[1] + d[1].min(0.0), 0.0], scratch);
        let stepped = [up[0], up[1] + down[1], up[2]];
        if stepped[0] * stepped[0] + stepped[2] * stepped[2] > r[0] * r[0] + r[2] * r[2] + 1e-9 {
            r = stepped;
        }
    }
    let hit_y = (r[1] - d[1]).abs() > 1e-9;
    MoveResult {
        moved: r,
        hit_x: (r[0] - d[0]).abs() > 1e-9,
        hit_y,
        hit_z: (r[2] - d[2]).abs() > 1e-9,
        on_ground: hit_y && d[1] < 0.0,
    }
}

/// Movement constants.
pub mod consts {
    pub const GRAVITY: f64 = 0.08;
    pub const DRAG_Y: f64 = 0.98;
    pub const AIR_FRICTION: f64 = 0.91;
    pub const DEFAULT_SLIP: f64 = 0.6;
    pub const WALK_SPEED: f64 = 0.1;
    pub const SPRINT_MULT: f64 = 1.3;
    pub const SNEAK_MULT: f64 = 0.3;
    pub const AIR_ACCEL: f64 = 0.02;
    pub const JUMP: f64 = 0.42;
    pub const STEP: f64 = 0.6;
    pub const FLY_SPEED: f64 = 0.05;
    pub const PLAYER_W: f64 = 0.6;
    pub const PLAYER_H: f64 = 1.8;
    pub const PLAYER_H_SNEAK: f64 = 1.5;
    pub const EYE: f64 = 1.62;
    pub const EYE_SNEAK: f64 = 1.27;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Floor;
    impl Collider for Floor {
        fn boxes(&self, area: &Aabb, out: &mut Vec<Aabb>) {
            for x in area.min[0].floor() as i32..=area.max[0].floor() as i32 {
                for z in area.min[2].floor() as i32..=area.max[2].floor() as i32 {
                    let (x, z) = (f64::from(x), f64::from(z));
                    out.push(Aabb::new([x, 63.0, z], [x + 1.0, 64.0, z + 1.0]));
                    if x == 3.0 {
                        out.push(Aabb::new([x, 64.0, z], [x + 1.0, 64.5, z + 1.0]));
                    }
                }
            }
        }
    }

    #[test]
    fn lands_and_steps_up_slab() {
        let mut s = Vec::new();
        let bb = Aabb::at_feet([0.5, 65.0, 0.5], 0.6, 1.8);
        let r = move_and_collide(&bb, [0.0, -2.0, 0.0], 0.6, false, &Floor, &mut s);
        assert!(r.on_ground);
        assert!((r.moved[1] + 1.0).abs() < 1e-9);
        let bb = Aabb::at_feet([2.5, 64.0, 0.5], 0.6, 1.8);
        let r = move_and_collide(&bb, [0.5, -0.08, 0.0], 0.6, true, &Floor, &mut s);
        assert!((r.moved[0] - 0.5).abs() < 1e-9, "{r:?}");
        assert!((r.moved[1] - 0.5).abs() < 1e-9, "{r:?}");
    }

    #[test]
    fn ray_hits_face() {
        let b = Aabb::new([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let (t, axis, neg) = b.ray([0.5, 3.0, 0.5], [0.0, -1.0, 0.0]).unwrap();
        assert!((t - 2.0).abs() < 1e-9);
        assert_eq!(axis, 1);
        assert!(!neg);
    }
}
