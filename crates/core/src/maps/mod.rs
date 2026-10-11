pub mod install;
pub mod status;

pub use install::{install_map, InstallError, InstallOutcome};
pub use status::{map_status, MapState, MapStatus};
