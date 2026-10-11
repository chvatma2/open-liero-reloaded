//! Navigation (notes §1.4, §1.5, §4.3): rock-free areas shared through
//! [`AreaCache`], the resumable [`Search`] over them, and the [`Follower`]
//! that picks the node to head for. Paths ignore dirt, as OLX's do: the bot
//! digs through it.

pub mod areas;
mod checks;
pub mod follow;
mod path;
pub mod search;
mod trace;

pub use areas::{AreaCache, Grower, Rect, WORM_SIZE, rock_hash};
pub use follow::{Follower, LOOKAHEAD, Step, jump_loses_node, jump_rise};
pub use path::SPLIT_DIST;
pub use search::{MAX_PATH_NODES, MAX_SEARCH_AREAS, Search, SearchError, SearchStatus};
pub(crate) use trace::{fast_trace_line, fast_worst};
pub use trace::{trace_worm_line, worm_line_clear};
