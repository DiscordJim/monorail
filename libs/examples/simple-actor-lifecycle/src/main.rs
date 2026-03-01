use monorail::core::monolib;
use monorail::core::actor::base::{Actor, SelfAddr};
use futures::channel::oneshot;
use std::time::Duration;
use monorail::core::topology::MonorailTopology;
use monorail::core::topology::MonorailConfiguration;
use monorail::core::shard::shard::signal_monorail;
struct CounterActor;

impl Actor for CounterActor {
    type Message = CounterMessage;
    type Arguments = i32;
    type State = i32;
    
    async fn pre_start(args: Self::Arguments) -> Self::State {
        println!("[Counter] Starting with initial value: {}", args);
        args
    }
    
    async fn handle(
        _this: SelfAddr<'_, Self>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        match message {
            CounterMessage::Increment => {
                *state += 1;
                println!("[Counter] Incremented to {}", *state);
            }
            CounterMessage::GetValue(tx) => {
                println!("[Counter] Returning value {}", *state);
                let _ = tx.send(*state);
            }
        }
        Ok(())
    }
    
    async fn post_stop(
        _this: SelfAddr<'_, Self>,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        println!("[Counter] Shutting down with final value: {}", *state);
        Ok(())
    }
}

enum CounterMessage {
    Increment,
    GetValue(oneshot::Sender<i32>),
}

/// Simple actor lifecycle
/// Demonstrates basic actor creation, messaging, and shutdown
#[monorail::main]
async fn main() {
    // Spawn a counter actor
    let counter = monolib::spawn_actor::<CounterActor>(10);
    
    // Increment it a few times
    for i in 1..=5 {
        let _ = counter.send(CounterMessage::Increment).await;
        println!("Sent increment #{}", i);
    }
    
    // Get the final value
    let (tx, rx) = oneshot::channel();
    let _ = counter.send(CounterMessage::GetValue(tx)).await;
    
    if let Ok(value) = rx.await {
        println!("Final counter value: {}", value);
        assert_eq!(value, 15, "Counter should be 10 + 5 increments");
    }
    
    // Stop the actor
    counter.stop();
    monolib::sleep(Duration::from_millis(50)).await;
}
