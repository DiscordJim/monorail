Hello

# Waiters
1. Everything should have their own waiter.

# Todo
1. Actors can recreate themselves.
~~2. Supervision messages.~~
3. Core recovery.
4. Registration.
5. Optimization for multiple threads on same core
6. Bridge channel backlogs
7. actor channel backpressure.
8. actor recovery,.
9. better actor channels in general
~~10. actors can unmanage themselves~~
~~11. wait for stop~~
12. Better runtime signalling

# Using

There are three core concepts that must be defined:

## Shard

A shard is a computation engine running and pinned to a particular CPU core. Each shard has its own async scheduler.

## Actor

An actor runs on a particular shard. The shard schedules its work, and passes messages to it for processing. Messages can come frmo any shard.

## Topology

The topology in monorail defines how many shards are active, and sets up the communication between them.

## Basic examples

Check libs/examples for more info

### Basic Setup

```rust
fn main() {
    MonorailTopology::normal(|| async {
        let topology = monolib::get_topology_info();
        let num_cores = topology.cores;
        
        // signal shutdown
        signal_monorail(Ok(()));
    })
}
```
This can be simplified with proc macros.
```rust
#[monorail::main]
async fn main() {
    let topology = monolib::get_topology_info();
    let num_cores = topology.cores;
}
```

### Actors

Define an actor by implementing the `Actor` trait:

```rust
struct CounterActor;

enum CounterMessage {
    Increment,
    GetValue(oneshot::Sender<i32>),
}

impl Actor for CounterActor {
    type Message = CounterMessage;
    type Arguments = i32;  // initial value
    type State = i32;
    
    async fn pre_start(args: Self::Arguments) -> Self::State {
        args  // initialize state with starting value
    }
    
    async fn handle(
        _this: SelfAddr<'_, Self>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        match message {
            CounterMessage::Increment => *state += 1,
            CounterMessage::GetValue(tx) => { let _ = tx.send(*state); }
        }
        Ok(())
    }
}
```

Spawn and use actors:

```rust
// Spawn on current shard
let counter = monolib::spawn_actor::<CounterActor>(10);

// Send messages
counter.send(CounterMessage::Increment).await;

// Get response
let (tx, rx) = oneshot::channel();
counter.send(CounterMessage::GetValue(tx)).await;
let value = rx.await.unwrap();

// Stop actor
counter.stop();
```

### Cross-Shard Operations

Submit work to specific shards:

```rust
// Fire and forget
monolib::submit_to(ShardId::new(2), async move || {
    println!("Running on shard 2");
});

// Call and wait for result
let result = monolib::call_on(ShardId::new(3), async move || {
    42
}).await.unwrap();

// Broadcast to all shards
let results = monolib::call_on_all(|| async {
    monolib::shard_id().as_usize()
}).await;
```

Actors go to the shard from which they are launched in the origin.

### Named actors

Register actors with a name for discovery:

```rust
// Spawn and register
let counter = monolib::spawn_actor::<CounterActor>(0)
    .register("my_counter")
    .await;

// Query by name from anywhere
let counter = monolib::actors::get::<CounterActor>("my_counter").await.unwrap();
counter.send(CounterMessage::Increment).await;
```
