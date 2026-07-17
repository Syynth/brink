//! Arena geometry, shared collision + vision math, and the player entity.
//!
//! This module owns the fixed level: outer walls, interior dividers, the two
//! door/switch pairs, and the exit objective. It also spawns the player and
//! holds the geometry helpers (`point_in_cone`, `resolve_collision`) that the
//! guard and camera behavior modules reuse.

use bevy::prelude::*;

use crate::stats::PlayerStats;

/// Inner half-extents of the play area (world units). The full arena is
/// `2 * ARENA_HALF`, sized to sit inside a ~1280x720 window without a
/// scrolling camera.
pub const ARENA_HALF: Vec2 = Vec2::new(600.0, 330.0);

/// Player collision + render half-size.
pub const PLAYER_HALF: Vec2 = Vec2::new(12.0, 12.0);

/// Where the player (re)starts each round.
pub const PLAYER_START: Vec2 = Vec2::new(-540.0, -270.0);

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// The infiltrator. Exactly one exists.
#[derive(Component, Debug)]
pub struct Player;

/// An axis-aligned solid. Present on walls and (while closed) doors.
#[derive(Component, Debug, Clone, Copy)]
pub struct Collider {
    pub half_extents: Vec2,
}

/// Static wall marker.
#[derive(Component, Debug)]
pub struct Wall;

/// The round objective: touch it to escape.
#[derive(Component, Debug)]
pub struct Exit;

// ---------------------------------------------------------------------------
// Geometry helpers (pure, unit-tested)
// ---------------------------------------------------------------------------

/// Whether `target` lies inside the vision cone rooted at `apex`, facing
/// `facing` radians, with the given half-angle and range.
#[must_use]
pub fn point_in_cone(apex: Vec2, facing: f32, half_angle: f32, range: f32, target: Vec2) -> bool {
    let to_target = target - apex;
    let dist = to_target.length();
    if dist > range || dist < f32::EPSILON {
        return dist < f32::EPSILON; // standing on the apex counts as seen
    }
    let facing_dir = Vec2::new(facing.cos(), facing.sin());
    let cos_theta = to_target.normalize().dot(facing_dir);
    cos_theta >= half_angle.cos()
}

/// A rectangular blocker: center + half-extents.
pub type Blocker = (Vec2, Vec2);

/// Resolve an axis-aligned move against a set of blockers. `desired` is where
/// the mover wants to be; `half` is its half-size. Returns the nearest
/// non-overlapping position, resolving each axis independently so the mover
/// slides along walls instead of sticking.
#[must_use]
pub fn resolve_collision(from: Vec2, desired: Vec2, half: Vec2, blockers: &[Blocker]) -> Vec2 {
    let mut pos = from;

    // X axis first.
    pos.x = desired.x;
    for &(center, bhalf) in blockers {
        if overlaps(pos, half, center, bhalf) {
            pos.x = from.x;
            break;
        }
    }

    // Then Y axis, using the (possibly reverted) X.
    pos.y = desired.y;
    for &(center, bhalf) in blockers {
        if overlaps(pos, half, center, bhalf) {
            pos.y = from.y;
            break;
        }
    }

    // Keep the mover inside the outer bounds regardless.
    let limit = ARENA_HALF - half;
    pos.x = pos.x.clamp(-limit.x, limit.x);
    pos.y = pos.y.clamp(-limit.y, limit.y);
    pos
}

fn overlaps(a: Vec2, ahalf: Vec2, b: Vec2, bhalf: Vec2) -> bool {
    (a.x - b.x).abs() < ahalf.x + bhalf.x && (a.y - b.y).abs() < ahalf.y + bhalf.y
}

/// Whether the straight segment `from → to` is unobstructed by any blocker
/// rectangle. This is the wall line-of-sight test that makes vision cones
/// respect geometry (plan §10.2): a guard whose cone mathematically covers the
/// player still cannot *see* them through a wall. Uses the slab method against
/// each axis-aligned box and reports "blocked" if the segment enters any box
/// strictly before reaching the target.
#[must_use]
pub fn raycast_clear(from: Vec2, to: Vec2, blockers: &[Blocker]) -> bool {
    let d = to - from;
    let len2 = d.length_squared();
    if len2 < f32::EPSILON {
        return true;
    }
    for &(center, half) in blockers {
        if segment_hits_aabb(from, d, center, half) {
            return false;
        }
    }
    true
}

