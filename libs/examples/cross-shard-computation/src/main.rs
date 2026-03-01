use monorail::core::monolib;
use monorail::core::shard::state::ShardId;
use monorail::core::topology::MonorailTopology;
use monorail::core::topology::MonorailConfiguration;
use monorail::core::shard::shard::signal_monorail;
/// Cross-shard computation using call_on
/// Demonstrates submitting tasks to specific shards and collecting results
#[monorail::main]
async fn main() {
    let topology = monolib::get_topology_info();
    println!("Topology initialized with {} cores", topology.cores);
    
    let num_cores = topology.cores;
    let mut handles = Vec::new();
    
    // Submit tasks to all shards
    for i in 0..num_cores {
        let shard_id = ShardId::new(i);
        let handle = monolib::call_on(shard_id, move || async move {
            // Each shard computes a Fibonacci number
            let fib = compute_fibonacci(20 + i as u32);
            println!("Shard {} computed fib({}) = {}", 
                monolib::shard_id().as_usize(), 
                20 + i as u32,
                fib
            );
            fib
        });
        handles.push(handle);
    }
    
    // Wait for all results
    let mut total = 0u64;
    for handle in handles {
        if let Ok(result) = handle.await {
            total += result;
        }
    }
    println!("Total sum from all shards: {}", total);
    assert!(total > 0, "Should have computed some fibonacci numbers");
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
