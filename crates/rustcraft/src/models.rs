//! Cuboid entity models in the community "box UV" layout used by the
//! game's entity textures (each cube's six faces unwrap around a texture
//! offset). Geometry is authored here; textures come from the player's
//! install, so the shapes are matched to the documented texture layouts.

/// One textured cuboid, in model pixels (Y down, origin 24 px above feet).
#[derive(Clone, Copy, Debug)]
pub struct Cube {
    pub from: [f32; 3],
    pub size: [f32; 3],
    pub uv: [f32; 2],
    pub inflate: f32,
    pub mirror: bool,
}

const fn cube(from: [f32; 3], size: [f32; 3], uv: [f32; 2]) -> Cube {
    Cube {
        from,
        size,
        uv,
        inflate: 0.0,
        mirror: false,
    }
}

/// A rigid group of cubes rotating about `pivot`.
#[derive(Clone, Debug)]
pub struct Part {
    pub name: &'static str,
    pub pivot: [f32; 3],
    /// Rest rotation (radians, applied Z then Y then X).
    pub rot: [f32; 3],
    pub cubes: Vec<Cube>,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub parts: Vec<Part>,
    pub tex: [f32; 2],
}

fn part(name: &'static str, pivot: [f32; 3], rot: [f32; 3], cubes: Vec<Cube>) -> Part {
    Part {
        name,
        pivot,
        rot,
        cubes,
    }
}

const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;

