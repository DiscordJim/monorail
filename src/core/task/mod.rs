pub mod raw;

pub type Task = Box<dyn FnOnce() + Send + 'static>;

pub(crate) use raw::TaskControlBlockVTable;
pub use raw::{TaskControlBlock, TaskControlHeader};
pub use raw::{TcbInit, TcbFuture, TcbResult};