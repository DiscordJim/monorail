use monorail::core::monolib;
use monorail::core::actor::base::{Actor, SelfAddr};
use futures::channel::oneshot;
use std::time::Duration;
use monorail::core::topology::MonorailTopology;
use monorail::core::topology::MonorailConfiguration;
use monorail::core::shard::shard::signal_monorail;
/// A worker actor that processes data on a specific shard
struct WorkerActor;

#[derive(Debug)]
struct WorkItem {
    id: u64,
    data: Vec<u8>,
    result_tx: oneshot::Sender<ProcessedResult>,
}

#[derive(Debug)]
struct ProcessedResult {
    id: u64,
    checksum: u64,
    worker_shard: usize,
}

impl Actor for WorkerActor {
    type Message = WorkItem;
    type Arguments = ();
    type State = WorkerState;

    async fn pre_start(_args: Self::Arguments) -> Self::State {
        println!("[Worker on shard {:?}] Starting up", monolib::shard_id());
        WorkerState {
            processed_count: 0,
        }
    }

    async fn handle(
        _this: SelfAddr<'_, Self>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        // Simulate some CPU-intensive work
        let checksum = message.data.iter().fold(0u64, |acc, &x| acc.wrapping_add(x as u64));
        
        state.processed_count += 1;
        
        println!(
            "[Worker on shard {:?}] Processed item {} (total: {})",
            monolib::shard_id(),
            message.id,
            state.processed_count
        );

        let result = ProcessedResult {
            id: message.id,
            checksum,
            worker_shard: monolib::shard_id().as_usize(),
        };

        let _ = message.result_tx.send(result);
        Ok(())
    }

    async fn post_stop(
        _this: SelfAddr<'_, Self>,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        println!(
            "[Worker on shard {:?}] Shutting down. Processed {} items total.",
            monolib::shard_id(),
            state.processed_count
        );
        Ok(())
    }
}

struct WorkerState {
    processed_count: u64,
}

/// Worker actor with message passing
/// Demonstrates the WorkerActor processing work items
#[monorail::main]
async fn main() {
    // Spawn a worker actor
    let worker = monolib::spawn_actor::<WorkerActor>(());
    
    // Send some work items
    let mut result_receivers = Vec::new();
    
    for i in 0..5 {
        let (result_tx, result_rx) = oneshot::channel();
        
        let work_item = WorkItem {
            id: i,
            data: vec![i as u8; 100],
            result_tx,
        };
        
        let _ = worker.send(work_item).await;
        result_receivers.push(result_rx);
    }
    
    // Collect results
    let mut total_checksum = 0u64;
    for (i, rx) in result_receivers.into_iter().enumerate() {
        if let Ok(result) = rx.await {
            println!("Work item {} completed on shard {}", 
                result.id, 
                result.worker_shard
            );
            total_checksum += result.checksum;
        } else {
            panic!("Failed to receive result for work item {}", i);
        }
    }
    
    println!("Total checksum from all work items: {}", total_checksum);
    assert!(total_checksum > 0, "Should have processed work items");
    
    // Stop the worker
    worker.stop();
    monolib::sleep(Duration::from_millis(50)).await;
}
