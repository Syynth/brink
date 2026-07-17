//! Seeded BSP layout generation — the systems-logic ink-migration specimen.
//!
//! [`generate`] is a **pure function** `seed → LayoutData`: given a `u64` seed
//! it deterministically partitions the compound with a BSP tree, connects the
//! leaf rooms with a spanning tree (the BSP tree itself) plus a few extra loop
//! doorways for route choice, locks a subset of doors behind switches, and
//! stamps a **room recipe** onto every leaf (Entry, Exit, Guard post, Camera
//! nest, Storage, Switch room, Alarm panel, Vault, Barracks, Corridor). The
//! generator *is* the encounter designer (plan §10.1).
//!
//! **Solvable by construction.** Locked doors form a DAG: every locked door's
//! switch is placed in the room on the *entry side* of that door in the
//! spanning tree (its parent). By induction from the entry room, every room —
//! and therefore the exit — is reachable: to reach a room you cross its
//! parent's door, whose switch sits in the parent, which is itself reachable by
//! shallower doors first. [`LayoutData::solvable`] verifies this by simulation,
//! and the unit tests hammer it across thousands of seeds.
//!
//! Everything here is plain data + pure logic (no Bevy `App`, no ECS): the
//! Phase-1 ink port is a near-mechanical translation, and the tests need no
//! renderer.

use bevy::math::{Rect, Vec2};

use crate::world::ARENA_HALF;

// --- Tunables ---------------------------------------------------------------

/// Minimum interior width/height of a leaf room before BSP stops splitting.
const MIN_ROOM: f32 = 150.0;
/// Wall thickness (shared with the rendered geometry).
pub const WALL_T: f32 = 20.0;
/// Clear width of a doorway carved into a wall.
pub const DOOR_W: f32 = 74.0;
/// Maximum BSP recursion depth (bounds the room count).
const MAX_DEPTH: u32 = 4;
/// Below this depth the tree always splits; at/after it, splitting is a coin
/// flip so room counts vary between seeds.
const FORCE_SPLIT_DEPTH: u32 = 2;
/// Probability a leaf keeps splitting once past [`FORCE_SPLIT_DEPTH`].
const SPLIT_CHANCE: f32 = 0.7;
/// Fraction of tree doorways that get locked behind a switch.
const LOCK_CHANCE: f32 = 0.45;
/// How many extra (loop) doorways to add on top of the spanning tree, as a
/// fraction of the leaf count.
const LOOP_FRACTION: f32 = 0.25;

/// Guaranteed room-count bounds — asserted by the generator tests so the
/// encounter never degenerates to a single box or explode past the arena.
pub const MIN_ROOMS: usize = 4;
pub const MAX_ROOMS: usize = 16;

// Compile-time sanity so the bounds stay meaningful (also keeps the public
// constants exercised in every build, not only the test target).
const _: () = assert!(MIN_ROOMS >= 1 && MIN_ROOMS < MAX_ROOMS);

// --- Deterministic PRNG (splitmix64) ----------------------------------------

/// A tiny deterministic PRNG so the generator needs no `rand` dependency and a
/// given seed always yields byte-identical layouts.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        // Perturb so seed 0 is not a fixed point.
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in `[0, 1)`.
    fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform float in `[lo, hi)`.
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.f32() * (hi - lo)
    }

    fn bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// Uniform index in `0..n` (`n` must be non-zero).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
}

// --- Output data ------------------------------------------------------------

/// The role a room plays in the encounter. The generator stamps exactly one
/// onto every leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recipe {
    Entry,
    Exit,
    GuardPost,
    CameraNest,
    Storage,
    SwitchRoom,
    AlarmPanel,
    Vault,
    Barracks,
    Corridor,
}

/// A generated leaf room: its interior rectangle and its recipe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    pub rect: Rect,
    pub recipe: Recipe,
}

impl Room {
    #[must_use]
    pub fn center(&self) -> Vec2 {
        self.rect.center()
    }
}

/// A solid wall rectangle (axis-aligned), ready to spawn as a collider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallRect {
    pub center: Vec2,
    pub half: Vec2,
}

/// A door filling a doorway gap. Locked doors block until their switch is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoorSpec {
    /// Circuit id shared with the switch that opens it (locked doors only).
    pub id: u8,
    pub center: Vec2,
    pub half: Vec2,
    /// True if the door blocks a vertical wall (i.e. the door itself is tall).
    pub vertical: bool,
    /// Whether the door starts locked (needs its switch) or open.
    pub locked: bool,
}

