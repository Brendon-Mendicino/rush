use crate::executor::Executor;

mod cmd_runner;
mod executor;
mod job;
mod nix;
mod syntax;
mod tokenizer;
mod waiter;

fn main() {
    let mut executor = Executor::default();
    println!("Hello, world!");

    executor.run();
}
