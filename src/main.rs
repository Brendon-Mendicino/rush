use crate::executor::Executor;

mod builtin;
mod cmd_runner;
mod executor;
mod job;
mod nix;
mod shell;
mod syntax;
mod tokenizer;
mod waiter;

fn main() {
    let mut executor = Executor::new();

    executor.run();
}