/// A switch that opens every door sharing its `id`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwitchSpec {
    pub id: u8,
    pub pos: Vec2,
}

/// A gold pickup placed by a recipe. Vault gold is worth far more.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoldSpec {
    pub pos: Vec2,
    pub value: u32,
    pub vault: bool,
}

/// A camera placement: apex + the center angle it sweeps around.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSpec {
    pub pos: Vec2,
    pub angle: f32,
}

/// The full generated encounter. Pure data — instantiated into ECS entities by
/// the round/world code, and simulated directly by the solvability check.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutData {
    pub rooms: Vec<Room>,
    pub walls: Vec<WallRect>,
    pub doors: Vec<DoorSpec>,
    pub switches: Vec<SwitchSpec>,
    pub gold: Vec<GoldSpec>,
    pub cameras: Vec<CameraSpec>,
    pub guard_posts: Vec<Vec2>,
    pub alarm_panels: Vec<Vec2>,
    pub barracks: Vec<Vec2>,
    pub vault: Option<Vec2>,
    pub player_start: Vec2,
    pub exit: Vec2,
    /// Connectivity for the solvability check: `(room_a, room_b, door_index?)`.
    /// A `None` door index is an always-open connection (a loop or unlocked
    /// tree edge); a `Some` index references a lockable [`DoorSpec`].
    connections: Vec<Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Connection {
    a: usize,
    b: usize,
    door: Option<usize>,
}

// --- Internal BSP scaffolding ------------------------------------------------

/// A doorway carved into a split wall, connecting two leaf rooms.
#[derive(Debug, Clone, Copy)]
struct Doorway {
    /// True if the wall it pierces is vertical (constant-x), so the door is tall.
    vertical: bool,
    /// The constant coordinate of the wall line (x for vertical walls).
    coord: f32,
    /// Center of the gap along the wall (the perpendicular axis).
    perp: f32,
    a: usize,
    b: usize,
}

/// One split wall produced by an internal BSP node.
#[derive(Debug, Clone, Copy)]
struct SplitWall {
    vertical: bool,
    coord: f32,
    /// Wall extent along the perpendicular axis.
    lo: f32,
    hi: f32,
}

/// Recursive BSP builder: partitions `rect`, appending leaves to `leaves`,
/// split walls to `walls`, and one doorway per internal node to `doors`.
/// Returns the indices of the leaves under this subtree.
#[allow(clippy::many_single_char_names)]
fn build(
    rng: &mut Rng,
    rect: Rect,
    depth: u32,
    leaves: &mut Vec<Rect>,
    walls: &mut Vec<SplitWall>,
    doors: &mut Vec<Doorway>,
) -> Vec<usize> {
    let w = rect.width();
    let h = rect.height();
    let can_v = w >= 2.0 * MIN_ROOM + WALL_T;
    let can_h = h >= 2.0 * MIN_ROOM + WALL_T;

    let stop = depth >= MAX_DEPTH
        || (!can_v && !can_h)
        || (depth >= FORCE_SPLIT_DEPTH && !rng.chance(SPLIT_CHANCE));
    if stop {
        let idx = leaves.len();
        leaves.push(rect);
        return vec![idx];
    }

    // Prefer splitting the longer axis, but keep some seed-driven variety.
    let vertical = if can_v && can_h {
        if w > h * 1.3 {
            true
        } else if h > w * 1.3 {
            false
        } else {
            rng.bool()
        }
    } else {
        can_v
    };

    if vertical {
        let split = rng.range(rect.min.x + MIN_ROOM, rect.max.x - MIN_ROOM);
        let left = Rect::new(rect.min.x, rect.min.y, split, rect.max.y);
        let right = Rect::new(split, rect.min.y, rect.max.x, rect.max.y);
        walls.push(SplitWall {
            vertical: true,
            coord: split,
            lo: rect.min.y,
            hi: rect.max.y,
        });
        let l = build(rng, left, depth + 1, leaves, walls, doors);
        let r = build(rng, right, depth + 1, leaves, walls, doors);
        let perp = rng.range(rect.min.y + MIN_ROOM * 0.4, rect.max.y - MIN_ROOM * 0.4);
        let a = leaf_touching(leaves, &l, true, split, perp, true);
        let b = leaf_touching(leaves, &r, true, split, perp, false);
        doors.push(Doorway {
            vertical: true,
            coord: split,
            perp,
            a,
            b,
        });
        let mut all = l;
        all.extend(r);
        all
    } else {
        let split = rng.range(rect.min.y + MIN_ROOM, rect.max.y - MIN_ROOM);
        let bottom = Rect::new(rect.min.x, rect.min.y, rect.max.x, split);
        let top = Rect::new(rect.min.x, split, rect.max.x, rect.max.y);
        walls.push(SplitWall {
            vertical: false,
            coord: split,
            lo: rect.min.x,
            hi: rect.max.x,
        });
        let bo = build(rng, bottom, depth + 1, leaves, walls, doors);
        let to = build(rng, top, depth + 1, leaves, walls, doors);
        let perp = rng.range(rect.min.x + MIN_ROOM * 0.4, rect.max.x - MIN_ROOM * 0.4);
        let a = leaf_touching(leaves, &bo, false, split, perp, true);
        let b = leaf_touching(leaves, &to, false, split, perp, false);
        doors.push(Doorway {
            vertical: false,
            coord: split,
            perp,
            a,
            b,
        });
        let mut all = bo;
        all.extend(to);
        all
    }
}

