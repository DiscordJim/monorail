use std::cell::Cell;

use asyncnal::{EventSetter, LocalEvent};

use crate::core::actor::base::ActorSignal;



pub struct SignalBus {
    event: LocalEvent,
    message: Cell<Option<SignalBusEvent>>
}

impl SignalBus {
    pub fn new() -> Self {
        Self {
            event: LocalEvent::new(),
            message: Cell::new(None)
        }
    }
    pub fn clear_signal(&self) -> Option<ActorSignal> {
        if let Some(e) = self.message.take() {
            Some(e.event)
        } else {
            None
        }
    }
    pub async fn wait(&self, priority: SignalPriority) -> ActorSignal {
        loop {
            let event = self.message.get();
            match event {
                None => {},
                Some(e) => if e.priority >= priority {
                    println!("Hot? {:?}", e);
                    self.message.take();
                    self.event.set_all(|| {});
                    return e.event;
                }
            }
            self.event.wait().await;
        }
    }
    fn raise_internal(&self, event: ActorSignal, priority: SignalPriority) {
        // let generation = self.get_generation();
        self.message.set(Some(SignalBusEvent {
            event,
            // generation,
            priority
        }));
        self.event.set_all(|| {});
        // generation
    }

    pub fn raise(&self, event: ActorSignal, priority: SignalPriority) {
        self.raise_internal(event, priority);
    }

}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SignalPriority {
    NonCritical = 0,
    Interrupt = 1

}

#[derive(Clone, Copy, Debug)]
struct SignalBusEvent {
    event: ActorSignal,
    priority: SignalPriority,
    // generation: usize
}


#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, sync::atomic::{AtomicUsize, Ordering}, time::Duration};

    use crate::core::{actor::{base::ActorSignal, signals::{SignalBus, SignalPriority}}, executor::scheduler::Executor, shard::state::ShardId};


    #[test]
    pub fn test_signal_bus_interrupt() {
        static INTER: AtomicUsize = AtomicUsize::new(0);

        let bus_a = SignalBus::new();
        let bus_b = SignalBus::new();

        let executor = Executor::new(ShardId::new(0));
        executor.block_on(async {

            
            assert!(bus_a.clear_signal().is_none());
            assert_eq!(INTER.load(Ordering::SeqCst), 0);
            

            let han = executor.spawn({
                println!("Spawning some st...");
                // let bus = bus.clone();
                async {

                    INTER.store(1, Ordering::SeqCst);
                    bus_a.raise(ActorSignal::Stop, SignalPriority::NonCritical);


                    // executor.sleep(Duration::from_millis(10)).await;
                    
                    smol::future::race(async {
                        executor.sleep(Duration::from_secs(3_000_000)).await
                    }, async {
                        bus_b.wait(SignalPriority::NonCritical).await;
                    }).await;
                    
                    INTER.store(2, Ordering::SeqCst);
                    bus_a.raise(ActorSignal::Stop, SignalPriority::NonCritical);

                    // executor.sleep(Duration::from_secs(3_000_000)).await;

                }
            });
            

            println!("Cool...");
            bus_a.wait(SignalPriority::NonCritical).await;
            assert_eq!(INTER.load(Ordering::SeqCst), 1);

            
            println!("Flag Passage: A");
            
            bus_b.raise(ActorSignal::Kill, SignalPriority::Interrupt);

            bus_a.wait(SignalPriority::NonCritical).await;
            assert_eq!(INTER.load(Ordering::SeqCst), 2);
            // // bus.


            han.await;


        });
    }
}