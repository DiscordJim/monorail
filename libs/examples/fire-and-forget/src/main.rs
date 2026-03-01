use monorail::core::monolib;
use monorail::core::shard::state::ShardId;
use std::time::Duration;
use monorail::core::topology::MonorailTopology;
use monorail::core::topology::MonorailConfiguration;
use monorail::core::shard::shard::signal_monorail;
/// Submit tasks without waiting for results
/// Demonstrates fire-and-forget task submission
#[monorail::main]
async fn main() {
    let topology = monolib::get_topology_info();
    
    // Submit fire-and-forget tasks to all shards
    for i in 0..topology.cores {
        let shard_id = ShardId::new(i);
        monolib::submit_to(shard_id, move || async move {
            let fib = compute_fibonacci(15 + i as u32);
            println!("Shard {}: Computed fib({}) = {} (fire-and-forget)", 
                monolib::shard_id().as_usize(),
                15 + i as u32,
                fib
            );
        });
    }
    
    // Give tasks time to complete
    println!("Waiting for fire-and-forget tasks to complete...");
    monolib::sleep(Duration::from_millis(100)).await;
    println!("All fire-and-forget tasks should have completed");
}

// Helper function for computation
fn compute_fibonacci(n: u32) -> u64 {
    if n <= 1 {
        return n as u64;
    }
    let mut a = 0u64;
    let mut b = 1u64;
    for _ in 2..=n {
        let c = a.wrapping_add(b);
        a = b;
        b = c;
    }
    b
}
