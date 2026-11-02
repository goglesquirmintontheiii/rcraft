//! Multi-noise biome selection: climate parameters, the R-tree the game
//! builds over the parameter list, and its search (including the warm-start
//! from the previous result, which decides ties exactly like the game).

use std::cell::Cell;

use serde_json::Value;

/// Quantise a climate value the way `Climate.quantizeCoord` does.
#[inline]
#[must_use]
pub fn quantize(f: f32) -> i64 {
    (f * 10000.0) as i64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Param {
    pub min: i64,
    pub max: i64,
}

impl Param {
    #[inline]
    fn distance(self, l: i64) -> i64 {
        let m = l - self.max;
        let n = self.min - l;
        if m > 0 { m } else { n.max(0) }
    }

    fn span(a: Param, b: Param) -> Param {
        Param {
            min: a.min.min(b.min),
            max: a.max.max(b.max),
        }
    }

    fn from_json(v: &Value) -> Param {
        match v {
            Value::Array(a) => Param {
                min: quantize(a[0].as_f64().unwrap() as f32),
                max: quantize(a[1].as_f64().unwrap() as f32),
            },
            n => {
                let q = quantize(n.as_f64().unwrap() as f32);
                Param { min: q, max: q }
            }
        }
    }
}

/// 7-D parameter point: temperature, humidity, continentalness, erosion,
/// depth, weirdness, offset.
pub type Space = [Param; 7];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target(pub [i64; 7]);

impl Target {
    #[must_use]
    pub fn new(t: f32, h: f32, c: f32, e: f32, d: f32, w: f32) -> Self {
        Self([
            quantize(t),
            quantize(h),
            quantize(c),
            quantize(e),
            quantize(d),
            quantize(w),
            0,
        ])
    }
}

#[derive(Debug)]
enum NodeKind {
    Leaf(u32),
    Sub(Vec<usize>),
}

#[derive(Debug)]
struct Node {
    space: Space,
    kind: NodeKind,
}

impl Node {
    #[inline]
    fn distance(&self, t: &[i64; 7]) -> i64 {
        let mut l = 0i64;
        for i in 0..7 {
            let d = self.space[i].distance(t[i]);
            l = l.wrapping_add(d.wrapping_mul(d));
        }
        l
    }
}

/// `Climate.RTree`.
#[derive(Debug)]
pub struct RTree {
    nodes: Vec<Node>,
    root: usize,
}

fn parameter_space(nodes: &[Node], children: &[usize]) -> Space {
    let mut s = nodes[children[0]].space;
    for &c in &children[1..] {
        for i in 0..7 {
            s[i] = Param::span(s[i], nodes[c].space[i]);
        }
    }
    s
}

fn cost(s: &Space) -> i64 {
    s.iter().map(|p| (p.max - p.min).abs()).sum()
}

impl RTree {
    /// Build from `(space, value)` leaves in parameter-list order.
    #[must_use]
    pub fn build(leaves: &[(Space, u32)]) -> Self {
        let mut nodes: Vec<Node> = leaves
            .iter()
            .map(|(s, v)| Node {
                space: *s,
                kind: NodeKind::Leaf(*v),
            })
            .collect();
        let ids: Vec<usize> = (0..nodes.len()).collect();
        let root = Self::build_rec(&mut nodes, ids);
        Self { nodes, root }
    }

    fn key(nodes: &[Node], n: usize, dim: usize, abs: bool) -> i64 {
        let p = nodes[n].space[dim];
        let l = (p.min + p.max) / 2;
        if abs { l.abs() } else { l }
    }

    fn sort(nodes: &[Node], list: &mut [usize], dim: usize, abs: bool) {
        // Java's List.sort is a stable merge sort; Rust's sort_by is stable.
        list.sort_by(|&a, &b| {
            for l in 0..7 {
                let d = (dim + l) % 7;
                let ka = Self::key(nodes, a, d, abs);
                let kb = Self::key(nodes, b, d, abs);
                match ka.cmp(&kb) {
                    std::cmp::Ordering::Equal => {}
                    o => return o,
                }
            }
            std::cmp::Ordering::Equal
        });
    }

    fn bucketize(nodes: &mut Vec<Node>, list: &[usize]) -> Vec<usize> {
        let size = list.len() as f64;
        let i = 6f64.powf(((size - 0.01).ln() / 6f64.ln()).floor()) as usize;
        let mut out = Vec::new();
        let mut cur = Vec::new();
        for &n in list {
            cur.push(n);
            if cur.len() >= i {
                let space = parameter_space(nodes, &cur);
                nodes.push(Node {
                    space,
                    kind: NodeKind::Sub(std::mem::take(&mut cur)),
                });
                out.push(nodes.len() - 1);
            }
        }
        if !cur.is_empty() {
            let space = parameter_space(nodes, &cur);
            nodes.push(Node {
                space,
                kind: NodeKind::Sub(cur),
            });
            out.push(nodes.len() - 1);
        }
        out
    }

    fn build_rec(nodes: &mut Vec<Node>, mut list: Vec<usize>) -> usize {
        if list.len() == 1 {
            return list[0];
        }
        if list.len() <= 6 {
            list.sort_by_key(|&n| {
                let mut l = 0i64;
                for j in 0..7 {
                    let p = nodes[n].space[j];
                    l += ((p.min + p.max) / 2).abs();
                }
                l
            });
            let space = parameter_space(nodes, &list);
            nodes.push(Node {
                space,
                kind: NodeKind::Sub(list),
            });
            return nodes.len() - 1;
        }
        let mut best = i64::MAX;
        let mut best_dim = 0;
        let mut best_buckets: Vec<usize> = Vec::new();
        for k in 0..7 {
            Self::sort(nodes, &mut list, k, false);
            let buckets = Self::bucketize(nodes, &list);
            let m: i64 = buckets.iter().map(|&b| cost(&nodes[b].space)).sum();
            if best > m {
                best = m;
                best_dim = k;
                best_buckets = buckets;
            }
        }
        Self::sort(nodes, &mut best_buckets, best_dim, true);
        let children: Vec<usize> = best_buckets
            .into_iter()
            .map(|b| {
                let NodeKind::Sub(ch) = &nodes[b].kind else {
                    unreachable!()
                };
                let ch = ch.clone();
                Self::build_rec(nodes, ch)
            })
            .collect();
        let space = parameter_space(nodes, &children);
        nodes.push(Node {
            space,
            kind: NodeKind::Sub(children),
        });
        nodes.len() - 1
    }

    fn search_rec(&self, n: usize, t: &[i64; 7], best: Option<usize>) -> usize {
        match &self.nodes[n].kind {
            NodeKind::Leaf(_) => n,
            NodeKind::Sub(children) => {
                let mut l = best.map_or(i64::MAX, |b| self.nodes[b].distance(t));
                let mut leaf2 = best;
                for &c in children {
                    let m = self.nodes[c].distance(t);
                    if l > m {
                        let leaf3 = self.search_rec(c, t, leaf2);
                        let d = if c == leaf3 {
                            m
                        } else {
                            self.nodes[leaf3].distance(t)
                        };
                        if l > d {
                            l = d;
                            leaf2 = Some(leaf3);
                        }
                    }
                }
                leaf2.expect("non-empty tree")
            }
        }
    }

    /// Search, warm-started from (and updating) `last`.
    #[must_use]
    pub fn search(&self, t: &Target, last: &Cell<Option<usize>>) -> u32 {
        let leaf = self.search_rec(self.root, &t.0, last.get());
        last.set(Some(leaf));
        match self.nodes[leaf].kind {
            NodeKind::Leaf(v) => v,
            NodeKind::Sub(_) => unreachable!(),
        }
    }
}

/// Parse a dimension's multi-noise `biomes` list into leaves; returns the
/// leaves and the biome names in first-appearance order.
#[must_use]
pub fn parse_parameter_list(list: &[Value]) -> (Vec<(Space, u32)>, Vec<String>) {
    let mut names: Vec<String> = Vec::new();
    let mut leaves = Vec::new();
    for e in list {
        let b = e["biome"].as_str().unwrap().to_owned();
        let idx = names.iter().position(|n| *n == b).unwrap_or_else(|| {
            names.push(b.clone());
            names.len() - 1
        }) as u32;
        let p = &e["parameters"];
        let space = [
            Param::from_json(&p["temperature"]),
            Param::from_json(&p["humidity"]),
            Param::from_json(&p["continentalness"]),
            Param::from_json(&p["erosion"]),
            Param::from_json(&p["depth"]),
            Param::from_json(&p["weirdness"]),
            Param::from_json(&p["offset"]),
        ];
        leaves.push((space, idx));
    }
    (leaves, names)
}
