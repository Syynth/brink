//! Doors and switches — the minimal reactive entity.
//!
//! A door is locked (solid) until its paired switch is flipped, at which point
//! it opens and stops blocking. This is the "await on a global/local" seam from
//! plan §3: in Phase 1 the door is an ink flow that suspends until a switch
//! value changes. Here it is a one-line reactive sync from switch state to door
//! state, timed alongside the other behavior systems.
//!
//! Legibility (#1009): a closed door looks nothing like a wall — it gets an
//! accent-colored frame, a padlock glyph, and a numeral — and the same accent
//! color + numeral appears on its switch, so "flip switch 0 to open door 0" is
//! readable at a glance instead of a guess. A switch also shows a `[E]` prompt
//! the moment the player is close enough to interact.

use bevy::prelude::*;
use std::time::Instant;

use crate::layout_gen::LayoutData;
use crate::rounds::RoundScoped;
use crate::timing::BehaviorTimings;
use crate::world::{Collider, Player};

const SWITCH_HALF: Vec2 = Vec2::new(14.0, 14.0);
const INTERACT_RADIUS: f32 = 46.0;

/// Per-id accent color shared by a switch and the door(s) it opens, so the
/// association reads as "same color = same circuit" without needing a legend.
const ACCENT_COLORS: [Color; 4] = [
    Color::srgb(0.25, 0.75, 0.95), // cyan
    Color::srgb(0.95, 0.6, 0.2),   // amber
    Color::srgb(0.7, 0.45, 0.95),  // violet
    Color::srgb(0.35, 0.9, 0.55),  // green
];

#[must_use]
fn accent_color(id: u8) -> Color {
    ACCENT_COLORS[id as usize % ACCENT_COLORS.len()]
}

/// A wall-mounted switch. Flipping it opens every door with the matching id.
#[derive(Component, Debug)]
pub struct Switch {
    pub id: u8,
    pub on: bool,
}

/// A door that blocks movement until its switch is on.
#[derive(Component, Debug)]
pub struct Door {
    pub switch_id: u8,
    pub open: bool,
}

/// Marks the `[E]` interact-prompt text entity that hovers over a switch.
#[derive(Component, Debug)]
pub struct SwitchPrompt {
    switch_id: u8,
}

/// Flip the nearest switch when the player presses E next to it.
pub fn switch_interact_system(
    keys: Res<ButtonInput<KeyCode>>,
    player: Query<&Transform, With<Player>>,
    mut switches: Query<(&Transform, &mut Switch)>,
) {
    if !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    let Ok(player_tf) = player.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    let mut best: Option<(f32, Mut<Switch>)> = None;
    for (tf, sw) in &mut switches {
        let d = tf.translation.truncate().distance(player_pos);
        if d < INTERACT_RADIUS && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            best = Some((d, sw));
        }
    }
    if let Some((_, mut sw)) = best {
        sw.on = !sw.on;
    }
}

/// Reactively sync each door's open state (and appearance) to its switch.
pub fn door_sync_system(
    switches: Query<&Switch>,
    mut doors: Query<(&mut Door, &mut Sprite)>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();

    for (mut door, mut sprite) in &mut doors {
        let open = switches.iter().any(|sw| sw.id == door.switch_id && sw.on);
        door.open = open;
        sprite.color = if open {
            // Clearly-open: mostly transparent, tinted by the door's own
            // accent so it still visually pairs with its switch.
            let mut c = accent_color(door.switch_id);
            c.set_alpha(0.18);
            c
        } else {
            // Clearly-locked: a distinct warm "hazard" fill, unlike any wall.
            Color::srgb(0.55, 0.18, 0.18)
        };
    }

    timings.doors = start.elapsed();
}

/// Keep switch sprites colored by state (cheap, untimed — it is cosmetic).
pub fn switch_visual_system(mut switches: Query<(&Switch, &mut Sprite)>) {
    for (sw, mut sprite) in &mut switches {
        sprite.color = if sw.on {
            Color::srgb(0.3, 0.85, 0.4)
        } else {
            Color::srgb(0.85, 0.75, 0.3)
        };
    }
}

