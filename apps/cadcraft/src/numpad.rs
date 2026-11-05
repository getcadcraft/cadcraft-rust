//! The desktop's key codes for the window (cc_ui_egui::keycode): egui-winit names the numpad's digits as the number
//! row's and drops winit's key location, so ModelCraft's modeler keys could not tell Numpad7 from 7 with Num Lock on.
//! eframe runs on our own winit event loop here ([`eframe::create_native`]) and every event goes on to it unchanged;
//! a key press on the numpad (`KeyLocation::Numpad`) is reported first by its code as the browser names it
//! (`KeyboardEvent.code`: winit's `KeyCode` names are the W3C ones, "Numpad7", "NumpadDecimal").

use eframe::UserEvent;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, StartCause, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyLocation, PhysicalKey};
use winit::window::WindowId;

/// eframe's application, with the numpad's key codes reported on the way.
pub struct KeyCodes<'a>(pub eframe::EframeWinitApplication<'a>);

/// The code of a numpad key press, as `KeyboardEvent.code` names it; None for any other key or a release.
pub fn numpad_code(e: &winit::event::KeyEvent) -> Option<String> {
    if e.state != ElementState::Pressed || e.location != KeyLocation::Numpad {
        return None;
    }
    match e.physical_key {
        PhysicalKey::Code(c) => Some(format!("{c:?}")),
        PhysicalKey::Unidentified(_) => None,
    }
}

impl ApplicationHandler<UserEvent> for KeyCodes<'_> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.0.resumed(el);
    }
    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let WindowEvent::KeyboardInput { event: k, .. } = &event
            && let Some(code) = numpad_code(k)
        {
            cc_ui_egui::keycode::host_key(&code);
        }
        self.0.window_event(el, id, event);
    }
    fn new_events(&mut self, el: &ActiveEventLoop, cause: StartCause) {
        self.0.new_events(el, cause);
    }
    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        self.0.user_event(el, event);
    }
    fn device_event(&mut self, el: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.0.device_event(el, id, event);
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.0.about_to_wait(el);
    }
    fn suspended(&mut self, el: &ActiveEventLoop) {
        self.0.suspended(el);
    }
    fn exiting(&mut self, el: &ActiveEventLoop) {
        self.0.exiting(el);
    }
    fn memory_warning(&mut self, el: &ActiveEventLoop) {
        self.0.memory_warning(el);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn winit_names_are_the_browsers() {
        use winit::keyboard::KeyCode;
        assert_eq!(format!("{:?}", KeyCode::Numpad7), "Numpad7");
        assert_eq!(format!("{:?}", KeyCode::NumpadDecimal), "NumpadDecimal");
        assert_eq!(format!("{:?}", KeyCode::Digit7), "Digit7");
        // the names keycode.rs pairs by: the same physical key egui reports
        assert_eq!(egui::Key::from_name(&format!("{:?}", KeyCode::Numpad3)), Some(egui::Key::Num3));
    }
}
