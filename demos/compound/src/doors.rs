//! Doors and switches — the minimal reactive entity.
//!
//! A door is locked (solid) until its paired switch is flipped, at which point
//! it opens and stops blocking. This is the "await on a global/local" seam from
//! plan §3: in Phase 1 the door is an ink flow that suspends until a switch
//! value changes. Here it is a one-line reactive sync from switch state to door
//! state, timed alongside the other behavior systems.

use bevy::prelude::*;
use std::time::Instant;

use crate::rounds::RoundScoped;
use crate::timing::BehaviorTimings;
use crate::world::{Collider, Player};

const SWITCH_HALF: Vec2 = Vec2::new(14.0, 14.0);
const INTERACT_RADIUS: f32 = 46.0;
const DOOR_HALF: Vec2 = Vec2::new(10.0, 135.0);

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
            Color::srgba(0.2, 0.8, 0.35, 0.25)
        } else {
            Color::srgb(0.7, 0.3, 0.3)
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

/// Spawn the round's doors and switches (closed / off). Called from round start.
pub fn spawn_doors(commands: &mut Commands) {
    // (door center, switch position, id)
    let pairs = [
        (Vec2::new(-200.0, 195.0), Vec2::new(-460.0, 150.0), 0u8),
        (Vec2::new(200.0, -195.0), Vec2::new(-40.0, -250.0), 1u8),
    ];
    for (door_pos, switch_pos, id) in pairs {
        commands.spawn((
            Sprite::from_color(Color::srgb(0.7, 0.3, 0.3), DOOR_HALF * 2.0),
            Transform::from_translation(door_pos.extend(0.5)),
            Door {
                switch_id: id,
                open: false,
            },
            Collider {
                half_extents: DOOR_HALF,
            },
            RoundScoped,
        ));
        commands.spawn((
            Sprite::from_color(Color::srgb(0.85, 0.75, 0.3), SWITCH_HALF * 2.0),
            Transform::from_translation(switch_pos.extend(0.5)),
            Switch { id, on: false },
            RoundScoped,
        ));
    }
}
