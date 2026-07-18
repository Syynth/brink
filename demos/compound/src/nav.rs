//! Guard navigation — room-graph pathfinding (#1044).
//!
//! v2's BSP generator (`layout_gen`) gives guards cross-room movement targets
//! (patrol posts, a last-known player position, an alarm panel, search points).
//! The old guard code walked *straight* at those targets with no regard for
//! walls, so any target in another room read as the guard phasing through a
//! wall or teleporting. This module fixes the *pathing* half of that: it builds
//! a graph whose **nodes are rooms** and whose **edges are doorways**, and finds
//! a path as a sequence of **door-center waypoints**. Movement is a straight
//! line only *within* a room — and a straight segment between two points of one
//! convex room rectangle can never cross that room's walls, so following the
//! waypoints never crosses geometry.
//!
//! **Doors are always traversable by guards** regardless of lock state: the
//! staff carry keys, and it also keeps the player's switch mechanic from ever
//! bricking guard navigation (a player locking a door must not be able to strand
//! the compound's guards). The player-only lock mechanic (`doors.rs`) is
//! unchanged. So every connection the generator emits — locked or open, tree or
//! loop — is an edge here.
//!
//! The wall-collision half of the fix lives in `guards.rs` (guards run the same
//! [`crate::world::resolve_collision`] the player does), as defense in depth:
//! even if a path were wrong, a guard can never end a frame inside a wall.
//!
//! Everything here is pure data + pure functions ([`RoomGraph::waypoints`] and
//! the A* search), unit-tested against both synthetic and generated layouts.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use bevy::math::{Rect, Vec2};
use bevy::prelude::Resource;

use crate::layout_gen::LayoutData;

/// One outgoing edge of a room: the neighbour room and the world-space center of
/// the doorway connecting them (the waypoint a guard walks to in order to cross).
#[derive(Debug, Clone, Copy)]
struct Edge {
    to: usize,
    door: Vec2,
}

/// A room-connectivity graph for guard pathfinding: rooms as nodes, doorways as
/// edges. Built once per round from the layout and stored in [`NavGraph`].
#[derive(Debug, Clone, Default)]
pub struct RoomGraph {
    /// Room interior rectangles, indexed by room id (matches `LayoutData::rooms`).
    rooms: Vec<Rect>,
    /// Adjacency: `adj[room]` lists every doorway leaving `room`.
    adj: Vec<Vec<Edge>>,
}

impl RoomGraph {
    /// Build the graph from a generated layout. Every door — locked or open — is
    /// an edge, because guards traverse all doors (staff keys, see module docs).
    #[must_use]
    pub fn from_layout(layout: &LayoutData) -> Self {
        let rooms: Vec<Rect> = layout.rooms.iter().map(|r| r.rect).collect();
        let mut adj: Vec<Vec<Edge>> = vec![Vec::new(); rooms.len()];
        for (a, b, door) in layout.room_adjacency() {
            if a < adj.len() && b < adj.len() {
                adj[a].push(Edge { to: b, door });
                adj[b].push(Edge { to: a, door });
            }
        }
        Self { rooms, adj }
    }

