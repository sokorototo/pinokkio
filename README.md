## `pinokkio`: A minimal, single-threaded async runtime.

Basically one step above `pollster`, as it allows spawning tasks. Currently for purely educational purposes. `wasm-bindgen` compatibility is WIP.

### 🧪 Sample Usage:

```rust
let fut = async { 42 };

let mut rt = rt::Runtime::new();
let result = rt.block_on(fut);

assert_eq!(result, 42);
```

The following example demonstrates spawning a task, and awaiting it's completion with a `TaskMonitor` future.

```rust
let fut_60 = async {
   let mut counter = 60;

   loop {
      futures::future::ready(()).await;
      counter -= 1;

      println!("Counter = {}", counter);
      if counter == 0 {
         break;
      }
   }
};

let mut rt = rt::Runtime::new();
let monitor = rt.spawn(fut_60);

rt.block_on(monitor);
```

### 🧸 Extras:

Enabled via the `timers` Cargo Feature, `pinokkio` contains a simple implementation of async timers.
 - Lightweight, no external dependencies and with decent resolution.
 - Preemptive, uses a dedicated sleep thread to put process to sleep. Instead of busy looping waiting for the duration to expire.

Spawns several tasks, each sleeping for a set duration and awaits their combined completion using `futures::join_all`

```rust
let mut rt = rt::Runtime::new();

fn task<R: fmt::Display>(id: R) -> impl Future<Output = ()> {
   async move {
      println!("[{}] Sleeping for 5s", id);

      for i in 0..5 {
         println!("[{}]: {}s left", id, 5 - i);
         sleep(time::Duration::from_secs(1)).await;
      }

      println!("[{}] Done sleeping", id);
   }
}

let tasks = (0..5).map(|id| rt.spawn(task(id)));
let join = futures::future::join_all(tasks);

let results = rt.block_on(join);
assert!(results.len() == 5);
```