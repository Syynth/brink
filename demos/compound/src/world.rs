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

/// Wall / door thickness.
const WALL_T: f32 = 20.0;

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

/// Spawn the camera, floor, static walls, exit, and player. Doors and switches
/// are spawned by [`crate::doors::spawn_doors`].
pub fn setup_static_world(mut commands: Commands) {
    commands.spawn(Camera2d);

    // Floor backdrop.
    commands.spawn((
        Sprite::from_color(FLOOR_COLOR, ARENA_HALF * 2.0),
        Transform::from_xyz(0.0, 0.0, -10.0),
    ));

    // Outer border walls (top, bottom, left, right).
    let w = ARENA_HALF.x;
    let h = ARENA_HALF.y;
    spawn_wall(&mut commands, Vec2::new(0.0, h), Vec2::new(w, WALL_T / 2.0));
    spawn_wall(
        &mut commands,
        Vec2::new(0.0, -h),
        Vec2::new(w, WALL_T / 2.0),
    );
    spawn_wall(
        &mut commands,
        Vec2::new(-w, 0.0),
        Vec2::new(WALL_T / 2.0, h),
    );
    spawn_wall(&mut commands, Vec2::new(w, 0.0), Vec2::new(WALL_T / 2.0, h));

    // Left divider at x=-200, leaving a doorway gap near the top.
    spawn_wall(
        &mut commands,
        Vec2::new(-200.0, -135.0),
        Vec2::new(WALL_T / 2.0, 195.0),
    );
    // Right divider at x=200, leaving a doorway gap near the bottom.
    spawn_wall(
        &mut commands,
        Vec2::new(200.0, 135.0),
        Vec2::new(WALL_T / 2.0, 195.0),
    );

    // Exit objective, top-right room.
    commands.spawn((
        Sprite::from_color(EXIT_COLOR, Vec2::new(60.0, 60.0)),
        Transform::from_xyz(540.0, 270.0, -1.0),
        Exit,
    ));

    // Player.
    commands.spawn((
        Sprite::from_color(PLAYER_COLOR, PLAYER_HALF * 2.0),
        Transform::from_translation(PLAYER_START.extend(1.0)),
        Player,
        PlayerStats::default(),
    ));
}

fn spawn_wall(commands: &mut Commands, center: Vec2, half: Vec2) {
    commands.spawn((
        Sprite::from_color(WALL_COLOR, half * 2.0),
        Transform::from_translation(center.extend(0.0)),
        Wall,
        Collider { half_extents: half },
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
    fn collision_clamps_to_arena() {
        let out = resolve_collision(Vec2::ZERO, Vec2::new(99999.0, 99999.0), PLAYER_HALF, &[]);
        assert!(out.x <= ARENA_HALF.x - PLAYER_HALF.x + 0.01);
        assert!(out.y <= ARENA_HALF.y - PLAYER_HALF.y + 0.01);
    }
}
