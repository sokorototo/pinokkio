use std::fmt;

use super::*;
use crate::timers::sleep;

#[test]
fn simple_chain() {
	let fut_1 = async { 42 };
	let fut_2 = async move { fut_1.await + 1 };
	let fut_3 = async move { fut_2.await + 1 };

	let mut rt = Runtime::new();
	let result = rt.block_on(fut_3);

	assert_eq!(result, Some(44));
}

#[test]
fn auto_tasks() {
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

	let mut rt = Runtime::new();
	let monitor = rt.spawn(fut_60);

	rt.block_on(monitor).unwrap();
}

#[test]
fn sleep_tasks() {
	let mut rt = Runtime::new();

	let sleep_5s = async {
		println!("Sleeping for 5s");

		for i in 0..5 {
			println!("{}s left", 5 - i);
			sleep(time::Duration::from_secs(1)).await;
		}

		println!("Done sleeping");
	};

	rt.block_on(sleep_5s).unwrap();
}

#[test]
fn green_threads() {
	let mut rt = Runtime::new();

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

	let results = rt.block_on(join).unwrap();
	assert!(results.len() == 5);
}