/// Slab intersection of the parametric segment `from + t*d`, `t ∈ [0,1)`,
/// against the box `center ± half`. Returns true if it enters the box before
/// `t = 1` (the target). The tiny epsilons keep a ray that merely grazes the
/// target's own containing box (t≈1) from counting as blocked.
fn segment_hits_aabb(from: Vec2, d: Vec2, center: Vec2, half: Vec2) -> bool {
    let min = center - half;
    let max = center + half;
    let mut t_enter = 0.0f32;
    let mut t_exit = 1.0f32;
    for axis in 0..2 {
        let (o, dir, lo, hi) = (from[axis], d[axis], min[axis], max[axis]);
        if dir.abs() < f32::EPSILON {
            // Parallel to this slab: miss if the origin is outside it.
            if o < lo || o > hi {
                return false;
            }
        } else {
            let inv = 1.0 / dir;
            let mut t0 = (lo - o) * inv;
            let mut t1 = (hi - o) * inv;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            t_enter = t_enter.max(t0);
            t_exit = t_exit.min(t1);
            if t_enter > t_exit {
                return false;
            }
        }
    }
    // Blocked only if the overlap interval starts before the target and has
    // positive length inside the segment.
    t_enter < t_exit && t_enter < 1.0 - 1e-3 && t_exit > 1e-3
}

/// Draw a vision-cone outline (apex → arc → apex) with immediate-mode gizmos.
/// Shared by the guard and camera debug overlays.
pub fn draw_cone(
    gizmos: &mut Gizmos,
    apex: Vec2,
    facing: f32,
    half_angle: f32,
    range: f32,
    color: Color,
) {
    const SEGMENTS: usize = 10;
    let mut points = Vec::with_capacity(SEGMENTS + 2);
    points.push(apex);
    for i in 0..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let a = facing - half_angle + t * (2.0 * half_angle);
        points.push(apex + Vec2::new(a.cos(), a.sin()) * range);
    }
    points.push(apex);
    gizmos.linestrip_2d(points, color);
}

// ---------------------------------------------------------------------------
// Static arena setup (once, at startup)
// ---------------------------------------------------------------------------

const WALL_COLOR: Color = Color::srgb(0.35, 0.37, 0.42);
const FLOOR_COLOR: Color = Color::srgb(0.11, 0.12, 0.14);
const EXIT_COLOR: Color = Color::srgb(0.2, 0.85, 0.35);
const PLAYER_COLOR: Color = Color::srgb(0.3, 0.7, 1.0);

/// Spawn the persistent scene: the render camera, the floor backdrop, and the
/// player. The compound itself (walls, doors, exit, gold, guards, …) is
/// generated per round and instantiated by [`spawn_layout`], since every round
/// draws a fresh seeded layout (plan §10.1).
pub fn setup_static_world(mut commands: Commands) {
    commands.spawn(Camera2d);

    // Floor backdrop.
    commands.spawn((
        Sprite::from_color(FLOOR_COLOR, ARENA_HALF * 2.0),
        Transform::from_xyz(0.0, 0.0, -10.0),
    ));

    // Player (repositioned to the layout's entry room each round).
    commands.spawn((
        Sprite::from_color(PLAYER_COLOR, PLAYER_HALF * 2.0),
        Transform::from_translation(PLAYER_START.extend(1.0)),
        Player,
        PlayerStats::default(),
    ));
}

