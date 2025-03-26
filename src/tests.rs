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
			futures_lite::future::yield_now().await;
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

	fn task(i: usize) -> impl Future<Output = ()> {
		async move {
			println!("[{}] Sleeping for 5s", i);

			for i in 0..5 {
				println!("{}s left", 5 - i);
				sleep(time::Duration::from_secs(1)).await;
			}

			println!("[{}] Done sleeping", i);
		}
	}

	let task_1 = rt.spawn(task(1));
	let task_2 = rt.spawn(task(2));

	rt.block_on(async move {
		futures_lite::future::zip(task_1, task_2).await;
	});
}
