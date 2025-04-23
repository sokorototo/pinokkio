## `pinokkio`: A minimal, single-threaded async runtime.

Basically one step above `pollster`, as it allows spawning tasks _and_ blocking the current thread. Currently for purely educational purposes. Only depends on `oneshot`, which in itself depends on nothing else, `oneshot` makes my life so much easier. wasm-bindgen compatibility is WIP.

### 🧪 Sample Usage:

```rust
let fut = async { 42 };

let mut rt = pinokkio::rt::Runtime::new();
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

let mut rt = pinokkio::rt::Runtime::new();
let monitor = rt.spawn(fut_60);

rt.block_on(monitor);
```

### 🧸 Extras:

Enabled via the `timers` Cargo Feature, `pinokkio` contains an implementation of async timers. I have not read other implementations, mine is implemented with a `SleepSubroutine` that polls other `Sleep` futures:
 - Pros:
   - Sleep futures don't poll themselves. They immediately return if already due, acting like a simple `futures::yield_now`.
 - Cons:
   - Having state managed externally by `SleepSubroutine` means dropping the `Sleep` future before it's due results in a zombie timer. This is memory that is managed but never freed.

---

Spawns several tasks, each sleeping for a set duration and awaits their combined completion using `futures::join_all`

```rust
let mut rt = pinokkio::rt::Runtime::new();

fn task<R: std::fmt::Display>(id: R) -> impl Future<Output = ()> {
   async move {
      println!("[{}] Sleeping for 5s", id);

      for i in 0..5 {
         println!("[{}]: {}s left", id, 5 - i);
         pinokkio::timers::sleep(time::Duration::from_secs(1)).await;
      }

      println!("[{}] Done sleeping", id);
   }
}

// initialize Sleep subroutine
rt.spawn(pinokkio::timers::SleepSubroutine);

let tasks = (0..5).map(|id| rt.spawn(task(id)));
let join = futures::future::join_all(tasks);

let results = rt.block_on(join);
assert!(results.len() == 5);
```