//! Which physical key a key press came from, where egui cannot say: egui 0.37 reports the numpad's digits as the
//! number row's (Key::Num7 for both `Digit7` and `Numpad7`, on desktop and web), and ModelCraft's modeler keys
//! switch views from the numpad only (modelcraft.js reads `e.code`).
//!
//! The host tells this module each key press's code as the browser names it (`KeyboardEvent.code`: the web entry's
//! keydown listener; on the desktop winit's physical key and its location) with [`host_key`]. Before each frame
//! (`UiApp::raw_input_hook`) [`pair`] matches those codes to the frame's `Event::Key` presses in order, by their
//! physical key; a view then asks [`code_of`] for the code of each press it reads. A press the host said nothing
//! about still tells the numpad apart when Num Lock is off: its key is a navigation key (Home, End, PageUp …) while
//! its physical key is a digit, which the number row never sends.
//!
//! Everything here is per thread (the window's input runs on one thread) and capped: a host that reports codes
//! without frames to take them keeps at most [`CAP`].

use std::cell::RefCell;
use std::collections::VecDeque;

/// At most this many codes wait for a frame, and at most this many presses are named per frame.
pub const CAP: usize = 64;

thread_local! {
    /// Codes the host reported since the last frame, oldest first.
    static PENDING: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
    /// This frame's presses in order: egui's key, the code, whether a view read it already.
    static FRAME: RefCell<Vec<(egui::Key, Option<String>, bool)>> = const { RefCell::new(Vec::new()) };
}

/// The host: a key went down with this code (`KeyboardEvent.code`, e.g. "Numpad7", "Digit7", "KeyA"). Codes longer
/// than 32 bytes or not plain ASCII letters and digits are not codes and are ignored.
pub fn host_key(code: &str) {
    if code.is_empty() || code.len() > 32 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return;
    }
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        if p.len() >= CAP {
            p.pop_front();
        }
        p.push_back(code.to_string());
    });
}

/// The physical key a code names, as egui would name it (both "Digit7" and "Numpad7" are Num7).
fn code_key(code: &str) -> Option<egui::Key> {
    egui::Key::from_name(code)
}

/// The numpad code a press with Num Lock off stands for: a navigation key whose physical key is a digit or the
/// decimal point (the number row sends its digit then).
fn numlock_off(key: egui::Key, physical: Option<egui::Key>) -> Option<String> {
    use egui::Key;
    let nav = matches!(
        key,
        Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::Insert
            | Key::Delete
    );
    if !nav {
        return None;
    }
    let p = physical?;
    let d = match p {
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::Period => "Decimal",
        _ => return None,
    };
    Some(format!("Numpad{d}"))
}

/// Before a frame: name each new key press in `events` from the codes the host reported (in order, by physical
/// key), else from Num Lock off's navigation keys; the codes left over are dropped.
pub fn pair(events: &[egui::Event]) {
    let mut pending = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let mut out: Vec<(egui::Key, Option<String>, bool)> = Vec::new();
    for e in events {
        let egui::Event::Key { key, physical_key, pressed: true, .. } = e else { continue };
        if out.len() >= CAP {
            break;
        }
        let phys = physical_key.or(Some(*key));
        let found = pending.iter().position(|c| code_key(c).is_some_and(|k| Some(k) == phys));
        let code = match found {
            Some(i) => {
                // the codes before it belong to presses egui did not report (keys it has no name for)
                let c = pending.get(i).cloned();
                pending.drain(..=i);
                c
            }
            None => None,
        };
        let code = code.or_else(|| numlock_off(*key, *physical_key));
        out.push((*key, code, false));
    }
    FRAME.with(|f| *f.borrow_mut() = out);
}

/// The code of the next press of `key` this frame (each press is read once); None when the host
/// said nothing and the key does not tell.
pub fn code_of(key: egui::Key) -> Option<String> {
    FRAME.with(|f| {
        let mut f = f.borrow_mut();
        let e = f.iter_mut().find(|(k, _, used)| *k == key && !*used)?;
        e.2 = true;
        e.1.clone()
    })
}

/// Whether `code` is a numpad key's (`Numpad0`…`Numpad9`, `NumpadDecimal`).
pub fn is_numpad(code: &str) -> bool {
    code.strip_prefix("Numpad").is_some_and(|d| d == "Decimal" || (d.len() == 1 && d.bytes().all(|b| b.is_ascii_digit())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: egui::Key, phys: Option<egui::Key>) -> egui::Event {
        egui::Event::Key { key, physical_key: phys, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn host_codes_tell_the_numpad_from_the_row() {
        host_key("Digit7");
        host_key("Numpad7");
        pair(&[press(egui::Key::Num7, Some(egui::Key::Num7)), press(egui::Key::Num7, Some(egui::Key::Num7))]);
        assert_eq!(code_of(egui::Key::Num7).as_deref(), Some("Digit7"));
        assert_eq!(code_of(egui::Key::Num7).as_deref(), Some("Numpad7"));
        assert_eq!(code_of(egui::Key::Num7), None);
        // the next frame starts afresh
        pair(&[press(egui::Key::Num7, Some(egui::Key::Num7))]);
        assert_eq!(code_of(egui::Key::Num7), None);
    }

    #[test]
    fn unreported_codes_are_skipped_and_capped() {
        host_key("ShiftLeft");
        host_key("Numpad1");
        host_key("bad code!");
        pair(&[press(egui::Key::Num1, Some(egui::Key::Num1))]);
        assert_eq!(code_of(egui::Key::Num1).as_deref(), Some("Numpad1"));
        for _ in 0..1000 {
            host_key("Numpad3");
        }
        assert_eq!(PENDING.with(|p| p.borrow().len()), CAP);
        pair(&[]);
        assert_eq!(PENDING.with(|p| p.borrow().len()), 0);
    }

    #[test]
    fn num_lock_off() {
        pair(&[press(egui::Key::Home, Some(egui::Key::Num7)), press(egui::Key::Home, Some(egui::Key::Home))]);
        assert_eq!(code_of(egui::Key::Home).as_deref(), Some("Numpad7"));
        assert_eq!(code_of(egui::Key::Home), None);
        assert!(is_numpad("NumpadDecimal") && is_numpad("Numpad0") && !is_numpad("NumpadEnter") && !is_numpad("Digit1"));
    }
}
