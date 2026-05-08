mod analyze;
mod audit;
mod diagram;
#[cfg(feature = "vector")]
mod search;
mod suggest;

pub use analyze::cmd_analyze;
pub use audit::cmd_audit;
pub use diagram::cmd_diagram;
#[cfg(all(feature = "vector", feature = "structural"))]
pub use search::cmd_graphrag;
#[cfg(feature = "vector")]
pub use search::{cmd_cluster, cmd_generate_voidignore, cmd_index, cmd_search};
pub use suggest::cmd_suggest;