#[must_use]
pub fn cow() -> Model {
    Model {
        tex: [64.0, 32.0],
        parts: vec![
            part(
                "head",
                [0.0, 4.0, -8.0],
                [0.0; 3],
                vec![
                    cube([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0.0, 0.0]),
                    cube([-5.0, -5.0, -4.0], [1.0, 3.0, 1.0], [22.0, 0.0]),
                    cube([4.0, -5.0, -4.0], [1.0, 3.0, 1.0], [22.0, 0.0]),
                ],
            ),
            part(
                "body",
                [0.0, 5.0, 2.0],
                [HALF_PI, 0.0, 0.0],
                vec![
                    cube([-6.0, -10.0, -7.0], [12.0, 18.0, 10.0], [18.0, 4.0]),
                    cube([-2.0, 2.0, -8.0], [4.0, 6.0, 1.0], [52.0, 0.0]),
                ],
            ),
            part(
                "leg0",
                [-4.0, 12.0, 7.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg1",
                [4.0, 12.0, 7.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg2",
                [-4.0, 12.0, -6.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg3",
                [4.0, 12.0, -6.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])],
            ),
        ],
    }
}

#[must_use]
pub fn pig() -> Model {
    Model {
        tex: [64.0, 32.0],
        parts: vec![
            part(
                "head",
                [0.0, 12.0, -6.0],
                [0.0; 3],
                vec![
                    cube([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 0.0]),
                    cube([-2.0, 0.0, -9.0], [4.0, 3.0, 1.0], [16.0, 16.0]),
                ],
            ),
            part(
                "body",
                [0.0, 11.0, 2.0],
                [HALF_PI, 0.0, 0.0],
                vec![cube([-5.0, -10.0, -7.0], [10.0, 16.0, 8.0], [28.0, 8.0])],
            ),
            part(
                "leg0",
                [-3.0, 18.0, 7.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg1",
                [3.0, 18.0, 7.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg2",
                [-3.0, 18.0, -5.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg3",
                [3.0, 18.0, -5.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0])],
            ),
        ],
    }
}

#[must_use]
pub fn sheep() -> Model {
    Model {
        tex: [64.0, 32.0],
        parts: vec![
            part(
                "head",
                [0.0, 6.0, -8.0],
                [0.0; 3],
                vec![cube([-3.0, -4.0, -6.0], [6.0, 6.0, 8.0], [0.0, 0.0])],
            ),
            part(
                "body",
                [0.0, 5.0, 2.0],
                [HALF_PI, 0.0, 0.0],
                vec![cube([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [28.0, 8.0])],
            ),
            part(
                "leg0",
                [-3.0, 12.0, 7.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg1",
                [3.0, 12.0, 7.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg2",
                [-3.0, 12.0, -5.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])],
            ),
            part(
                "leg3",
                [3.0, 12.0, -5.0],
                [0.0; 3],
                vec![cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])],
            ),
        ],
    }
}

/// The wool layer drawn over a sheep with `sheep_fur.png`.
#[must_use]
pub fn sheep_fur() -> Model {
    let inf = |mut c: Cube, i: f32| {
        c.inflate = i;
        c
    };
    Model {
        tex: [64.0, 32.0],
        parts: vec![
            part(
                "head",
                [0.0, 6.0, -8.0],
                [0.0; 3],
                vec![inf(
                    cube([-3.0, -4.0, -4.0], [6.0, 6.0, 6.0], [0.0, 0.0]),
                    0.6,
                )],
            ),
            part(
                "body",
                [0.0, 5.0, 2.0],
                [HALF_PI, 0.0, 0.0],
                vec![inf(
                    cube([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [28.0, 8.0]),
                    1.75,
                )],
            ),
            part(
                "leg0",
                [-3.0, 12.0, 7.0],
                [0.0; 3],
                vec![inf(
                    cube([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0]),
                    0.5,
                )],
            ),
            part(
                "leg1",
                [3.0, 12.0, 7.0],
                [0.0; 3],
                vec![inf(
                    cube([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0]),
                    0.5,
                )],
            ),
            part(
                "leg2",
                [-3.0, 12.0, -5.0],
                [0.0; 3],
                vec![inf(
                    cube([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0]),
                    0.5,
                )],
            ),
            part(
                "leg3",
                [3.0, 12.0, -5.0],
                [0.0; 3],
                vec![inf(
                    cube([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0]),
                    0.5,
                )],
            ),
        ],
    }
}

#[must_use]
pub fn chicken() -> Model {
    Model {
        tex: [64.0, 32.0],
        parts: vec![
            part(
                "head",
                [0.0, 15.0, -4.0],
                [0.0; 3],
                vec![
                    cube([-2.0, -6.0, -2.0], [4.0, 6.0, 3.0], [0.0, 0.0]),
                    cube([-2.0, -4.0, -4.0], [4.0, 2.0, 2.0], [14.0, 0.0]),
                    cube([-1.0, -2.0, -3.0], [2.0, 2.0, 2.0], [14.0, 4.0]),
                ],
            ),
            part(
                "body",
                [0.0, 16.0, 0.0],
                [HALF_PI, 0.0, 0.0],
                vec![cube([-3.0, -4.0, -3.0], [6.0, 8.0, 6.0], [0.0, 9.0])],
            ),
            part(
                "leg0",
                [-2.0, 19.0, 1.0],
                [0.0; 3],
                vec![cube([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [26.0, 0.0])],
            ),
            part(
                "leg1",
                [1.0, 19.0, 1.0],
                [0.0; 3],
                vec![cube([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [26.0, 0.0])],
            ),
            part(
                "wing0",
                [-4.0, 13.0, 0.0],
                [0.0; 3],
                vec![cube([0.0, 0.0, -3.0], [1.0, 4.0, 6.0], [24.0, 13.0])],
            ),
            part(
                "wing1",
                [4.0, 13.0, 0.0],
                [0.0; 3],
                vec![cube([-1.0, 0.0, -3.0], [1.0, 4.0, 6.0], [24.0, 13.0])],
            ),
        ],
    }
}

/// The 64×64 player skin layout (classic "wide" arms).
#[must_use]
pub fn player() -> Model {
    let outer = |mut c: Cube, i: f32| {
        c.inflate = i;
        c
    };
    Model {
        tex: [64.0, 64.0],
        parts: vec![
            part(
                "head",
                [0.0, 0.0, 0.0],
                [0.0; 3],
                vec![
                    cube([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0]),
                    outer(cube([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [32.0, 0.0]), 0.5),
                ],
            ),
            part(
                "body",
                [0.0, 0.0, 0.0],
                [0.0; 3],
                vec![
                    cube([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0]),
                    outer(
                        cube([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 32.0]),
                        0.25,
                    ),
                ],
            ),
            part(
                "right_arm",
                [-5.0, 2.0, 0.0],
                [0.0; 3],
                vec![
                    cube([-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], [40.0, 16.0]),
                    outer(
                        cube([-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], [40.0, 32.0]),
                        0.25,
                    ),
                ],
            ),
            part(
                "left_arm",
                [5.0, 2.0, 0.0],
                [0.0; 3],
                vec![
                    cube([-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], [32.0, 48.0]),
                    outer(
                        cube([-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], [48.0, 48.0]),
                        0.25,
                    ),
                ],
            ),
            part(
                "right_leg",
                [-1.9, 12.0, 0.0],
                [0.0; 3],
                vec![
                    cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0]),
                    outer(cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 32.0]), 0.25),
                ],
            ),
            part(
                "left_leg",
                [1.9, 12.0, 0.0],
                [0.0; 3],
                vec![
                    cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [16.0, 48.0]),
                    outer(cube([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 48.0]), 0.25),
                ],
            ),
        ],
    }
}

/// A quad in model space: 4 corners (px, Y down) and UVs (0..1).
#[derive(Clone, Copy, Debug)]
pub struct ModelQuad {
    pub pos: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    /// Face normal in model space.
    pub normal: [f32; 3],
}

