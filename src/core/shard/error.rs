use crate::core::shard::state::ShardId;




#[derive(thiserror::Error, Debug)]
pub enum ShardError {

    #[error("Failed to terminate core setup. Configuration channel closed on shard {0:?}.")]
    ConfigFailure(ShardId),
    #[error("Failed to bind to core: {0:?}")]
    CoreBindFailure(#[from] nix::Error)


}