use monorail::core::monolib;
use monorail::core::{
    actor::base::{Actor, SelfAddr},
    shard::state::ShardId,
};
use futures::channel::oneshot;
use std::time::Duration;
use monorail::core::topology::MonorailTopology;
use monorail::core::topology::MonorailConfiguration;
use monorail::core::shard::shard::signal_monorail;
/// A coordinator actor that distributes work across shards
struct CoordinatorActor;

struct CoordinateTask {
    num_items: u64,
    completion_tx: oneshot::Sender<Vec<ProcessedResult>>,
}

struct CoordinatorState {
    worker_shards: Vec<ShardId>,
}

#[derive(Debug)]
struct ProcessedResult {
    id: u64,
    checksum: u64,
    worker_shard: usize,
}

impl Actor for CoordinatorActor {
    type Message = CoordinateTask;
    type Arguments = Vec<ShardId>;
    type State = CoordinatorState;

    async fn pre_start(args: Self::Arguments) -> Self::State {
        println!("[Coordinator] Starting with {} worker shards", args.len());
        CoordinatorState {
            worker_shards: args,
        }
    }

    async fn handle(
        _this: SelfAddr<'_, Self>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        println!("[Coordinator] Distributing {} work items", message.num_items);
        
        let mut results = Vec::new();
        
        // Distribute work across shards in round-robin fashion
        for i in 0..message.num_items {
            let shard_idx = (i as usize) % state.worker_shards.len();
            let target_shard = state.worker_shards[shard_idx];
            
            // Submit work to the target shard
            let (tx, rx) = oneshot::channel();
            
            monolib::submit_to(target_shard, async move || {
                // Spawn a worker actor on the target shard if needed
                // For this example, we'll just do the work directly
                let data: Vec<u8> = (0..100).map(|x| ((i + x) % 256) as u8).collect();
                let checksum = data.iter().fold(0u64, |acc, &x| acc.wrapping_add(x as u64));
                
                let result = ProcessedResult {
                    id: i,
                    checksum,
                    worker_shard: monolib::shard_id().as_usize(),
                };
                
                let _ = tx.send(result);
            });
            
            // Collect the result
            if let Ok(result) = rx.await {
                results.push(result);
            }
        }
        
        println!("[Coordinator] All work completed. Collected {} results", results.len());
        let _ = message.completion_tx.send(results);
        
        Ok(())
    }

    async fn post_stop(
        _this: SelfAddr<'_, Self>,
        _state: &mut Self::State,
    ) -> anyhow::Result<()> {
        println!("[Coordinator] Shutting down");
        Ok(())
    }
}

/// A statistics collector that gathers results from all shards
struct StatsActor;

#[derive(Debug)]
enum StatsMessage {
    RecordResult(ProcessedResult),
    GetStats(oneshot::Sender<Statistics>),
}

#[derive(Debug, Clone)]
struct Statistics {
    total_processed: u64,
    per_shard_count: Vec<(usize, u64)>,
    total_checksum: u64,
}

struct StatsState {
    results: Vec<ProcessedResult>,
}

impl Actor for StatsActor {
    type Message = StatsMessage;
    type Arguments = ();
    type State = StatsState;

    async fn pre_start(_args: Self::Arguments) -> Self::State {
        println!("[Stats] Starting statistics collector");
        StatsState {
            results: Vec::new(),
        }
    }

    async fn handle(
        _this: SelfAddr<'_, Self>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        match message {
            StatsMessage::RecordResult(result) => {
                state.results.push(result);
            }
            StatsMessage::GetStats(tx) => {
                // Calculate statistics
                let mut per_shard: std::collections::HashMap<usize, u64> = std::collections::HashMap::new();
                let mut total_checksum = 0u64;
                
                for result in &state.results {
                    *per_shard.entry(result.worker_shard).or_insert(0) += 1;
                    total_checksum = total_checksum.wrapping_add(result.checksum);
                }
                
                let mut per_shard_vec: Vec<_> = per_shard.into_iter().collect();
                per_shard_vec.sort_by_key(|(shard, _)| *shard);
                
                let stats = Statistics {
                    total_processed: state.results.len() as u64,
                    per_shard_count: per_shard_vec,
                    total_checksum,
                };
                
                println!("[Stats] Reporting: {:?}", stats);
                let _ = tx.send(stats);
            }
        }
        Ok(())
    }

    async fn post_stop(
        _this: SelfAddr<'_, Self>,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        println!("[Stats] Shutting down. Processed {} results total", state.results.len());
        Ok(())
    }
}

/// Actor-based distributed processing
/// Demonstrates spawning actors, message passing, and coordination
#[monorail::main]
async fn main() {
    let topology = monolib::get_topology_info();
    let num_cores = topology.cores;
    
    // Spawn a coordinator on shard 0
    let coordinator = monolib::spawn_actor::<CoordinatorActor>(
        (0..num_cores).map(|i| ShardId::new(i)).collect()
    ).register("coordinator").await;
    
    // Spawn a stats collector on shard 0
    let stats_actor = monolib::spawn_actor::<StatsActor>(());
    
    // Submit a coordination task
    let (tx, rx) = oneshot::channel();
    let _ = coordinator.send(CoordinateTask {
        num_items: 50,
        completion_tx: tx,
    }).await;
    
    // Wait for results
    if let Ok(results) = rx.await {
        println!("Received {} results from coordinator", results.len());
        assert_eq!(results.len(), 50, "Should have processed all 50 items");
        
        // Send results to stats collector
        for result in results {
            let _ = stats_actor.send(StatsMessage::RecordResult(result)).await;
        }
    }
    
    // Get final statistics
    let (stats_tx, stats_rx) = oneshot::channel();
    let _ = stats_actor.send(StatsMessage::GetStats(stats_tx)).await;
    if let Ok(stats) = stats_rx.await {
        println!("\n=== Final Statistics ===");
        println!("Total items processed: {}", stats.total_processed);
        println!("Per-shard distribution:");
        for (shard, count) in &stats.per_shard_count {
            println!("  Shard {}: {} items", shard, count);
        }
        println!("Total checksum: {}", stats.total_checksum);
        
        assert_eq!(stats.total_processed, 50, "Should have recorded all items");
    }
    
    // Clean shutdown
    coordinator.stop();
    stats_actor.stop();
    monolib::sleep(Duration::from_millis(50)).await;
}