    /// The room index containing `p`, or — if `p` is on a wall/outside every
    /// room — the room whose center is nearest. Always returns a valid index for
    /// a non-empty graph.
    #[must_use]
    pub fn room_at(&self, p: Vec2) -> usize {
        if let Some(i) = self.rooms.iter().position(|r| r.contains(p)) {
            return i;
        }
        self.rooms
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.center()
                    .distance_squared(p)
                    .partial_cmp(&b.center().distance_squared(p))
                    .unwrap_or(Ordering::Equal)
            })
            .map_or(0, |(i, _)| i)
    }

    /// The interior rectangle of the room containing `p` (nearest-center
    /// fallback). Used to sample search points strictly inside a room (#1044).
    #[must_use]
    pub fn room_rect_at(&self, p: Vec2) -> Rect {
        let i = self.room_at(p);
        self.rooms
            .get(i)
            .copied()
            .unwrap_or_else(|| Rect::from_center_half_size(p, Vec2::splat(64.0)))
    }

    /// Whether the two rooms containing `a` and `b` are connected at all (guards
    /// pass every door, so this is plain graph connectivity). Used by the
    /// reachability tests.
    #[must_use]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn connected(&self, a: Vec2, b: Vec2) -> bool {
        let start = self.room_at(a);
        let goal = self.room_at(b);
        start == goal || self.astar_rooms(start, goal, a).is_some()
    }

    /// The movement waypoints to walk from `from` to `to`: the door centers of
    /// each room crossed, in order, followed by `to` itself. Within a single
    /// room this is just `[to]` — a straight line. If the two rooms are somehow
    /// disconnected (should not happen: the generator is solvable by
    /// construction and guards ignore locks), falls back to `[to]` and lets the
    /// caller's wall-collision keep the guard honest.
    #[must_use]
    pub fn waypoints(&self, from: Vec2, to: Vec2) -> Vec<Vec2> {
        if self.rooms.is_empty() {
            return vec![to];
        }
        let start = self.room_at(from);
        let goal = self.room_at(to);
        if start == goal {
            return vec![to];
        }
        match self.astar_rooms(start, goal, from) {
            Some(mut doors) => {
                doors.push(to);
                doors
            }
            None => vec![to],
        }
    }

    /// A* over the room graph from `start` to `goal`, entering `start` at world
    /// point `origin`. Edge cost is the straight-line distance between the door
    /// a room was entered through and the door leaving it; the heuristic is the
    /// straight-line distance from a candidate door to the goal room's center
    /// (admissible — the true remaining path is never shorter). Returns the
    /// sequence of door centers crossed, or `None` if `goal` is unreachable.
    ///
    /// Ties break on room index so the result is fully deterministic (the demo's
    /// determinism rule).
    fn astar_rooms(&self, start: usize, goal: usize, origin: Vec2) -> Option<Vec<Vec2>> {
        if start >= self.rooms.len() || goal >= self.rooms.len() {
            return None;
        }
        let goal_center = self.rooms[goal].center();
        let n = self.rooms.len();
        let mut g = vec![f32::INFINITY; n];
        let mut came_from = vec![usize::MAX; n];
        // The door center through which each room was reached (origin for start).
        let mut arrival = vec![Vec2::ZERO; n];
        let mut came_door = vec![Vec2::ZERO; n];

        g[start] = 0.0;
        arrival[start] = origin;
        let mut heap: BinaryHeap<Node> = BinaryHeap::new();
        heap.push(Node {
            f: origin.distance(goal_center),
            room: start,
        });

        while let Some(Node { room: u, .. }) = heap.pop() {
            if u == goal {
                break;
            }
            let base = g[u];
            let ap = arrival[u];
            for e in &self.adj[u] {
                let tentative = base + ap.distance(e.door);
                if tentative + 1e-4 < g[e.to] {
                    g[e.to] = tentative;
                    came_from[e.to] = u;
                    came_door[e.to] = e.door;
                    arrival[e.to] = e.door;
                    heap.push(Node {
                        f: tentative + e.door.distance(goal_center),
                        room: e.to,
                    });
                }
            }
        }

        if g[goal].is_infinite() {
            return None;
        }

        // Reconstruct the door sequence start → goal.
        let mut doors = Vec::new();
        let mut cur = goal;
        while cur != start {
            doors.push(came_door[cur]);
            cur = came_from[cur];
            if cur == usize::MAX {
                return None;
            }
        }
        doors.reverse();
        Some(doors)
    }
}

/// A* frontier entry. `Ord` sorts by `f` (ascending, via the `Reverse` behavior
/// implemented below) then room index, so the [`BinaryHeap`] pops the lowest-`f`
/// node and ties are deterministic.
#[derive(Debug, Clone, Copy)]
struct Node {
    f: f32,
    room: usize,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap on `f`: reverse the cost comparison so the smallest `f` is the
        // "greatest" element the max-heap yields first. Break ties on room index
        // (also reversed) for determinism.
        other
            .f
            .total_cmp(&self.f)
            .then_with(|| other.room.cmp(&self.room))
    }
}