/// Toggle each switch's `[E]` prompt based on player proximity, so the
/// interact affordance only shows up when it is actually usable.
pub fn switch_prompt_system(
    player: Query<&Transform, With<Player>>,
    switches: Query<(&Transform, &Switch)>,
    mut prompts: Query<(&SwitchPrompt, &mut Visibility)>,
) {
    let Ok(player_tf) = player.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    for (prompt, mut vis) in &mut prompts {
        let in_range = switches.iter().any(|(tf, sw)| {
            sw.id == prompt.switch_id
                && tf.translation.truncate().distance(player_pos) < INTERACT_RADIUS
        });
        *vis = if in_range {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// Draw the always-on (not F1-gated) door/switch legibility glyphs: an
/// accent-colored outline for both, plus a padlock glyph on closed doors.
/// This is core gameplay feedback, not a debug overlay, so it stays on
/// regardless of the vision-cone toggle.
pub fn draw_door_switch_glyphs(
    doors: Query<(&Transform, &Door, &Collider)>,
    switches: Query<(&Transform, &Switch)>,
    mut gizmos: Gizmos,
) {
    for (tf, door, collider) in &doors {
        let pos = tf.translation.truncate();
        let accent = accent_color(door.switch_id);
        gizmos.rect_2d(pos, collider.half_extents * 2.0, accent);
        if !door.open {
            draw_lock_glyph(&mut gizmos, pos);
        }
    }

    for (tf, sw) in &switches {
        let pos = tf.translation.truncate();
        let accent = accent_color(sw.id);
        gizmos.circle_2d(pos, SWITCH_HALF.x + 6.0, accent);
    }
}

/// A simple padlock silhouette (body + shackle ring + keyhole) built from
/// gizmo primitives, so it renders without depending on any font/glyph
/// coverage.
fn draw_lock_glyph(gizmos: &mut Gizmos, center: Vec2) {
    let glyph_color = Color::srgb(0.95, 0.93, 0.85);
    let body_half = Vec2::new(9.0, 7.0);
    let body_center = center + Vec2::new(0.0, -3.0);
    gizmos.rect_2d(body_center, body_half * 2.0, glyph_color);
    // Shackle: a ring above the body (a full circle reads unambiguously
    // regardless of arc-rotation direction, unlike a half-arc would).
    gizmos.circle_2d(center + Vec2::new(0.0, 5.0), 6.0, glyph_color);
    // Keyhole slot.
    gizmos.line_2d(
        body_center + Vec2::new(0.0, 3.0),
        body_center + Vec2::new(0.0, -3.0),
        Color::srgb(0.2, 0.2, 0.2),
    );
}

/// Spawn the layout's **locked** doors and their switches (closed / off), plus
/// id labels and interact prompts. Unlocked and loop connections are just
/// carved gaps in the wall geometry — they need no entity.
pub fn spawn_doors_from_layout(commands: &mut Commands, layout: &LayoutData) {
    for d in layout.doors.iter().filter(|d| d.locked) {
        let id = d.id;
        let accent = accent_color(id);

        commands.spawn((
            Sprite::from_color(Color::srgb(0.55, 0.18, 0.18), d.half * 2.0),
            Transform::from_translation(d.center.extend(0.5)),
            Door {
                switch_id: id,
                open: false,
            },
            Collider {
                half_extents: d.half,
            },
            RoundScoped,
        ));
        commands.spawn((
            Text2d::new(format!("{id}")),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(accent),
            Transform::from_translation(d.center.extend(1.5)),
            RoundScoped,
        ));
    }

    for s in &layout.switches {
        let id = s.id;
        let accent = accent_color(id);
        let switch_pos = s.pos;

        commands.spawn((
            Sprite::from_color(Color::srgb(0.85, 0.75, 0.3), SWITCH_HALF * 2.0),
            Transform::from_translation(switch_pos.extend(0.5)),
            Switch { id, on: false },
            RoundScoped,
        ));
        commands.spawn((
            Text2d::new(format!("{id}")),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(accent),
            Transform::from_translation((switch_pos + Vec2::new(0.0, 22.0)).extend(1.5)),
            RoundScoped,
        ));
        commands.spawn((
            Text2d::new("[E]"),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::srgb(0.95, 0.95, 0.9)),
            Transform::from_translation((switch_pos + Vec2::new(0.0, -24.0)).extend(1.5)),
            Visibility::Hidden,
            SwitchPrompt { switch_id: id },
            RoundScoped,
        ));
    }
}
