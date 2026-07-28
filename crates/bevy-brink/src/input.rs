//! Tiny input helpers for keyboard-driven dialogue UIs.
//!
//! These don't depend on any bevy-brink type — they operate on Bevy's
//! `ButtonInput<KeyCode>` directly. They're shipped here mostly to spare
//! every example writing the same digit-key ladder.

use bevy_input::ButtonInput;
use bevy_input::keyboard::KeyCode;

const DIGIT_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// If a `Digit1`..=`Digit9` key was just pressed and the corresponding
/// 0-based index is within `0..max`, return that index. Otherwise
/// return `None`.
///
/// Convenience for the common "press 1 to pick the first choice" UI:
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use bevy_brink::{
/// #     BrinkContext, BrinkFlow, BrinkGlobals, Choice, RuntimeError, digit_key_to_choice_index,
/// #     flow_context_view,
/// # };
/// # fn example(
/// #     keys: Res<ButtonInput<KeyCode>>,
/// #     choices: &[Choice],
/// #     flow: &mut BrinkFlow,
/// #     globals: &mut BrinkGlobals,
/// #     ctx: &mut BrinkContext,
/// # ) -> Result<(), RuntimeError> {
/// if let Some(idx) = digit_key_to_choice_index(&keys, choices.len()) {
///     let mut view = flow_context_view(globals, ctx);
///     flow.choose(&mut view, idx)?;
/// }
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn digit_key_to_choice_index(keys: &ButtonInput<KeyCode>, max: usize) -> Option<usize> {
    for (idx, key) in DIGIT_KEYS.iter().enumerate() {
        if idx >= max {
            return None;
        }
        if keys.just_pressed(*key) {
            return Some(idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_index_when_in_range() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Digit3);
        assert_eq!(digit_key_to_choice_index(&keys, 5), Some(2));
    }

    #[test]
    fn returns_none_when_above_max() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Digit5);
        // Only 3 choices available; Digit5 maps to index 4 which is out.
        assert_eq!(digit_key_to_choice_index(&keys, 3), None);
    }

    #[test]
    fn returns_none_when_no_digit_pressed() {
        let keys = ButtonInput::<KeyCode>::default();
        assert_eq!(digit_key_to_choice_index(&keys, 5), None);
    }

    #[test]
    fn returns_none_when_max_zero() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Digit1);
        assert_eq!(digit_key_to_choice_index(&keys, 0), None);
    }
}