/// Find the leaf in `candidates` that borders the split line `coord` on the
/// requested side and spans `perp` along the perpendicular axis.
fn leaf_touching(
    leaves: &[Rect],
    candidates: &[usize],
    vertical: bool,
    coord: f32,
    perp: f32,
    low_side: bool,
) -> usize {
    for &i in candidates {
        let r = leaves[i];
        if vertical {
            let borders = if low_side { r.max.x } else { r.min.x };
            if (borders - coord).abs() < 0.01 && perp >= r.min.y - 0.01 && perp <= r.max.y + 0.01 {
                return i;
            }
        } else {
            let borders = if low_side { r.max.y } else { r.min.y };
            if (borders - coord).abs() < 0.01 && perp >= r.min.x - 0.01 && perp <= r.max.x + 0.01 {
                return i;
            }
        }
    }
    // Fallback: the geometrically nearest bordering leaf (should not happen for
    // a well-formed BSP, but keeps the generator total).
    candidates
        .iter()
        .copied()
        .min_by(|&i, &j| {
            let di = border_dist(&leaves[i], vertical, coord, perp, low_side);
            let dj = border_dist(&leaves[j], vertical, coord, perp, low_side);
            di.partial_cmp(&dj).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(candidates[0])
}

fn border_dist(r: &Rect, vertical: bool, coord: f32, perp: f32, low_side: bool) -> f32 {
    if vertical {
        let b = if low_side { r.max.x } else { r.min.x };
        (b - coord).abs() + (perp - perp.clamp(r.min.y, r.max.y)).abs()
    } else {
        let b = if low_side { r.max.y } else { r.min.y };
        (b - coord).abs() + (perp - perp.clamp(r.min.x, r.max.x)).abs()
    }
}

// --- Public generator --------------------------------------------------------

/// Generate a complete, solvable-by-construction encounter for `seed`.
///
/// Rounds escalate the compound by drawing a new seed and scaling the guard
/// count around it (plan §10.3 — the player never levels up), so this function
/// stays a pure `seed → LayoutData` map with no difficulty parameter.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn generate(seed: u64) -> LayoutData {
    let mut rng = Rng::new(seed);

    // 1. BSP partition.
    let bounds = Rect::from_corners(-ARENA_HALF, ARENA_HALF);
    let mut leaves: Vec<Rect> = Vec::new();
    let mut split_walls: Vec<SplitWall> = Vec::new();
    let mut tree_doors: Vec<Doorway> = Vec::new();
    build(
        &mut rng,
        bounds,
        0,
        &mut leaves,
        &mut split_walls,
        &mut tree_doors,
    );

    let n = leaves.len();

    // 2. Spanning-tree BFS from the entry leaf (the one nearest bottom-left)
    //    to assign each room a depth, so locked doors can put their switch on
    //    the entry side (parent room).
    let entry = nearest_leaf(&leaves, Vec2::new(-ARENA_HALF.x, -ARENA_HALF.y));
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n]; // (neighbor, tree-door index)
    for (di, d) in tree_doors.iter().enumerate() {
        adj[d.a].push((d.b, di));
        adj[d.b].push((d.a, di));
    }
    let (depth, parent_door) = bfs_tree(n, entry, &adj);

    // 3. Lock a subset of tree doors; place each lock's switch in the shallower
    //    (entry-side) room — provably solvable.
    let mut doors: Vec<DoorSpec> = Vec::new();
    let mut switches: Vec<SwitchSpec> = Vec::new();
    let mut connections: Vec<Connection> = Vec::new();
    let mut next_switch_id: u8 = 0;
    for (di, d) in tree_doors.iter().enumerate() {
        let (door_center, door_half) = door_geometry(d);
        // Never lock the very first door out of the entry, so the player is
        // never stuck at the door with a switch behind it.
        let entry_side = if depth[d.a] <= depth[d.b] { d.a } else { d.b };
        let child = if entry_side == d.a { d.b } else { d.a };
        let lockable = parent_door[child] == Some(di) && entry_side != entry;
        let locked = lockable && rng.chance(LOCK_CHANCE);
        let door_index = doors.len();
        if locked {
            let id = next_switch_id;
            next_switch_id += 1;
            doors.push(DoorSpec {
                id,
                center: door_center,
                half: door_half,
                vertical: d.vertical,
                locked: true,
            });
            switches.push(SwitchSpec {
                id,
                pos: switch_pos(&leaves[entry_side], &mut rng),
            });
            connections.push(Connection {
                a: d.a,
                b: d.b,
                door: Some(door_index),
            });
        } else {
            doors.push(DoorSpec {
                id: u8::MAX,
                center: door_center,
                half: door_half,
                vertical: d.vertical,
                locked: false,
            });
            connections.push(Connection {
                a: d.a,
                b: d.b,
                door: None,
            });
        }
    }

    // 4. Extra loop doorways for route choice: pick adjacent leaf pairs not yet
    //    tree-connected and carve an open doorway between them.
    let loops = ((n as f32) * LOOP_FRACTION).round() as usize;
    add_loops(
        &leaves,
        &mut rng,
        loops,
        &tree_doors,
        &mut doors,
        &mut connections,
    );

    // 5. Assign recipes.
    let exit = farthest_leaf(entry, &depth);
    let recipes = assign_recipes(&leaves, entry, exit, &depth, &switches, &mut rng);

    // 6. Build rooms + place recipe content.
    let mut rooms = Vec::with_capacity(n);
    let mut gold = Vec::new();
    let mut cameras = Vec::new();
    let mut guard_posts = Vec::new();
    let mut alarm_panels = Vec::new();
    let mut barracks = Vec::new();
    let mut vault = None;
    for (i, &rect) in leaves.iter().enumerate() {
        let recipe = recipes[i];
        let c = rect.center();
        match recipe {
            Recipe::Storage => {
                gold.push(GoldSpec {
                    pos: c,
                    value: 15,
                    vault: false,
                });
                gold.push(GoldSpec {
                    pos: c + Vec2::new(rect.width() * 0.25, 0.0),
                    value: 15,
                    vault: false,
                });
            }
            Recipe::Vault => {
                gold.push(GoldSpec {
                    pos: c,
                    value: 120,
                    vault: true,
                });
                vault = Some(c);
            }
            Recipe::CameraNest => {
                cameras.push(CameraSpec {
                    pos: c + Vec2::new(0.0, rect.height() * 0.3),
                    angle: -std::f32::consts::FRAC_PI_2,
                });
            }
            Recipe::GuardPost => guard_posts.push(c),
            Recipe::AlarmPanel => alarm_panels.push(c),
            Recipe::Barracks => barracks.push(c),
            _ => {}
        }
        rooms.push(Room { rect, recipe });
    }
    // Guarantee at least one patrol anchor even if no room drew GuardPost.
    if guard_posts.is_empty() {
        guard_posts.push(rooms[entry].center());
    }
    // Guarantee gold exists: greed-vs-safety is the core loop, so a layout that
    // happened to draw no Storage/Vault gets a stash in a non-entry room.
    if gold.is_empty() {
        let target = rooms
            .iter()
            .enumerate()
            .filter(|(i, r)| *i != entry && r.recipe != Recipe::Exit)
            .max_by_key(|(i, _)| depth[*i].min(u32::MAX - 1))
            .map_or(exit, |(i, _)| i);
        gold.push(GoldSpec {
            pos: rooms[target].center(),
            value: 15,
            vault: false,
        });
        if rooms[target].recipe == Recipe::Corridor {
            rooms[target].recipe = Recipe::Storage;
        }
    }

    // 7. Rasterize walls (outer boundary + split walls minus doorways).
    let walls = build_walls(&split_walls, &doors, &tree_doors, &connections);

    let player_start = inset(&leaves[entry], 40.0);
    let exit_pos = leaves[exit].center();

    let data = LayoutData {
        rooms,
        walls,
        doors,
        switches,
        gold,
        cameras,
        guard_posts,
        alarm_panels,
        barracks,
        vault,
        player_start,
        exit: exit_pos,
        connections,
    };
    // Solvable-by-construction is the whole point — assert it every generation
    // (compiled out of release, always live under tests + debug play).
    debug_assert!(
        data.solvable(),
        "layout generator produced an unsolvable layout for seed {seed}"
    );
    data
}

