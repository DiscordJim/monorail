pub mod core;

mod monolib {
    pub use crate::core::shard::shard::{submit_to, shard_id, call_on, get_topology_info, call_on_local};
}