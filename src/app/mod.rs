//! Desktop UI application module.

pub mod state;
pub mod ui;

pub use state::GuiState;
pub use ui::{run_app, setup_custom_fonts, PcTroubleshooterApp};
