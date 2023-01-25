//! Tablet tools handling.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use sctk::reexports::client::protocol::wl_shm::WlShm;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Attached, Main};
use sctk::reexports::protocols::unstable::tablet::v2::client::zwp_tablet_tool_v2::*;

use crate::dpi::PhysicalPosition;
use crate::event::{DeviceId, ElementState, TabletButton, WindowEvent};
use crate::platform_impl::wayland;
use crate::platform_impl::wayland::event_loop::WinitState;
use crate::platform_impl::wayland::seat::pointer::{PointerType, WinitPointer};
use crate::platform_impl::wayland::seat::tablet::TabletState;

pub struct TabletPointer {
    tool: ZwpTabletToolV2,
    cursor_surface: WlSurface,
    shm: Attached<WlShm>,
}
impl PartialEq for TabletPointer {
    fn eq(&self, other: &Self) -> bool {
        self.tool == other.tool
    }
}
impl Deref for TabletPointer {
    type Target = ZwpTabletToolV2;
    fn deref(&self) -> &Self::Target {
        &self.tool
    }
}
impl TabletPointer {
    // copied from sctk/seat/pointer/theme.rs
    pub fn set_cursor(&self, cursor_name: &str, enter_serial: u32) -> bool {
        let name = std::env::var("XCURSOR_THEME")
            .ok()
            .unwrap_or_else(|| "default".into());
        let size = std::env::var("XCURSOR_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24);
        let scale = 1; // todo: ?
        let mut theme = wayland_cursor::CursorTheme::load_from_name(&name, size, &self.shm);
        let Some(cursor) = theme.get_cursor(cursor_name) else { return false };
        let image = &cursor[0];
        let (w, h) = image.dimensions();
        let (hx, hy) = image.hotspot();
        self.cursor_surface.set_buffer_scale(scale as i32);
        self.cursor_surface.attach(Some(image), 0, 0);
        if self.cursor_surface.as_ref().version() >= 4 {
            self.cursor_surface.damage_buffer(0, 0, w as i32, h as i32);
        } else {
            // surface is old and does not support damage_buffer, so we damage
            // in surface coordinates and hope it is not rescaled
            self.cursor_surface
                .damage(0, 0, w as i32 / scale as i32, h as i32 / scale as i32);
        }
        self.cursor_surface.commit();
        self.tool.set_cursor(
            enter_serial,
            Some(&self.cursor_surface),
            hx as i32 / scale as i32,
            hy as i32 / scale as i32,
        );
        true
    }
}

pub(super) struct Tool {
    tool: ZwpTabletToolV2,
    cursor_surface: WlSurface,
    state: RefCell<ToolState>,
    tablet_state: Weak<RefCell<TabletState>>,
}
#[derive(Default)]
struct ToolState {
    eraser: bool,
    latest_serial: Rc<Cell<u32>>,
    enter_serial: Rc<Cell<u32>>,
    surface: Option<WlSurface>,
    contact: bool,
    x: f64,
    y: f64,
    prerssure: f64,
    distance: f64,
    tilt_x: f64,
    tilt_y: f64,
    /// in degrees
    rotation: f64,
}
impl Tool {
    pub fn new(tool: Main<ZwpTabletToolV2>, tablet_state: &Rc<RefCell<TabletState>>) -> Rc<Self> {
        let surface = tablet_state.borrow().compositor.create_surface();
        surface.quick_assign(move |_, _, _| {}); // todo: ?
        let data = Rc::new(Self {
            tool: tool.detach(),
            cursor_surface: surface.detach(),
            state: Default::default(),
            tablet_state: Rc::downgrade(tablet_state),
        });
        let weak_data = Rc::downgrade(&data);
        tool.quick_assign(move |_, event, mut dispatch_data| {
            let Some(data) = weak_data.upgrade() else { return };
            data.handle_event(event, dispatch_data.get::<WinitState>().unwrap());
        });
        data
    }
    fn handle_event(&self, event: Event, winit_state: &mut WinitState) {
        let state = &mut *self.state.borrow_mut();
        match event {
            Event::Type { tool_type } => state.eraser = matches!(tool_type, Type::Eraser),
            Event::HardwareSerial { .. } => { /* not implemented */ }
            Event::HardwareIdWacom { .. } => { /* not implemented */ }
            Event::Capability { .. } => { /* not implemented */ }
            Event::Done => {}

            Event::Removed => {
                let Some(tablet_state) = self.tablet_state.upgrade() else { return };
                tablet_state
                    .borrow_mut()
                    .tools
                    .retain(|other| other.tool != self.tool);
            }
            Event::ProximityIn {
                surface, serial, ..
            } => {
                let window_id = wayland::make_wid(&surface);
                state.surface = Some(surface);
                state.enter_serial.set(serial);
                state.latest_serial.set(serial);
                winit_state.event_sink.push_window_event(
                    WindowEvent::TabletPenEnter {
                        device_id: DeviceId(crate::platform_impl::DeviceId::Wayland(
                            wayland::DeviceId,
                        )),
                        inverted: state.eraser,
                    },
                    window_id,
                );

                let Some(window) = winit_state.window_map.get_mut(&window_id) else { return };
                let Some(tablet_state) = self.tablet_state.upgrade() else { return };
                window.pointer_entered(WinitPointer {
                    pointer: PointerType::Tablet(TabletPointer {
                        tool: self.tool.clone(),
                        cursor_surface: self.cursor_surface.clone(),
                        shm: tablet_state.borrow().shm.clone(),
                    }),
                    latest_serial: state.latest_serial.clone(),
                    latest_enter_serial: state.enter_serial.clone(),
                    seat: tablet_state.borrow().seat.clone(),
                    // these doesnt matter
                    pointer_constraints: Default::default(),
                    confined_pointer: Default::default(),
                    locked_pointer: Default::default(),
                });
            }
            Event::ProximityOut => {
                let Some(surface) = &state.surface else { return };
                let window_id = wayland::make_wid(&surface);
                state.surface = None;
                winit_state.event_sink.push_window_event(
                    WindowEvent::TabletPenLeave {
                        device_id: DeviceId(crate::platform_impl::DeviceId::Wayland(
                            wayland::DeviceId,
                        )),
                    },
                    window_id,
                );

                let Some(window) = winit_state.window_map.get_mut(&window_id) else { return };
                let Some(tablet_state) = self.tablet_state.upgrade() else { return };
                window.pointer_left(WinitPointer {
                    pointer: PointerType::Tablet(TabletPointer {
                        tool: self.tool.clone(),
                        cursor_surface: self.cursor_surface.clone(),
                        shm: tablet_state.borrow().shm.clone(),
                    }),
                    seat: tablet_state.borrow().seat.clone(),
                    // these doesnt matter
                    pointer_constraints: Default::default(),
                    confined_pointer: Default::default(),
                    locked_pointer: Default::default(),
                    latest_serial: Default::default(),
                    latest_enter_serial: Default::default(),
                });
            }
            Event::Down { serial } => {
                let Some(surface) = &state.surface else { return };
                state.contact = true;
                state.latest_serial.set(serial);
                winit_state.event_sink.push_window_event(
                    WindowEvent::TabletButton {
                        device_id: DeviceId(crate::platform_impl::DeviceId::Wayland(
                            wayland::DeviceId,
                        )),
                        button: match state.eraser {
                            true => TabletButton::Eraser,
                            false => TabletButton::Tip,
                        },
                        state: ElementState::Pressed,
                    },
                    wayland::make_wid(&surface),
                );
            }
            Event::Up => {
                let Some(surface) = &state.surface else { return };
                state.contact = false;
                winit_state.event_sink.push_window_event(
                    WindowEvent::TabletButton {
                        device_id: DeviceId(crate::platform_impl::DeviceId::Wayland(
                            wayland::DeviceId,
                        )),
                        button: match state.eraser {
                            true => TabletButton::Eraser,
                            false => TabletButton::Tip,
                        },
                        state: ElementState::Released,
                    },
                    wayland::make_wid(&surface),
                );
            }
            Event::Motion { x, y } => [state.x, state.y] = [x, y],
            Event::Pressure { pressure } => state.prerssure = pressure as f64 / 65535.0,
            Event::Distance { distance } => state.distance = distance as f64 / 65535.0,
            Event::Tilt { tilt_x, tilt_y } => [state.tilt_x, state.tilt_y] = [tilt_x, tilt_y],
            Event::Rotation { degrees } => state.rotation = degrees,
            Event::Slider { .. } => { /* not implemented */ }
            Event::Wheel { .. } => { /* not implemented */ }
            Event::Button {
                button,
                state: button_state,
                serial,
                ..
            } => {
                let Some(surface) = &state.surface else { return };
                state.latest_serial.set(serial);

                // https://github.com/torvalds/linux/blob/0015edd6f66172f93aa720192020138ca13ba0a6/include/uapi/linux/input-event-codes.h#L413
                const BTN_STYLUS: u32 = 0x14b;
                const BTN_STYLUS2: u32 = 0x14c;
                let button = match button {
                    BTN_STYLUS => 0,
                    BTN_STYLUS2 => 1,
                    e => return println!("Unknown tool button: {e}"),
                };

                winit_state.event_sink.push_window_event(
                    WindowEvent::TabletButton {
                        device_id: DeviceId(crate::platform_impl::DeviceId::Wayland(
                            wayland::DeviceId,
                        )),
                        button: TabletButton::Pen(button),
                        state: match button_state {
                            ButtonState::Released => ElementState::Released,
                            ButtonState::Pressed => ElementState::Pressed,
                            _ => unreachable!(),
                        },
                    },
                    wayland::make_wid(&surface),
                );
            }
            Event::Frame { .. } => {
                let ToolState {
                    latest_serial: _,
                    enter_serial: _,
                    eraser: _,
                    contact: _,
                    surface,
                    x,
                    y,
                    prerssure,
                    distance,
                    tilt_x,
                    tilt_y,
                    rotation,
                } = &state;
                let Some(surface) = surface else { return };
                winit_state.event_sink.push_window_event(
                    WindowEvent::TabletPenMotion {
                        device_id: DeviceId(crate::platform_impl::DeviceId::Wayland(
                            wayland::DeviceId,
                        )),
                        location: PhysicalPosition::new(*x, *y),
                        pressure: *prerssure,
                        rotation: *rotation,
                        distance: *distance,
                        tilt: [*tilt_x, *tilt_y],
                    },
                    wayland::make_wid(&surface),
                );
            }
            _ => unreachable!(),
        }
    }
}
impl Drop for Tool {
    fn drop(&mut self) {
        self.tool.destroy();
        self.cursor_surface.destroy();
    }
}