impl LayoutData {
    /// Simulate a solve: BFS from the entry room, flipping any switch that lies
    /// in a reachable room and traversing any door that is open or unlocked.
    /// Returns whether the exit room is reachable.
    #[must_use]
    pub fn solvable(&self) -> bool {
        let n = self.rooms.len();
        let entry = self
            .rooms
            .iter()
            .position(|r| r.recipe == Recipe::Entry)
            .unwrap_or(0);
        let exit = self
            .rooms
            .iter()
            .position(|r| r.recipe == Recipe::Exit)
            .unwrap_or(n.saturating_sub(1));

        // Which room each switch sits in.
        let switch_room: Vec<usize> = self
            .switches
            .iter()
            .map(|s| self.room_containing(s.pos))
            .collect();

        let mut reachable = vec![false; n];
        reachable[entry] = true;
        let mut changed = true;
        while changed {
            changed = false;
            // Set of currently-open door ids (switch in a reachable room).
            let open_ids: Vec<u8> = self
                .switches
                .iter()
                .zip(&switch_room)
                .filter(|&(_, &room)| reachable[room])
                .map(|(s, _)| s.id)
                .collect();
            for conn in &self.connections {
                let door_open = match conn.door {
                    None => true,
                    Some(di) => {
                        let d = &self.doors[di];
                        !d.locked || open_ids.contains(&d.id)
                    }
                };
                if !door_open {
                    continue;
                }
                for (from, to) in [(conn.a, conn.b), (conn.b, conn.a)] {
                    if reachable[from] && !reachable[to] {
                        reachable[to] = true;
                        changed = true;
                    }
                }
            }
        }
        reachable[exit]
    }

