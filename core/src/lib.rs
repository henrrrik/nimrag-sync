//! nimrag-sync's core: talking to inReach devices and to Garmin's inReach
//! service, without any user interface. The Qt window links it as a static
//! library through `ffi`, and the command line lives in `cli`.

pub mod cli;
pub mod credentials;
pub mod csv;
pub mod device;
pub mod ffi;
pub mod frame;
pub mod places;
pub mod plan;
pub mod server;
pub mod status;
pub mod sync;