fn rotate(p: [f32; 3], r: [f32; 3]) -> [f32; 3] {
    let [mut x, mut y, mut z] = p;
    // Z, then Y, then X (as the game composes part rotations).
    if r[2] != 0.0 {
        let (s, c) = r[2].sin_cos();
        (x, y) = (x * c - y * s, x * s + y * c);
    }
    if r[1] != 0.0 {
        let (s, c) = r[1].sin_cos();
        (x, z) = (x * c + z * s, -x * s + z * c);
    }
    if r[0] != 0.0 {
        let (s, c) = r[0].sin_cos();
        (y, z) = (y * c - z * s, y * s + z * c);
    }
    [x, y, z]
}

fn cube_quads(
    c: &Cube,
    tex: [f32; 2],
    out: &mut Vec<ModelQuad>,
    xf: impl Fn([f32; 3]) -> [f32; 3],
) {
    let i = c.inflate;
    let (x0, y0, z0) = (c.from[0] - i, c.from[1] - i, c.from[2] - i);
    let (x1, y1, z1) = (
        c.from[0] + c.size[0] + i,
        c.from[1] + c.size[1] + i,
        c.from[2] + c.size[2] + i,
    );
    let [w, h, d] = c.size;
    let [u, v] = c.uv;
    let r = |a: f32, b: f32, cw: f32, ch: f32| {
        [a / tex[0], b / tex[1], (a + cw) / tex[0], (b + ch) / tex[1]]
    };
    // (corners TL, TR, BR, BL as seen from outside; uv rect; normal)
    let faces: [([[f32; 3]; 4], [f32; 4], [f32; 3]); 6] = [
        // front (-Z)
        (
            [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
            r(u + d, v + d, w, h),
            [0.0, 0.0, -1.0],
        ),
        // back (+Z)
        (
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            r(u + 2.0 * d + w, v + d, w, h),
            [0.0, 0.0, 1.0],
        ),
        // +X side
        (
            [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
            r(u, v + d, d, h),
            [1.0, 0.0, 0.0],
        ),
        // -X side
        (
            [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
            r(u + d + w, v + d, d, h),
            [-1.0, 0.0, 0.0],
        ),
        // top (-Y in model space is up)
        (
            [[x1, y0, z1], [x0, y0, z1], [x0, y0, z0], [x1, y0, z0]],
            r(u + d, v, w, d),
            [0.0, -1.0, 0.0],
        ),
        // bottom
        (
            [[x1, y1, z0], [x0, y1, z0], [x0, y1, z1], [x1, y1, z1]],
            r(u + d + w, v, w, d),
            [0.0, 1.0, 0.0],
        ),
    ];
    for (p, uv, n) in faces {
        let [a, b, cc, dd] = uv;
        let (a, cc) = if c.mirror { (cc, a) } else { (a, cc) };
        out.push(ModelQuad {
            pos: p.map(&xf),
            uv: [[a, b], [cc, b], [cc, dd], [a, dd]],
            normal: {
                let o = xf([0.0; 3]);
                let q = xf(n);
                [q[0] - o[0], q[1] - o[1], q[2] - o[2]]
            },
        });
    }
}

/// Posed model quads in model space. `pose(name)` returns extra rotation.
pub fn build(m: &Model, pose: impl Fn(&str) -> [f32; 3], out: &mut Vec<ModelQuad>) {
    for p in &m.parts {
        let extra = pose(p.name);
        let rot = [
            p.rot[0] + extra[0],
            p.rot[1] + extra[1],
            p.rot[2] + extra[2],
        ];
        for c in &p.cubes {
            cube_quads(c, m.tex, out, |q| {
                let r = rotate(q, rot);
                [r[0] + p.pivot[0], r[1] + p.pivot[1], r[2] + p.pivot[2]]
            });
        }
    }
}

/// Map a model-space point (px) to world space for an entity at `pos`
/// facing `yaw`, scaled by `scale`.
#[must_use]
pub fn to_world(p: [f32; 3], pos: [f64; 3], yaw: f32, scale: f32) -> [f64; 3] {
    let right = p[0] / 16.0 * scale;
    let up = (24.0 - p[1]) / 16.0 * scale;
    let fwd = -p[2] / 16.0 * scale;
    let (sy, cy) = f64::from(yaw).to_radians().sin_cos();
    let f = [-sy, cy];
    let r = [-cy, -sy];
    [
        pos[0] + f64::from(fwd) * f[0] + f64::from(right) * r[0],
        pos[1] + f64::from(up),
        pos[2] + f64::from(fwd) * f[1] + f64::from(right) * r[1],
    ]
}