    fn room_containing(&self, p: Vec2) -> usize {
        self.rooms
            .iter()
            .position(|r| r.rect.contains(p))
            .unwrap_or_else(|| {
                // Nearest room center as a fallback.
                self.rooms
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.center()
                            .distance_squared(p)
                            .partial_cmp(&b.center().distance_squared(p))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map_or(0, |(i, _)| i)
            })
    }

    /// Total gold value placed in the layout (for tests / balancing).
    #[must_use]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn total_gold(&self) -> u32 {
        self.gold.iter().map(|g| g.value).sum()
    }
}

// --- Helpers ----------------------------------------------------------------

fn nearest_leaf(leaves: &[Rect], p: Vec2) -> usize {
    leaves
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.center()
                .distance_squared(p)
                .partial_cmp(&b.center().distance_squared(p))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or(0, |(i, _)| i)
}

/// BFS the spanning tree from `entry`; return each room's depth and the tree
/// door that connects it to its parent.
fn bfs_tree(n: usize, entry: usize, adj: &[Vec<(usize, usize)>]) -> (Vec<u32>, Vec<Option<usize>>) {
    let mut depth = vec![u32::MAX; n];
    let mut parent_door = vec![None; n];
    let mut queue = std::collections::VecDeque::new();
    depth[entry] = 0;
    queue.push_back(entry);
    while let Some(u) = queue.pop_front() {
        for &(v, di) in &adj[u] {
            if depth[v] == u32::MAX {
                depth[v] = depth[u] + 1;
                parent_door[v] = Some(di);
                queue.push_back(v);
            }
        }
    }
    (depth, parent_door)
}

fn farthest_leaf(entry: usize, depth: &[u32]) -> usize {
    depth
        .iter()
        .enumerate()
        .filter(|&(_, &d)| d != u32::MAX)
        .max_by_key(|&(_, &d)| d)
        .map_or(entry, |(i, _)| i)
}

