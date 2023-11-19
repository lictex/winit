//! Tablet handling.

use sctk::globals::GlobalData;
use sctk::reexports::client::backend::ObjectId;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::protocol::wl_shm::WlShm;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::Proxy;
use sctk::reexports::client::{delegate_dispatch, Dispatch};
use sctk::reexports::client::{Connection, QueueHandle};
use sctk::reexports::protocols::wp::tablet::zv2::client::zwp_tablet_manager_v2::ZwpTabletManagerV2;
use sctk::reexports::protocols::wp::tablet::zv2::client::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2;
use sctk::reexports::protocols::wp::tablet::zv2::client::zwp_tablet_pad_v2::{
    self, ZwpTabletPadV2,
};
use sctk::reexports::protocols::wp::tablet::zv2::client::zwp_tablet_seat_v2::{
    self, ZwpTabletSeatV2,
};
use sctk::reexports::protocols::wp::tablet::zv2::client::zwp_tablet_tool_v2::{
    self, ZwpTabletToolV2,
};
use sctk::reexports::protocols::wp::tablet::zv2::client::zwp_tablet_v2::{self, ZwpTabletV2};

use crate::dpi::PhysicalPosition;
use crate::event::{DeviceEvent, WindowEvent};
use crate::platform_impl::wayland::state::WinitState;
use crate::platform_impl::wayland::{make_wid, DeviceId};

#[derive(Debug)]
pub struct TabletState {
    pub manager: ZwpTabletManagerV2,
    pub seats: Vec<(ZwpTabletSeatV2, WlSeat)>,
    pub tablets: Vec<ZwpTabletV2>,
    pub pads: ahash::AHashMap<ObjectId, PadData>,
    pub tools: ahash::AHashMap<ObjectId, ToolData>,
}
impl TabletState {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind::<ZwpTabletManagerV2, _, _>(queue_handle, 1..=1, GlobalData)?;
        Ok(Self {
            manager,
            seats: Default::default(),
            tablets: Default::default(),
            pads: Default::default(),
            tools: Default::default(),
        })
    }
    pub fn new_seat(&mut self, queue_handle: &QueueHandle<WinitState>, seat: WlSeat) {
        let tablet_seat = self
            .manager
            .get_tablet_seat(&seat, queue_handle, GlobalData);
        self.seats.push((tablet_seat, seat));
    }
}

#[derive(Debug, Default)]
pub struct PadData {
    surfaces: Vec<WlSurface>,
}

#[derive(Debug)]
pub struct ToolData {
    pointer: std::sync::Arc<crate::platform_impl::wayland::GenericPointer>,
    eraser: bool,
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

impl Dispatch<ZwpTabletManagerV2, GlobalData, WinitState> for TabletState {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZwpTabletManagerV2,
        _event: <ZwpTabletManagerV2 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
    }
}

