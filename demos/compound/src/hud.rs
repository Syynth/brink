//! Heads-up display.
//!
//! Prints run state plus the per-frame behavior-system timing readout. Those
//! microsecond figures are the whole point of the instrumentation: they are the
//! Rust-side cost baseline the Phase 1 ink port is measured against.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::ShowCones;
use crate::alarm::Alarm;
use crate::guards::Guard;
use crate::rats::Rat;
use crate::rounds::Round;
use crate::stats::Loadout;
use crate::timing::{BehaviorTimings, micros};

/// Marks the HUD text entity.
#[derive(Component, Debug)]
pub struct HudText;

/// The one-line goal reminder (#1009): a persistent top-center banner so the
/// player never has to guess what "winning" means or how doors open.
const OBJECTIVE_TEXT: &str = "Reach the green exit — flip switches (E) to open doors";

/// Spawn the HUD text in the top-left corner, plus the top-center objective
/// banner.
pub fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 0.9, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            ..default()
        },
        HudText,
    ));

    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(0),
            right: px(0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|parent| {
            parent.spawn((
                Text::new(OBJECTIVE_TEXT),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.95, 0.85)),
            ));
        });
}

/// Refresh the HUD every frame.
#[allow(clippy::too_many_arguments)]
pub fn update_hud(
    round: Res<Round>,
    alarm: Res<Alarm>,
    loadout: Res<Loadout>,
    timings: Res<BehaviorTimings>,
    show_cones: Res<ShowCones>,
    diagnostics: Res<DiagnosticsStore>,
    guards: Query<(), With<Guard>>,
    rats: Query<(), With<Rat>>,
    mut hud: Query<&mut Text, With<HudText>>,
) {
    let Ok(mut text) = hud.single_mut() else {
        return;
    };

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(bevy::diagnostic::Diagnostic::smoothed)
        .unwrap_or(0.0);

    let guard_n = guards.iter().count();
    let rat_n = rats.iter().count();

    let alarm_state = if alarm.global {
        "GLOBAL"
    } else if alarm.tier() >= crate::alarm::REINFORCE_LEVEL {
        "sweeping"
    } else if alarm.level > 0.05 {
        "alerted"
    } else {
        "calm"
    };

    **text = format!(
        "THE COMPOUND\n\
         Round {round_n}   Banked {banked}g   Carrying {carried}g\n\
         Cams disabled {cams}   Coins {coins}   Smoke {smoke}\n\
         Alarm {alarm_lvl:.2} (tier {tier}, {alarm_state})   FPS {fps:.0}\n\
         Guards {guard_n}   Rats {rat_n}   Cones [F1] {cones}\n\
         \n\
         behavior systems (this frame):\n\
         \x20 guards  {t_guard}\n\
         \x20 cameras {t_cam}\n\
         \x20 doors   {t_door}\n\
         \x20 alarm   {t_alarm}\n\
         \x20 rats    {t_rats}\n\
         \x20 TOTAL   {t_total}\n\
         \n\
         WASD move  Shift run  E interact  LMB coin  Q smoke  + rats  R reset",
        round_n = round.number,
        banked = round.banked,
        carried = round.carried,
        cams = round.cameras_disabled,
        coins = loadout.coins,
        smoke = loadout.smokes,
        alarm_lvl = alarm.level,
        tier = alarm.tier(),
        cones = if show_cones.0 { "on" } else { "off" },
        t_guard = micros(timings.guards),
        t_cam = micros(timings.cameras),
        t_door = micros(timings.doors),
        t_alarm = micros(timings.alarm),
        t_rats = micros(timings.rats),
        t_total = micros(timings.total()),
    );
}