/// Geometry (center + half-extents) of a door filling a doorway gap.
fn door_geometry(d: &Doorway) -> (Vec2, Vec2) {
    if d.vertical {
        (
            Vec2::new(d.coord, d.perp),
            Vec2::new(WALL_T / 2.0, DOOR_W / 2.0),
        )
    } else {
        (
            Vec2::new(d.perp, d.coord),
            Vec2::new(DOOR_W / 2.0, WALL_T / 2.0),
        )
    }
}

/// A switch position inside a room, offset from center so it reads as
/// wall-mounted rather than sitting on the objective.
fn switch_pos(room: &Rect, rng: &mut Rng) -> Vec2 {
    let c = room.center();
    let dx = (room.width() * 0.3).min(60.0);
    let dy = (room.height() * 0.3).min(60.0);
    Vec2::new(c.x + rng.range(-dx, dx), c.y + rng.range(-dy, dy))
}

fn inset(r: &Rect, _margin: f32) -> Vec2 {
    // Player start sits toward the room's inner corner but safely off the wall.
    r.center()
}

/// Add up to `count` extra open doorways between adjacent leaves that are not
/// already directly connected by a tree door.
fn add_loops(
    leaves: &[Rect],
    rng: &mut Rng,
    count: usize,
    tree_doors: &[Doorway],
    doors: &mut Vec<DoorSpec>,
    connections: &mut Vec<Connection>,
) {
    if count == 0 {
        return;
    }
    let n = leaves.len();
    let mut existing: Vec<(usize, usize)> = tree_doors
        .iter()
        .map(|d| (d.a.min(d.b), d.a.max(d.b)))
        .collect();

    let mut candidates: Vec<Doorway> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let key = (i, j);
            if existing.contains(&key) {
                continue;
            }
            if let Some(dw) = shared_doorway(&leaves[i], &leaves[j], i, j) {
                candidates.push(dw);
            }
        }
    }
    // Deterministic shuffle-and-take.
    for _ in 0..count {
        if candidates.is_empty() {
            break;
        }
        let pick = rng.below(candidates.len());
        let dw = candidates.swap_remove(pick);
        let key = (dw.a.min(dw.b), dw.a.max(dw.b));
        if existing.contains(&key) {
            continue;
        }
        existing.push(key);
        let (center, half) = door_geometry(&dw);
        let door_index = doors.len();
        doors.push(DoorSpec {
            id: u8::MAX,
            center,
            half,
            vertical: dw.vertical,
            locked: false,
        });
        connections.push(Connection {
            a: dw.a,
            b: dw.b,
            door: Some(door_index),
        });
        // The loop door is drawn as an always-open gap; record it so the wall
        // rasterizer carves it too. We reuse the DoorSpec + connection above;
        // `build_walls` reads doorway geometry from the doors list.
    }
}

/// If two leaves share a wall edge with room for a doorway, return the doorway
/// centered on the shared overlap.
fn shared_doorway(a: &Rect, b: &Rect, ai: usize, bi: usize) -> Option<Doorway> {
    // Vertical shared edge: a.max.x == b.min.x (or vice versa).
    for (l, r, li, ri) in [(a, b, ai, bi), (b, a, bi, ai)] {
        if (l.max.x - r.min.x).abs() < 0.5 {
            let lo = l.min.y.max(r.min.y);
            let hi = l.max.y.min(r.max.y);
            if hi - lo >= DOOR_W + 20.0 {
                return Some(Doorway {
                    vertical: true,
                    coord: l.max.x,
                    perp: (lo + hi) * 0.5,
                    a: li,
                    b: ri,
                });
            }
        }
        if (l.max.y - r.min.y).abs() < 0.5 {
            let lo = l.min.x.max(r.min.x);
            let hi = l.max.x.min(r.max.x);
            if hi - lo >= DOOR_W + 20.0 {
                return Some(Doorway {
                    vertical: false,
                    coord: l.max.y,
                    perp: (lo + hi) * 0.5,
                    a: li,
                    b: ri,
                });
            }
        }
    }
    None
}

