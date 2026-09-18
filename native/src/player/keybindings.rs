//! Player-only keyboard shortcuts — separate from any global sidebar/list
//! navigation, matching the old app's `onPlayerKeyDown` (only active while
//! something is playing). Keybindings are user-configurable strings
//! (`state::settings::KeybindingsConfig`) using the old app's own naming
//! convention (`"ArrowRight"`, `"Shift+ArrowLeft"`, `" "`, `"Escape"`), so
//! this module's job is resolving those strings against egui's typed `Key`
//! enum at input-check time, not storing anything itself.

use egui::Key;

fn key_from_name(name: &str) -> Option<Key> {
    match name {
        "ArrowRight" => Some(Key::ArrowRight),
        "ArrowLeft" => Some(Key::ArrowLeft),
        "ArrowUp" => Some(Key::ArrowUp),
        "ArrowDown" => Some(Key::ArrowDown),
        "Escape" => Some(Key::Escape),
        "Space" | " " => Some(Key::Space),
        "Enter" => Some(Key::Enter),
        _ if name.chars().count() == 1 => {
            let c = name.chars().next()?.to_ascii_uppercase();
            match c {
                'A' => Some(Key::A),
                'B' => Some(Key::B),
                'C' => Some(Key::C),
                'D' => Some(Key::D),
                'E' => Some(Key::E),
                'F' => Some(Key::F),
                'G' => Some(Key::G),
                'H' => Some(Key::H),
                'I' => Some(Key::I),
                'J' => Some(Key::J),
                'K' => Some(Key::K),
                'L' => Some(Key::L),
                'M' => Some(Key::M),
                'N' => Some(Key::N),
                'O' => Some(Key::O),
                'P' => Some(Key::P),
                'Q' => Some(Key::Q),
                'R' => Some(Key::R),
                'S' => Some(Key::S),
                'T' => Some(Key::T),
                'U' => Some(Key::U),
                'V' => Some(Key::V),
                'W' => Some(Key::W),
                'X' => Some(Key::X),
                'Y' => Some(Key::Y),
                'Z' => Some(Key::Z),
                _ => None,
            }
        }
        _ => None,
    }
}

/// True if any binding in the list was pressed *this frame* — matches the
/// old app's `matchBinding()`: a `"Shift+X"` binding requires shift held; a
/// plain binding is checked regardless of shift state (the old code never
/// required shift to be *absent* for a non-"Shift+" binding either).
pub fn any_pressed(input: &egui::InputState, bindings: &[String]) -> bool {
    for binding in bindings {
        let (needs_shift, name) = match binding.strip_prefix("Shift+") {
            Some(rest) => (true, rest),
            None => (false, binding.as_str()),
        };
        let Some(key) = key_from_name(name) else {
            continue;
        };
        if needs_shift && !input.modifiers.shift {
            continue;
        }
        if input.key_pressed(key) {
            return true;
        }
    }
    false
}
