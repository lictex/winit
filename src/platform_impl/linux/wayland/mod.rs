#![cfg(wayland_platform)]

//! Winit's Wayland backend.

use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::Proxy;

pub use crate::platform_impl::platform::WindowId;
pub use event_loop::{EventLoop, EventLoopProxy, EventLoopWindowTarget};
pub use output::{MonitorHandle, VideoMode};
pub use window::Window;

mod event_loop;
mod output;
mod seat;
mod state;
mod types;
mod window;

/// Dummy device id, since Wayland doesn't have device events.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId;

impl DeviceId {
    pub const unsafe fn dummy() -> Self {
        DeviceId
    }
}

/// Get the WindowId out of the surface.
#[inline]
fn make_wid(surface: &WlSurface) -> WindowId {
    WindowId(surface.id().as_ptr() as u64)
}

#[derive(Debug)]
pub enum GenericPointer {
    Default(sctk::seat::pointer::ThemedPointer<seat::WinitPointerData>),
    Tablet(crate::platform_impl::wayland::seat::TabletPointer),
}
impl GenericPointer {
    fn set_cursor(
        &self,
        conn: &wayland_client::Connection,
        name: &str,
        shm: &wayland_client::protocol::wl_shm::WlShm,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
        scale: i32,
    ) -> Result<(), sctk::seat::pointer::PointerThemeError> {
        match self {
            GenericPointer::Default(pointer) => pointer.set_cursor(conn, name, shm, surface, scale),
            GenericPointer::Tablet(pointer) => pointer.set_cursor(conn, name, shm, surface, scale),
        }
    }
    fn clear_cursor(&self) {
        match self {
            crate::platform_impl::wayland::GenericPointer::Default(pointer) => {
                pointer
                    .pointer()
                    .set_cursor(self.winit_data().latest_enter_serial(), None, 0, 0);
            }
            crate::platform_impl::wayland::GenericPointer::Tablet(pointer) => {
                pointer
                    .tool()
                    .set_cursor(self.winit_data().latest_enter_serial(), None, 0, 0);
            }
        }
    }
    fn winit_data(&self) -> &seat::WinitPointerData {
        match self {
            GenericPointer::Default(pointer) => {
                seat::WinitPointerDataExt::winit_data(pointer.pointer())
            }
            GenericPointer::Tablet(pointer) => pointer.winit_data(),
        }
    }
}
impl PartialEq for GenericPointer {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Default(a), Self::Default(b)) => a.pointer() == b.pointer(),
            (Self::Tablet(a), Self::Tablet(b)) => a.tool() == b.tool(),
            _ => false,
        }
    }
}