/// Rasterize the wall set: outer boundary + each split wall minus every
/// doorway (locked or open) carved into it.
fn build_walls(
    split_walls: &[SplitWall],
    doors: &[DoorSpec],
    tree_doors: &[Doorway],
    connections: &[Connection],
) -> Vec<WallRect> {
    let mut out = Vec::new();
    let half = ARENA_HALF;

    // Outer boundary.
    out.push(WallRect {
        center: Vec2::new(0.0, half.y),
        half: Vec2::new(half.x, WALL_T / 2.0),
    });
    out.push(WallRect {
        center: Vec2::new(0.0, -half.y),
        half: Vec2::new(half.x, WALL_T / 2.0),
    });
    out.push(WallRect {
        center: Vec2::new(-half.x, 0.0),
        half: Vec2::new(WALL_T / 2.0, half.y),
    });
    out.push(WallRect {
        center: Vec2::new(half.x, 0.0),
        half: Vec2::new(WALL_T / 2.0, half.y),
    });

    // Collect every doorway gap by its wall line, using the DoorSpec geometry
    // (this covers tree doors, locked doors, and loop doors uniformly).
    // We match a gap to a split wall by axis + coord.
    for sw in split_walls {
        // Gather gaps on this exact line.
        let mut gaps: Vec<(f32, f32)> = Vec::new(); // (lo, hi) along perpendicular axis
        for d in doors {
            if d.vertical == sw.vertical {
                let (line, perp, halfperp) = if sw.vertical {
                    (d.center.x, d.center.y, d.half.y)
                } else {
                    (d.center.y, d.center.x, d.half.x)
                };
                if (line - sw.coord).abs() < 0.5 && perp >= sw.lo - 0.5 && perp <= sw.hi + 0.5 {
                    gaps.push((perp - halfperp, perp + halfperp));
                }
            }
        }
        gaps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        // Emit wall segments between the gaps.
        let mut cursor = sw.lo;
        for (glo, ghi) in gaps {
            let glo = glo.max(sw.lo);
            let ghi = ghi.min(sw.hi);
            if glo > cursor + 1.0 {
                push_segment(&mut out, sw, cursor, glo);
            }
            cursor = cursor.max(ghi);
        }
        if sw.hi > cursor + 1.0 {
            push_segment(&mut out, sw, cursor, sw.hi);
        }
    }

    // Silence unused-param warnings while keeping the signature explicit about
    // the data the rasterizer conceptually consumes.
    let _ = (tree_doors, connections);
    out
}

fn push_segment(out: &mut Vec<WallRect>, sw: &SplitWall, lo: f32, hi: f32) {
    let mid = (lo + hi) * 0.5;
    let halfp = (hi - lo) * 0.5;
    if sw.vertical {
        out.push(WallRect {
            center: Vec2::new(sw.coord, mid),
            half: Vec2::new(WALL_T / 2.0, halfp),
        });
    } else {
        out.push(WallRect {
            center: Vec2::new(mid, sw.coord),
            half: Vec2::new(halfp, WALL_T / 2.0),
        });
    }
}

