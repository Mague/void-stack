mod analyze;
mod audit;
mod diagram;
#[cfg(feature = "vector")]
mod search;
mod suggest;

pub use analyze::cmd_analyze;
pub use audit::cmd_audit;
pub use diagram::{cmd_diagram, cmd_graph_html};
#[cfg(feature = "structural")]
pub use search::cmd_graph_build;
#[cfg(feature = "vector")]
pub use search::{cmd_cluster, cmd_generate_voidignore, cmd_index, cmd_search};
#[cfg(all(feature = "vector", feature = "structural"))]
pub use search::{cmd_graphrag, cmd_graphrag_cross};
pub use suggest::cmd_suggest;
