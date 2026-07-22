//! Shop intermission — weave-as-menu.
//!
//! Between rounds the player spends gold on upgrades. This is a `bevy_ui` menu
//! whose buttons are affordability-gated (mirroring ink's `* {gold >= 50}`
//! choice guards, plan §5). One-shot items grey out once owned; the medkit
//! restocks. In Phase 1 the whole screen becomes an ink weave, and the item
//! table becomes a `STRUCT` map — see `stats.rs`.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;

use crate::rounds::{LastOutcome, Phase, Round, StartRound};
use crate::stats::{CATALOGUE, Loadout, PlayerStats, apply_purchase, can_buy};
use crate::world::Player;

/// Root of the shop UI, despawned when leaving the shop.
#[derive(Component, Debug)]
pub struct ShopRoot;

/// What a shop button does.
#[derive(Component, Debug, Clone, Copy)]
pub enum ShopButton {
    /// Buy `CATALOGUE[index]`.
    Item(usize),
    /// Leave the shop and start the next round.
    Continue,
}

/// Marks the text on an item button so it can be refreshed.
#[derive(Component, Debug, Clone, Copy)]
pub struct ItemLabel(pub usize);

/// Marks the running "Gold: N" label.
#[derive(Component, Debug)]
pub struct GoldLabel;

const PANEL_BG: Color = Color::srgba(0.08, 0.09, 0.11, 0.97);
const AFFORD_BG: Color = Color::srgb(0.18, 0.4, 0.24);
const DENY_BG: Color = Color::srgb(0.26, 0.16, 0.16);
const CONTINUE_BG: Color = Color::srgb(0.2, 0.32, 0.5);

/// Build the shop screen when entering [`Phase::Shop`].
pub fn setup_shop(
    mut commands: Commands,
    round: Res<Round>,
    outcome: Res<LastOutcome>,
    loadout: Res<Loadout>,
) {
    let header = if outcome.escaped {
        format!("ESCAPED — round {} cleared", round.number)
    } else {
        format!("CAUGHT — round {} failed", round.number)
    };

    commands
        .spawn((
            ShopRoot,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(10),
                ..default()
            },
            BackgroundColor(PANEL_BG),
        ))
        .with_children(|parent| {
            parent.spawn(text(header, 30.0, Color::srgb(0.9, 0.9, 0.95)));
            parent.spawn(text(
                format!("+{} gold this round", outcome.reward),
                18.0,
                Color::srgb(0.75, 0.8, 0.7),
            ));
            parent.spawn((
                text(
                    format!("Gold: {}", round.banked),
                    24.0,
                    Color::srgb(0.95, 0.85, 0.3),
                ),
                GoldLabel,
            ));
            for i in 0..CATALOGUE.len() {
                spawn_item_button(parent, i, round.banked, &loadout);
            }
            spawn_continue_button(parent);
        });
}

/// A plain UI text bundle.
fn text(content: String, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn spawn_item_button(
    parent: &mut RelatedSpawnerCommands<'_, ChildOf>,
    index: usize,
    gold: u32,
    loadout: &Loadout,
) {
    let bg = if can_buy(&CATALOGUE[index], gold, loadout) {
        AFFORD_BG
    } else {
        DENY_BG
    };
    parent
        .spawn((
            Button,
            ShopButton::Item(index),
            Node {
                width: px(360),
                padding: UiRect::all(px(10)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(bg),
        ))
        .with_children(|b| {
            b.spawn((
                text(
                    item_label(index, loadout),
                    18.0,
                    Color::srgb(0.95, 0.95, 0.95),
                ),
                ItemLabel(index),
            ));
        });
}

fn spawn_continue_button(parent: &mut RelatedSpawnerCommands<'_, ChildOf>) {
    parent
        .spawn((
            Button,
            ShopButton::Continue,
            Node {
                width: px(360),
                padding: UiRect::all(px(12)),
                margin: UiRect::top(px(14)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(CONTINUE_BG),
        ))
        .with_children(|b| {
            b.spawn(text("Continue to next round".into(), 20.0, Color::WHITE));
        });
}

fn item_label(index: usize, loadout: &Loadout) -> String {
    let item = &CATALOGUE[index];
    if item.once_only && loadout.owned.contains(&item.id) {
        format!("{} — OWNED", item.name)
    } else {
        format!("{} — {}g  ({})", item.name, item.price, item.blurb)
    }
}

/// Handle shop button clicks: purchases and the continue action.
#[allow(clippy::too_many_arguments)]
pub fn shop_button_system(
    interactions: Query<(&Interaction, &ShopButton), Changed<Interaction>>,
    mut player: Query<&mut PlayerStats, With<Player>>,
    mut round: ResMut<Round>,
    mut loadout: ResMut<Loadout>,
    mut start: MessageWriter<StartRound>,
    mut next: ResMut<NextState<Phase>>,
) {
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *button {
            ShopButton::Item(i) => {
                let Ok(mut stats) = player.single_mut() else {
                    continue;
                };
                if let Some(price) =
                    apply_purchase(&CATALOGUE[i], round.banked, &mut stats, &mut loadout)
                {
                    round.banked -= price;
                }
            }
            ShopButton::Continue => {
                round.number += 1;
                start.write(StartRound);
                next.set(Phase::Playing);
            }
        }
    }
}

/// Refresh gold + item labels and affordability colors each frame in the shop.
pub fn shop_refresh_system(
    round: Res<Round>,
    loadout: Res<Loadout>,
    mut gold_labels: Query<&mut Text, With<GoldLabel>>,
    mut item_labels: Query<(&mut Text, &ItemLabel), Without<GoldLabel>>,
    mut buttons: Query<(&ShopButton, &mut BackgroundColor)>,
) {
    for mut text in &mut gold_labels {
        **text = format!("Gold: {}", round.banked);
    }
    for (mut text, label) in &mut item_labels {
        **text = item_label(label.0, &loadout);
    }
    for (button, mut bg) in &mut buttons {
        if let ShopButton::Item(i) = *button {
            *bg = if can_buy(&CATALOGUE[i], round.banked, &loadout) {
                AFFORD_BG.into()
            } else {
                DENY_BG.into()
            };
        }
    }
}

/// Tear down the shop UI when leaving the shop.
pub fn teardown_shop(mut commands: Commands, roots: Query<Entity, With<ShopRoot>>) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}