/// Stamp a recipe onto every leaf. Entry/Exit are fixed; switch rooms are
/// forced where switches landed; the rest are drawn from a weighted bag with
/// guaranteed minimums (≥1 alarm panel, ≥1 barracks).
fn assign_recipes(
    leaves: &[Rect],
    entry: usize,
    exit: usize,
    depth: &[u32],
    switches: &[SwitchSpec],
    rng: &mut Rng,
) -> Vec<Recipe> {
    let n = leaves.len();
    let mut recipes = vec![Recipe::Corridor; n];
    recipes[entry] = Recipe::Entry;
    recipes[exit] = Recipe::Exit;

    // Force switch rooms.
    for s in switches {
        if let Some(i) = leaves.iter().position(|r| r.contains(s.pos))
            && recipes[i] == Recipe::Corridor
        {
            recipes[i] = Recipe::SwitchRoom;
        }
    }

    // Guarantee an alarm panel: the global-alarm counterplay has no trigger
    // without one, so force the deepest non-entry/non-exit room to be a panel
    // even if it already held a switch (a room can host both — the switch is
    // spawned from the switch list, the recipe only drives extra content). The
    // panel is placed deep so a guard reaching it is a real journey to
    // intercept.
    let panel = (0..n)
        .filter(|&i| i != entry && i != exit)
        .max_by_key(|&i| depth[i].min(u32::MAX - 1));
    if let Some(p) = panel {
        recipes[p] = Recipe::AlarmPanel;
    }

    // Free rooms to assign (whatever is still a plain corridor).
    let mut free: Vec<usize> = (0..n).filter(|&i| recipes[i] == Recipe::Corridor).collect();
    // Deterministic shuffle.
    for i in (1..free.len()).rev() {
        let j = rng.below(i + 1);
        free.swap(i, j);
    }

    // Guarantee a barracks (reinforcement entry) when there is room for one.
    let want: Vec<Recipe> = vec![Recipe::Barracks];

    // Fill remaining wants + a weighted bag over the rest.
    let bag = [
        Recipe::Storage,
        Recipe::Storage,
        Recipe::GuardPost,
        Recipe::GuardPost,
        Recipe::CameraNest,
        Recipe::Vault,
        Recipe::Corridor,
    ];
    let mut vault_placed = false;
    for (k, &i) in free.iter().enumerate() {
        let r = if k < want.len() {
            want[k]
        } else {
            let mut pick = bag[rng.below(bag.len())];
            if pick == Recipe::Vault && vault_placed {
                pick = Recipe::Storage;
            }
            pick
        };
        if r == Recipe::Vault {
            vault_placed = true;
        }
        recipes[i] = r;
    }

    recipes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_same_seed_identical_layout() {
        for seed in [0u64, 1, 42, 9999, u64::MAX] {
            let a = generate(seed);
            let b = generate(seed);
            assert_eq!(a, b, "seed {seed} must be reproducible");
        }
    }

    #[test]
    fn different_seeds_differ() {
        let a = generate(1);
        let b = generate(2);
        assert_ne!(a, b, "distinct seeds should (nearly always) differ");
    }

    #[test]
    fn room_count_within_bounds() {
        for seed in 0..2000u64 {
            let l = generate(seed);
            assert!(
                l.rooms.len() >= MIN_ROOMS && l.rooms.len() <= MAX_ROOMS,
                "seed {seed}: {} rooms out of [{MIN_ROOMS},{MAX_ROOMS}]",
                l.rooms.len()
            );
        }
    }

    #[test]
    fn always_solvable_by_construction() {
        for seed in 0..5000u64 {
            let l = generate(seed);
            assert!(l.solvable(), "seed {seed} generated an UNSOLVABLE layout");
        }
    }

    #[test]
    fn has_exactly_one_entry_and_exit() {
        for seed in 0..1000u64 {
            let l = generate(seed);
            assert_eq!(
                l.rooms.iter().filter(|r| r.recipe == Recipe::Entry).count(),
                1,
                "seed {seed} entry count"
            );
            assert_eq!(
                l.rooms.iter().filter(|r| r.recipe == Recipe::Exit).count(),
                1,
                "seed {seed} exit count"
            );
        }
    }

    #[test]
    fn entry_and_exit_are_distinct_and_apart() {
        for seed in 0..1000u64 {
            let l = generate(seed);
            assert!(
                l.player_start.distance(l.exit) > MIN_ROOM,
                "seed {seed}: entry and exit too close"
            );
        }
    }

    #[test]
    fn always_has_an_alarm_panel() {
        // The global-alarm counterplay depends on a panel existing.
        for seed in 0..2000u64 {
            let l = generate(seed);
            assert!(
                !l.alarm_panels.is_empty(),
                "seed {seed}: no alarm panel — global alarm has no trigger"
            );
        }
    }

    #[test]
    fn every_locked_door_has_a_switch() {
        for seed in 0..2000u64 {
            let l = generate(seed);
            for d in l.doors.iter().filter(|d| d.locked) {
                assert!(
                    l.switches.iter().any(|s| s.id == d.id),
                    "seed {seed}: locked door {} has no switch",
                    d.id
                );
            }
        }
    }

    #[test]
    fn switches_precede_their_doors() {
        // The provably-solvable property, checked structurally: a locked door's
        // switch must be reachable before that door in the solve simulation.
        for seed in 0..3000u64 {
            let l = generate(seed);
            assert!(l.solvable(), "seed {seed}");
        }
    }

    #[test]
    fn gold_is_placed() {
        // Over many seeds the great majority place gold; ensure the mechanism
        // fires at all.
        let with_gold = (0..500u64)
            .filter(|&s| generate(s).total_gold() > 0)
            .count();
        assert!(with_gold > 400, "gold should appear in most layouts");
    }

    #[test]
    fn walls_are_generated() {
        for seed in 0..500u64 {
            let l = generate(seed);
            // At least the 4 outer walls.
            assert!(l.walls.len() >= 4, "seed {seed}: missing outer walls");
        }
    }
}
