mod about;
mod debug;
mod desktop_visuals;
mod devices;
mod frameconnect;
mod logs;
mod new_version_popup;
mod notifications;
mod settings;
mod settings_controls;
mod setup_wizard;
mod statistics;

#[cfg(not(target_arch = "wasm32"))]
mod installation;

pub use about::*;
pub use debug::*;
pub use desktop_visuals::*;
pub use devices::*;
pub use frameconnect::*;
pub use logs::*;
pub use new_version_popup::*;
pub use notifications::*;
pub use settings::*;
pub use settings_controls::*;
pub use setup_wizard::*;
pub use statistics::*;

#[cfg(not(target_arch = "wasm32"))]
pub use installation::*;
