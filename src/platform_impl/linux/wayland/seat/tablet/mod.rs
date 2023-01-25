//! Tablet handling.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use sctk::reexports::client::protocol::wl_compositor::WlCompositor;
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::protocol::wl_shm::WlShm;
use sctk::reexports::client::{Attached, Main};
use sctk::reexports::protocols::unstable::tablet::v2::client::zwp_tablet_manager_v2::ZwpTabletManagerV2;
use sctk::reexports::protocols::unstable::tablet::v2::client::zwp_tablet_seat_v2::Event as SeatEvent;
use sctk::reexports::protocols::unstable::tablet::v2::client::zwp_tablet_v2::*;

use crate::event::DeviceEvent;
use crate::platform_impl::wayland::event_loop::WinitState;
use crate::platform_impl::wayland::DeviceId;

mod pad;
mod tool;

pub use tool::TabletPointer;

pub struct Tablet(Rc<RefCell<TabletState>>);
impl Tablet {
    pub fn new(
        manager: &ZwpTabletManagerV2,
        compositor: &Attached<WlCompositor>,
        shm: &Attached<WlShm>,
        seat: &Attached<WlSeat>,
    ) -> Self {
        Self(TabletState::new(manager, compositor, shm, seat))
    }
}

struct TabletState {
    seat: WlSeat,
    compositor: Attached<WlCompositor>,
    shm: Attached<WlShm>,
    devices: Vec<Rc<TabletDevice>>,
    tools: Vec<Rc<tool::Tool>>,
    pads: Vec<Rc<pad::Pad>>,
}
impl TabletState {
    pub fn new(
        manager: &ZwpTabletManagerV2,
        compositor: &Attached<WlCompositor>,
        shm: &Attached<WlShm>,
        seat: &Attached<WlSeat>,
    ) -> Rc<RefCell<Self>> {
        let tablet_seat = manager.get_tablet_seat(&seat);
        let state = Rc::new(RefCell::new(Self {
            seat: seat.detach(),
            compositor: compositor.clone(),
            shm: shm.clone(),
            devices: Vec::new(),
            tools: Vec::new(),
            pads: Vec::new(),
        }));

        let weak_state = Rc::downgrade(&state);
        tablet_seat.quick_assign(move |_, event, _| {
            let Some(state) = weak_state.upgrade() else { return };
            match event {
                SeatEvent::TabletAdded { id } => {
                    let device = TabletDevice::new(id, &state);
                    state.borrow_mut().devices.push(device)
                }
                SeatEvent::ToolAdded { id } => {
                    let tool = tool::Tool::new(id, &state);
                    state.borrow_mut().tools.push(tool)
                }
                SeatEvent::PadAdded { id } => {
                    let pad = pad::Pad::new(id, &state);
                    state.borrow_mut().pads.push(pad)
                }
                _ => unreachable!(),
            }
        });
        state
    }
}

struct TabletDevice {
    device: ZwpTabletV2,
    tablet_state: Weak<RefCell<TabletState>>,
}
impl TabletDevice {
    pub fn new(device: Main<ZwpTabletV2>, tablet_state: &Rc<RefCell<TabletState>>) -> Rc<Self> {
        let data = Rc::new(Self {
            device: device.detach(),
            tablet_state: Rc::downgrade(&tablet_state),
        });
        let weak_data = Rc::downgrade(&data);
        device.quick_assign(move |_, event, mut dispatch_data| {
            let Some(data) = weak_data.upgrade() else { return };
            data.handle_event(event, dispatch_data.get::<WinitState>().unwrap());
        });

        data
    }
    fn handle_event(&self, event: Event, winit_state: &mut WinitState) {
        match event {
            Event::Name { .. } => { /* not implemented */ }
            Event::Id { .. } => { /* not implemented */ }
            Event::Path { .. } => { /* not implemented */ }
            Event::Done => winit_state
                .event_sink
                .push_device_event(DeviceEvent::Added, DeviceId),

            Event::Removed => {
                winit_state
                    .event_sink
                    .push_device_event(DeviceEvent::Removed, DeviceId);
                let Some(tablet_state) = self.tablet_state.upgrade() else { return };
                tablet_state
                    .borrow_mut()
                    .devices
                    .retain(|other| other.device != self.device);
            }
            _ => unreachable!(),
        }
    }
}
impl Drop for TabletDevice {
    fn drop(&mut self) {
        self.device.destroy();
    }
}
