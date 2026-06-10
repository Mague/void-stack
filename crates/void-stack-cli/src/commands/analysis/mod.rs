mod analyze;
mod audit;
mod diagram;
#[cfg(feature = "vector")]
mod search;
mod suggest;

pub use analyze::cmd_analyze;
pub use audit::cmd_audit;
pub use diagram::{cmd_diagram, cmd_graph_html};
#[cfg(feature = "vector")]
pub use search::{cmd_cluster, cmd_generate_voidignore, cmd_index, cmd_search};
#[cfg(feature = "structural")]
pub use search::{cmd_graph_build, cmd_review, cmd_suggest_tests};
#[cfg(all(feature = "vector", feature = "structural"))]
pub use search::{cmd_graphrag, cmd_graphrag_cross};
pub use suggest::cmd_suggest;
