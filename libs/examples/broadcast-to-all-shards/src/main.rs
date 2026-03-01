use monorail::core::monolib;
use monorail::core::topology::MonorailTopology;
use monorail::core::topology::MonorailConfiguration;
use monorail::core::shard::shard::signal_monorail;
/// Broadcast to all shards using call_on_all
/// Demonstrates broadcasting a computation to all shards simultaneously
#[monorail::main]
async fn main() {
    let topology = monolib::get_topology_info();
    println!("Broadcasting to {} shards", topology.cores);
    
    // Broadcast a task to all shards
    let results = monolib::call_on_all(|| async {
        // Each shard reports its ID
        let shard_id = monolib::shard_id().as_usize();
        println!("Shard {} responding", shard_id);
        shard_id
    }).await;
    
    println!("Received responses from {} shards:", results.len());
    assert_eq!(results.len(), topology.cores, "Should receive response from all shards");
    
    // Collect all shard IDs that responded
    let mut shard_ids = Vec::new();
    for result in results.iter() {
        match result {
            Ok(shard_id) => shard_ids.push(*shard_id),
            Err(e) => panic!("Error response: {:?}", e),
        }
    }
    
    // Sort and verify all shards responded exactly once
    shard_ids.sort();
    for (i, shard_id) in shard_ids.iter().enumerate() {
        println!("  Shard {} responded", shard_id);
        assert_eq!(*shard_id, i, "Each shard should respond exactly once");
    }
}