#[derive(Debug)]
pub struct TabletPointer {
    tool: ZwpTabletToolV2,
    inner: super::WinitPointerData,
    themed_pointer: sctk::seat::pointer::ThemedPointer<super::WinitPointerData>,
    shm: WlShm,
}
impl TabletPointer {
    pub fn set_cursor(
        &self,
        conn: &Connection,
        icon: cursor_icon::CursorIcon,
    ) -> Result<(), sctk::seat::pointer::PointerThemeError> {
        use sctk::compositor::SurfaceDataExt;
        let scale = self
            .themed_pointer
            .surface()
            .data::<sctk::compositor::SurfaceData>()
            .unwrap()
            .surface_data()
            .scale_factor();
        let name = std::env::var("XCURSOR_THEME")
            .ok()
            .unwrap_or_else(|| "default".into());
        let size = std::env::var("XCURSOR_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24);
        let mut theme =
            wayland_cursor::CursorTheme::load_from_name(conn, self.shm.clone(), &name, size)
                .map_err(sctk::seat::pointer::PointerThemeError::InvalidId)?;
        let cursor = theme
            .get_cursor(icon.name())
            .ok_or(sctk::seat::pointer::PointerThemeError::CursorNotFound)?;
        let image = &cursor[0];
        let (w, h) = image.dimensions();
        let (hx, hy) = image.hotspot();
        self.themed_pointer.surface().set_buffer_scale(scale as i32);
        self.themed_pointer.surface().attach(Some(image), 0, 0);
        if self.themed_pointer.surface().version() >= 4 {
            self.themed_pointer
                .surface()
                .damage_buffer(0, 0, w as i32, h as i32);
        } else {
            // surface is old and does not support damage_buffer, so we damage
            // in surface coordinates and hope it is not rescaled
            self.themed_pointer.surface().damage(
                0,
                0,
                w as i32 / scale as i32,
                h as i32 / scale as i32,
            );
        }
        self.themed_pointer.surface().commit();
        self.tool.set_cursor(
            self.inner.latest_enter_serial(),
            Some(self.themed_pointer.surface()),
            hx as i32 / scale as i32,
            hy as i32 / scale as i32,
        );
        Ok(())
    }
    pub fn winit_data(&self) -> &super::WinitPointerData {
        &self.inner
    }
    pub fn tool(&self) -> &ZwpTabletToolV2 {
        &self.tool
    }
}

impl Dispatch<ZwpTabletSeatV2, GlobalData, WinitState> for TabletState {
    wayland_client::event_created_child!(WinitState, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, GlobalData),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, GlobalData),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, GlobalData),
    ]);
    fn event(
        state: &mut WinitState,
        proxy: &ZwpTabletSeatV2,
        event: <ZwpTabletSeatV2 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        qhandle: &QueueHandle<WinitState>,
    ) {
        let Some(tablet) = &mut state.tablet else {
            return;
        };
        match event {
            zwp_tablet_seat_v2::Event::TabletAdded { id } => tablet.tablets.push(id),
            zwp_tablet_seat_v2::Event::ToolAdded { id } => {
                let seat = tablet
                    .seats
                    .iter()
                    .find(|f| &f.0 == proxy)
                    .unwrap()
                    .1
                    .clone();
                let cursor_surface = state.compositor_state.create_surface(qhandle);

                // let surface_id = cursor_surface.id();
                let pointer_data = super::WinitPointerData::new(seat.clone());
                let themed_pointer = state
                    .seat_state
                    .get_pointer_with_theme_and_data(
                        qhandle,
                        &seat,
                        state.shm.wl_shm(),
                        cursor_surface,
                        super::ThemeSpec::System,
                        pointer_data,
                    )
                    .expect("failed to create pointer with present capability.");

                tablet.tools.insert(
                    id.id(),
                    ToolData {
                        pointer: crate::platform_impl::wayland::GenericPointer::Tablet(
                            TabletPointer {
                                tool: id,
                                inner: super::WinitPointerData::new(seat),
                                themed_pointer,
                                shm: state.shm.wl_shm().clone(),
                            },
                        )
                        .into(),
                        eraser: Default::default(),
                        surface: Default::default(),
                        contact: Default::default(),
                        x: Default::default(),
                        y: Default::default(),
                        prerssure: Default::default(),
                        distance: Default::default(),
                        tilt_x: Default::default(),
                        tilt_y: Default::default(),
                        rotation: Default::default(),
                    },
                );
            }
            zwp_tablet_seat_v2::Event::PadAdded { id } => {
                tablet.pads.insert(id.id(), PadData::default());
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwpTabletV2, GlobalData, WinitState> for TabletState {
    fn event(
        state: &mut WinitState,
        proxy: &ZwpTabletV2,
        event: <ZwpTabletV2 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        match event {
            zwp_tablet_v2::Event::Name { .. } => { /* not implemented */ }
            zwp_tablet_v2::Event::Id { .. } => { /* not implemented */ }
            zwp_tablet_v2::Event::Path { .. } => { /* not implemented */ }
            zwp_tablet_v2::Event::Done => state
                .events_sink
                .push_device_event(DeviceEvent::Added, DeviceId),

            zwp_tablet_v2::Event::Removed => {
                state
                    .events_sink
                    .push_device_event(DeviceEvent::Removed, DeviceId);
                if let Some(tablet) = &mut state.tablet {
                    tablet.tablets.retain(|other| other != proxy);
                }
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwpTabletPadV2, GlobalData, WinitState> for TabletState {
    wayland_client::event_created_child!(WinitState, ZwpTabletSeatV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, GlobalData),
    ]);
    fn event(
        state: &mut WinitState,
        proxy: &ZwpTabletPadV2,
        event: <ZwpTabletPadV2 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        let Some(tablet) = &mut state.tablet else {
            return;
        };
        match event {
            zwp_tablet_pad_v2::Event::Group { .. } => { /* not implemented */ }
            zwp_tablet_pad_v2::Event::Path { .. } => { /* not implemented */ }
            zwp_tablet_pad_v2::Event::Buttons { .. } => { /* not implemented */ }
            zwp_tablet_pad_v2::Event::Done => {}

            zwp_tablet_pad_v2::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                for surface in &tablet.pads.get(&proxy.id()).unwrap().surfaces {
                    state.events_sink.push_window_event(
                        WindowEvent::TabletButton {
                            device_id: crate::event::DeviceId(
                                crate::platform_impl::DeviceId::Wayland(DeviceId),
                            ),
                            button: crate::event::TabletButton::Tablet(button),
                            state: match button_state.into_result() {
                                Ok(zwp_tablet_pad_v2::ButtonState::Released) => {
                                    crate::event::ElementState::Released
                                }
                                Ok(zwp_tablet_pad_v2::ButtonState::Pressed) => {
                                    crate::event::ElementState::Pressed
                                }
                                _ => unreachable!(),
                            },
                        },
                        make_wid(surface),
                    );
                }
            }
            zwp_tablet_pad_v2::Event::Enter { surface, .. } => {
                tablet
                    .pads
                    .get_mut(&proxy.id())
                    .unwrap()
                    .surfaces
                    .push(surface);
            }
            zwp_tablet_pad_v2::Event::Leave { surface, .. } => {
                tablet
                    .pads
                    .get_mut(&proxy.id())
                    .unwrap()
                    .surfaces
                    .retain(|other| other != &surface);
            }
            zwp_tablet_pad_v2::Event::Removed => {
                tablet.pads.remove(&proxy.id());
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwpTabletPadGroupV2, GlobalData, WinitState> for TabletState {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZwpTabletPadGroupV2,
        _event: <ZwpTabletPadGroupV2 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        /* not implemented */
    }
}

impl Dispatch<ZwpTabletToolV2, GlobalData, WinitState> for TabletState {
    fn event(
        state: &mut WinitState,
        proxy: &ZwpTabletToolV2,
        event: <ZwpTabletToolV2 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        let Some(tablet) = &mut state.tablet else {
            return;
        };
        let tool = &mut tablet.tools.get_mut(&proxy.id()).unwrap();

        match event {
            zwp_tablet_tool_v2::Event::Type { tool_type } => {
                tool.eraser = matches!(
                    tool_type.into_result(),
                    Ok(zwp_tablet_tool_v2::Type::Eraser)
                )
            }
            zwp_tablet_tool_v2::Event::HardwareSerial { .. } => { /* not implemented */ }
            zwp_tablet_tool_v2::Event::HardwareIdWacom { .. } => { /* not implemented */ }
            zwp_tablet_tool_v2::Event::Capability { .. } => { /* not implemented */ }
            zwp_tablet_tool_v2::Event::Done => {}

            zwp_tablet_tool_v2::Event::Removed => {
                tablet.tools.remove(&proxy.id());
            }
            zwp_tablet_tool_v2::Event::ProximityIn { surface, .. } => {
                let window_id = make_wid(&surface);
                tool.surface = Some(surface.clone());
                state.events_sink.push_window_event(
                    WindowEvent::TabletPenEnter {
                        device_id: crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(
                            DeviceId,
                        )),
                        inverted: tool.eraser,
                    },
                    window_id,
                );

                let Some(window) = state.windows.borrow().get(&window_id).cloned() else {
                    return;
                };
                let mut window = window.lock().unwrap();
                window.pointer_entered(std::sync::Arc::downgrade(&tool.pointer));
            }
            zwp_tablet_tool_v2::Event::ProximityOut => {
                let Some(surface) = &tool.surface else { return };
                let window_id = make_wid(&surface);
                tool.surface = None;
                state.events_sink.push_window_event(
                    WindowEvent::TabletPenLeave {
                        device_id: crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(
                            DeviceId,
                        )),
                    },
                    window_id,
                );

                let Some(window) = state.windows.borrow().get(&window_id).cloned() else {
                    return;
                };
                let mut window = window.lock().unwrap();
                window.pointer_left(std::sync::Arc::downgrade(&tool.pointer));
            }
            zwp_tablet_tool_v2::Event::Down { .. } => {
                let Some(surface) = &tool.surface else { return };
                tool.contact = true;
                state.events_sink.push_window_event(
                    WindowEvent::TabletButton {
                        device_id: crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(
                            DeviceId,
                        )),
                        button: match tool.eraser {
                            true => crate::event::TabletButton::Eraser,
                            false => crate::event::TabletButton::Tip,
                        },
                        state: crate::event::ElementState::Pressed,
                    },
                    make_wid(&surface),
                );
            }
            zwp_tablet_tool_v2::Event::Up => {
                let Some(surface) = &tool.surface else { return };
                tool.contact = false;
                state.events_sink.push_window_event(
                    WindowEvent::TabletButton {
                        device_id: crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(
                            DeviceId,
                        )),
                        button: match tool.eraser {
                            true => crate::event::TabletButton::Eraser,
                            false => crate::event::TabletButton::Tip,
                        },
                        state: crate::event::ElementState::Released,
                    },
                    make_wid(&surface),
                );
            }
            zwp_tablet_tool_v2::Event::Motion { x, y } => [tool.x, tool.y] = [x, y],
            zwp_tablet_tool_v2::Event::Pressure { pressure } => {
                tool.prerssure = pressure as f64 / 65535.0
            }
            zwp_tablet_tool_v2::Event::Distance { distance } => {
                tool.distance = distance as f64 / 65535.0
            }
            zwp_tablet_tool_v2::Event::Tilt { tilt_x, tilt_y } => {
                [tool.tilt_x, tool.tilt_y] = [tilt_x, tilt_y]
            }
            zwp_tablet_tool_v2::Event::Rotation { degrees } => tool.rotation = degrees,
            zwp_tablet_tool_v2::Event::Slider { .. } => { /* not implemented */ }
            zwp_tablet_tool_v2::Event::Wheel { .. } => { /* not implemented */ }
            zwp_tablet_tool_v2::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                let Some(surface) = &tool.surface else { return };

                // https://github.com/torvalds/linux/blob/0015edd6f66172f93aa720192020138ca13ba0a6/include/uapi/linux/input-event-codes.h#L413
                const BTN_STYLUS: u32 = 0x14b;
                const BTN_STYLUS2: u32 = 0x14c;
                let button = match button {
                    BTN_STYLUS => 0,
                    BTN_STYLUS2 => 1,
                    e => return println!("Unknown tool button: {e}"),
                };

                state.events_sink.push_window_event(
                    WindowEvent::TabletButton {
                        device_id: crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(
                            DeviceId,
                        )),
                        button: crate::event::TabletButton::Pen(button),
                        state: match button_state.into_result() {
                            Ok(zwp_tablet_tool_v2::ButtonState::Released) => {
                                crate::event::ElementState::Released
                            }
                            Ok(zwp_tablet_tool_v2::ButtonState::Pressed) => {
                                crate::event::ElementState::Pressed
                            }
                            _ => unreachable!(),
                        },
                    },
                    make_wid(&surface),
                );
            }
            zwp_tablet_tool_v2::Event::Frame { .. } => {
                let ToolData {
                    pointer: _,
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
                } = &tool;
                let Some(surface) = surface else { return };
                state.events_sink.push_window_event(
                    WindowEvent::TabletPenMotion {
                        device_id: crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(
                            DeviceId,
                        )),
                        location: PhysicalPosition::new(*x, *y),
                        pressure: *prerssure,
                        rotation: *rotation,
                        distance: *distance,
                        tilt: [*tilt_x, *tilt_y],
                    },
                    make_wid(&surface),
                );
            }
            _ => unreachable!(),
        }
    }
}

delegate_dispatch!(WinitState: [ZwpTabletManagerV2: GlobalData] => TabletState);
delegate_dispatch!(WinitState: [ZwpTabletSeatV2: GlobalData] => TabletState);
delegate_dispatch!(WinitState: [ZwpTabletV2: GlobalData] => TabletState);
delegate_dispatch!(WinitState: [ZwpTabletPadV2: GlobalData] => TabletState);
delegate_dispatch!(WinitState: [ZwpTabletPadGroupV2: GlobalData] => TabletState);
delegate_dispatch!(WinitState: [ZwpTabletToolV2: GlobalData] => TabletState);
