use monorail::core::monolib;
use monorail::core::shard::state::ShardId;
use std::time::Duration;
use monorail::core::topology::MonorailTopology;
use monorail::core::topology::MonorailConfiguration;
use monorail::core::shard::shard::signal_monorail;
/// Async operations and sleep
/// Demonstrates async sleep and timing operations
#[monorail::main]
async fn main() {
    println!("Current shard: {:?}", monolib::shard_id());
    
    // Test async sleep
    println!("Sleeping for 100ms...");
    let start = std::time::Instant::now();
    monolib::sleep(Duration::from_millis(100)).await;
    let elapsed = start.elapsed();
    println!("Awake after {:?}", elapsed);
    
    assert!(elapsed >= Duration::from_millis(95), "Should sleep for at least 95ms");
    assert!(elapsed < Duration::from_millis(200), "Should not sleep too long");
    
    // Test multiple concurrent sleeps
    println!("Testing concurrent operations...");
    let handles = (0..5).map(|i| {
        monolib::call_on(ShardId::new(i % monolib::get_topology_info().cores), move || async move {
            monolib::sleep(Duration::from_millis(10 * i as u64)).await;
            i as u64
        })
    }).collect::<Vec<_>>();
    
    for (i, handle) in handles.into_iter().enumerate() {
        if let Ok(result) = handle.await {
            assert_eq!(result, i as u64, "Should return correct value");
        }
    }
    
    println!("All concurrent operations completed");
}
