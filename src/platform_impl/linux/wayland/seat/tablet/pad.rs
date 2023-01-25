//! Tablet pads handling.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::Main;
use sctk::reexports::protocols::unstable::tablet::v2::client::zwp_tablet_pad_group_v2 as group;
use sctk::reexports::protocols::unstable::tablet::v2::client::zwp_tablet_pad_ring_v2 as ring;
use sctk::reexports::protocols::unstable::tablet::v2::client::zwp_tablet_pad_strip_v2 as strip;
use sctk::reexports::protocols::unstable::tablet::v2::client::zwp_tablet_pad_v2::*;

use crate::event::{DeviceId, ElementState, TabletButton, WindowEvent};
use crate::platform_impl::wayland;
use crate::platform_impl::wayland::event_loop::WinitState;
use crate::platform_impl::wayland::seat::tablet::TabletState;

pub(super) struct Pad {
    pad: ZwpTabletPadV2,
    state: RefCell<PadState>,
    tablet_state: Weak<RefCell<TabletState>>,
}
#[derive(Default)]
struct PadState {
    groups: Vec<Rc<PadGroup>>,
    surface: Option<WlSurface>,
}
impl Pad {
    pub fn new(pad: Main<ZwpTabletPadV2>, tablet_state: &Rc<RefCell<TabletState>>) -> Rc<Self> {
        let data = Rc::new(Self {
            pad: pad.detach(),
            state: Default::default(),
            tablet_state: Rc::downgrade(tablet_state),
        });
        let weak_data = Rc::downgrade(&data);
        pad.quick_assign(move |_, event, mut dispatch_data| {
            let Some(data) = weak_data.upgrade() else { return };
            data.handle_event(event, dispatch_data.get::<WinitState>().unwrap());
        });
        data
    }
    fn handle_event(&self, event: Event, winit_state: &mut WinitState) {
        let state = &mut *self.state.borrow_mut();
        match event {
            Event::Group { pad_group } => state.groups.push(PadGroup::new(pad_group)),
            Event::Path { .. } => { /* not implemented */ }
            Event::Buttons { .. } => { /* not implemented */ }
            Event::Done => {}

            Event::Button {
                button,
                state: button_state,
                ..
            } => {
                let Some(surface) = &state.surface else { return };
                winit_state.event_sink.push_window_event(
                    WindowEvent::TabletButton {
                        device_id: DeviceId(crate::platform_impl::DeviceId::Wayland(
                            wayland::DeviceId,
                        )),
                        button: TabletButton::Tablet(button),
                        state: match button_state {
                            ButtonState::Released => ElementState::Released,
                            ButtonState::Pressed => ElementState::Pressed,
                            _ => unreachable!(),
                        },
                    },
                    wayland::make_wid(&surface),
                );
            }
            Event::Enter { surface, .. } => state.surface = Some(surface),
            Event::Leave { surface, .. } => {
                assert!(Some(surface) == state.surface);
                state.surface = None
            }
            Event::Removed => {
                let Some(tablet_state) = self.tablet_state.upgrade() else { return };
                tablet_state
                    .borrow_mut()
                    .pads
                    .retain(|other| other.pad != self.pad);
            }
            _ => unreachable!(),
        }
    }
}
impl Drop for Pad {
    fn drop(&mut self) {
        self.pad.destroy();
    }
}

struct PadGroup {
    group: group::ZwpTabletPadGroupV2,
    state: RefCell<PadGroupState>,
}
#[derive(Default)]
struct PadGroupState {
    rings: Vec<Rc<PadRing>>,
    strips: Vec<Rc<PadStrip>>,
}
impl PadGroup {
    pub fn new(group: Main<group::ZwpTabletPadGroupV2>) -> Rc<Self> {
        let data = Rc::new(Self {
            group: group.detach(),
            state: Default::default(),
        });
        let weak_data = Rc::downgrade(&data);
        group.quick_assign(move |_, event, mut dispatch_data| {
            let Some(data) = weak_data.upgrade() else { return };
            data.handle_event(event, dispatch_data.get::<WinitState>().unwrap());
        });
        data
    }
    fn handle_event(&self, event: group::Event, _: &mut WinitState) {
        match event {
            group::Event::Buttons { .. } => { /* not implemented */ }
            group::Event::Ring { ring } => self.state.borrow_mut().rings.push(PadRing::new(ring)),
            group::Event::Strip { strip } => {
                self.state.borrow_mut().strips.push(PadStrip::new(strip))
            }
            group::Event::Modes { .. } => { /* not implemented */ }
            group::Event::Done => {}

            group::Event::ModeSwitch { .. } => { /* not implemented */ }
            _ => unreachable!(),
        }
    }
}
impl Drop for PadGroup {
    fn drop(&mut self) {
        self.group.destroy();
    }
}

struct PadRing {
    ring: ring::ZwpTabletPadRingV2,
}
impl PadRing {
    pub fn new(ring: Main<ring::ZwpTabletPadRingV2>) -> Rc<Self> {
        let data = Rc::new(Self {
            ring: ring.detach(),
        });
        let weak_data = Rc::downgrade(&data);
        ring.quick_assign(move |_, event, mut dispatch_data| {
            let Some(data) = weak_data.upgrade() else { return };
            data.handle_event(event, dispatch_data.get::<WinitState>().unwrap());
        });
        data
    }
    fn handle_event(&self, _: ring::Event, _: &mut WinitState) {
        /* not implemented */
    }
}
impl Drop for PadRing {
    fn drop(&mut self) {
        self.ring.destroy();
    }
}

struct PadStrip {
    strip: strip::ZwpTabletPadStripV2,
}
impl PadStrip {
    pub fn new(strip: Main<strip::ZwpTabletPadStripV2>) -> Rc<Self> {
        let data = Rc::new(Self {
            strip: strip.detach(),
        });
        let weak_data = Rc::downgrade(&data);
        strip.quick_assign(move |_, event, mut dispatch_data| {
            let Some(data) = weak_data.upgrade() else { return };
            data.handle_event(event, dispatch_data.get::<WinitState>().unwrap());
        });
        data
    }
    fn handle_event(&self, _: strip::Event, _: &mut WinitState) {
        /* not implemented */
    }
}
impl Drop for PadStrip {
    fn drop(&mut self) {
        self.strip.destroy();
    }
}