/// The current round's room graph, rebuilt each round from the layout. `None`
/// between rounds or in tests that do not populate it (guards then fall back to
/// straight-line motion, still wall-clamped by `resolve_collision`).
#[derive(Resource, Debug, Default)]
pub struct NavGraph(pub Option<RoomGraph>);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout_gen::{Recipe, Room, generate};

    // A tiny hand-built graph: three rooms in a row A—B—C, doors at x=100 and
    // x=300, each room 200 wide.
    fn line_graph() -> RoomGraph {
        let rooms = vec![
            Rect::new(0.0, 0.0, 100.0, 100.0),   // A: x 0..100
            Rect::new(100.0, 0.0, 300.0, 100.0), // B: x 100..300
            Rect::new(300.0, 0.0, 400.0, 100.0), // C: x 300..400
        ];
        let mut adj = vec![Vec::new(); 3];
        let d_ab = Vec2::new(100.0, 50.0);
        let d_bc = Vec2::new(300.0, 50.0);
        adj[0].push(Edge { to: 1, door: d_ab });
        adj[1].push(Edge { to: 0, door: d_ab });
        adj[1].push(Edge { to: 2, door: d_bc });
        adj[2].push(Edge { to: 1, door: d_bc });
        RoomGraph { rooms, adj }
    }

    #[test]
    fn same_room_is_a_straight_line() {
        let g = line_graph();
        let from = Vec2::new(20.0, 50.0);
        let to = Vec2::new(80.0, 50.0);
        assert_eq!(g.waypoints(from, to), vec![to]);
    }

    #[test]
    fn adjacent_rooms_route_through_the_shared_door() {
        let g = line_graph();
        let from = Vec2::new(20.0, 50.0); // room A
        let to = Vec2::new(250.0, 50.0); // room B
        let wps = g.waypoints(from, to);
        assert_eq!(wps, vec![Vec2::new(100.0, 50.0), to]);
    }

    #[test]
    fn spanning_two_rooms_visits_both_doors_in_order() {
        let g = line_graph();
        let from = Vec2::new(20.0, 50.0); // A
        let to = Vec2::new(350.0, 50.0); // C
        let wps = g.waypoints(from, to);
        assert_eq!(
            wps,
            vec![Vec2::new(100.0, 50.0), Vec2::new(300.0, 50.0), to],
            "A→C funnels through both doors then the target"
        );
    }

    #[test]
    fn every_waypoint_but_the_last_is_a_door_center() {
        let g = line_graph();
        let doors = [Vec2::new(100.0, 50.0), Vec2::new(300.0, 50.0)];
        let wps = g.waypoints(Vec2::new(20.0, 50.0), Vec2::new(350.0, 50.0));
        for w in &wps[..wps.len() - 1] {
            assert!(
                doors.iter().any(|d| d.distance(*w) < 1e-3),
                "intermediate waypoint {w:?} must be a door center"
            );
        }
    }

    #[test]
    fn disconnected_falls_back_to_straight_line() {
        // Room D is isolated (no edges).
        let mut g = line_graph();
        g.rooms.push(Rect::new(1000.0, 0.0, 1100.0, 100.0));
        g.adj.push(Vec::new());
        let to = Vec2::new(1050.0, 50.0); // room D
        assert_eq!(g.waypoints(Vec2::new(20.0, 50.0), to), vec![to]);
    }

    // --- Generated-layout properties (reachability across many seeds) ---------

    /// The nav graph must connect every guard post to every other post and to
    /// the entry, on every generated layout — otherwise a guard could be handed
    /// a target it can never path to. Guards ignore locks, so this is plain
    /// graph connectivity over all doors.
    #[test]
    fn all_posts_mutually_reachable_across_seeds() {
        for seed in 0..1000u64 {
            let layout = generate(seed);
            let graph = RoomGraph::from_layout(&layout);

            // Anchor set: the entry room center + every guard post.
            let entry = layout
                .rooms
                .iter()
                .find(|r| r.recipe == Recipe::Entry)
                .map_or(layout.player_start, Room::center);

            let mut anchors = vec![entry];
            anchors.extend(layout.guard_posts.iter().copied());

            for (i, &a) in anchors.iter().enumerate() {
                for &b in &anchors[i + 1..] {
                    assert!(
                        graph.connected(a, b),
                        "seed {seed}: anchors {a:?} and {b:?} are not mutually reachable"
                    );
                }
            }
        }
    }

    /// Every post must be reachable from the entry, and the returned path's
    /// intermediate waypoints must each be an actual doorway center — never a
    /// point sampled through geometry.
    #[test]
    fn entry_to_every_post_traverses_only_doors() {
        for seed in 0..1000u64 {
            let layout = generate(seed);
            let graph = RoomGraph::from_layout(&layout);
            let doors: Vec<Vec2> = layout.doors.iter().map(|d| d.center).collect();
            let entry = layout.player_start;

            for &post in &layout.guard_posts {
                let wps = graph.waypoints(entry, post);
                assert!(!wps.is_empty(), "seed {seed}: empty path to {post:?}");
                assert!(
                    wps.last().is_some_and(|w| w.distance(post) < 1e-3),
                    "seed {seed}: path does not end at the post"
                );
                for w in &wps[..wps.len() - 1] {
                    assert!(
                        doors.iter().any(|d| d.distance(*w) < 1e-3),
                        "seed {seed}: waypoint {w:?} is not a door center"
                    );
                }
            }
        }
    }

    /// Consecutive waypoints must lie in the same room (straight-line-safe): a
    /// step from one waypoint to the next never spans two rooms without a door
    /// between them. We check the weaker, robust invariant that each waypoint is
    /// inside some room rectangle (doors sit on a shared room edge, so a small
    /// inset tolerance covers the boundary).
    #[test]
    fn waypoints_stay_on_the_room_lattice() {
        for seed in 0..500u64 {
            let layout = generate(seed);
            let graph = RoomGraph::from_layout(&layout);
            let entry = layout.player_start;
            for &post in &layout.guard_posts {
                for w in graph.waypoints(entry, post) {
                    let near = layout.rooms.iter().any(|r| r.rect.inflate(1.0).contains(w));
                    assert!(near, "seed {seed}: waypoint {w:?} lies outside every room");
                }
            }
        }
    }
}
