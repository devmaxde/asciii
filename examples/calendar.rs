use asciii::{actions::calendar_and_tasks as calendar, storage::StorageDir};

fn main() {
	let dir = StorageDir::All;

	println!("{}", calendar(dir).unwrap())
}