/// Spawn the generated compound's walls and exit as round-scoped entities.
/// Doors/switches, cameras, gold, guards, and alarm panels are instantiated by
/// their own modules from the same [`LayoutData`].
pub fn spawn_layout_walls(commands: &mut Commands, layout: &crate::layout_gen::LayoutData) {
    for w in &layout.walls {
        commands.spawn((
            Sprite::from_color(WALL_COLOR, w.half * 2.0),
            Transform::from_translation(w.center.extend(0.0)),
            Wall,
            Collider {
                half_extents: w.half,
            },
            crate::rounds::RoundScoped,
        ));
    }

    // Exit objective.
    commands.spawn((
        Sprite::from_color(EXIT_COLOR, Vec2::new(56.0, 56.0)),
        Transform::from_translation(layout.exit.extend(-1.0)),
        Exit,
        crate::rounds::RoundScoped,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cone_sees_target_ahead() {
        // Facing +x (0 rad), target directly ahead within range.
        assert!(point_in_cone(
            Vec2::ZERO,
            0.0,
            0.5,
            100.0,
            Vec2::new(50.0, 0.0)
        ));
    }

    #[test]
    fn cone_misses_target_behind() {
        assert!(!point_in_cone(
            Vec2::ZERO,
            0.0,
            0.5,
            100.0,
            Vec2::new(-50.0, 0.0)
        ));
    }

    #[test]
    fn cone_misses_out_of_range() {
        assert!(!point_in_cone(
            Vec2::ZERO,
            0.0,
            0.5,
            100.0,
            Vec2::new(150.0, 0.0)
        ));
    }

    #[test]
    fn cone_misses_outside_angle() {
        // 90 degrees off to the side, half-angle only ~28 deg.
        assert!(!point_in_cone(
            Vec2::ZERO,
            0.0,
            0.5,
            100.0,
            Vec2::new(0.0, 50.0)
        ));
    }

    #[test]
    fn collision_blocks_into_wall_but_slides() {
        // Wall centered at (50,0), half (10,50). Mover at (0,0) half (10,10).
        let blockers = [(Vec2::new(50.0, 0.0), Vec2::new(10.0, 50.0))];
        // Moving straight into it on x is blocked, but y motion is preserved.
        let from = Vec2::new(25.0, 0.0);
        let desired = Vec2::new(45.0, 20.0);
        let out = resolve_collision(from, desired, Vec2::new(10.0, 10.0), &blockers);
        assert!((out.x - from.x).abs() < f32::EPSILON, "x should be blocked");
        assert!((out.y - 20.0).abs() < f32::EPSILON, "y should slide");
    }

    #[test]
    fn raycast_clear_with_no_blockers() {
        assert!(raycast_clear(
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            &[]
        ));
    }

    #[test]
    fn raycast_blocked_by_wall_between() {
        // Wall centered at origin, spanning y. A horizontal ray through it is
        // blocked.
        let blockers = [(Vec2::ZERO, Vec2::new(10.0, 100.0))];
        assert!(!raycast_clear(
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            &blockers
        ));
    }

    #[test]
    fn raycast_clear_when_wall_is_off_to_the_side() {
        // Wall well above the ray line: no obstruction.
        let blockers = [(Vec2::new(0.0, 200.0), Vec2::new(10.0, 30.0))];
        assert!(raycast_clear(
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            &blockers
        ));
    }

    #[test]
    fn raycast_clear_when_wall_is_behind_the_target() {
        // Wall past the target along the ray direction is not "between".
        let blockers = [(Vec2::new(200.0, 0.0), Vec2::new(10.0, 100.0))];
        assert!(raycast_clear(
            Vec2::new(-100.0, 0.0),
            Vec2::new(100.0, 0.0),
            &blockers
        ));
    }

    #[test]
    fn raycast_blocked_diagonally() {
        let blockers = [(Vec2::ZERO, Vec2::new(15.0, 15.0))];
        assert!(!raycast_clear(
            Vec2::new(-80.0, -80.0),
            Vec2::new(80.0, 80.0),
            &blockers
        ));
    }

    #[test]
    fn collision_clamps_to_arena() {
        let out = resolve_collision(Vec2::ZERO, Vec2::new(99999.0, 99999.0), PLAYER_HALF, &[]);
        assert!(out.x <= ARENA_HALF.x - PLAYER_HALF.x + 0.01);
        assert!(out.y <= ARENA_HALF.y - PLAYER_HALF.y + 0.01);
    }
}
