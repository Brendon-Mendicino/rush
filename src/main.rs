use std::{
    env,
    io::{self, Write},
    process,
};

use crate::{
    executor::Executor,
    invocation::{Invocation, USAGE},
};

mod builtin;
mod cmd_runner;
mod executor;
mod invocation;
mod job;
mod nix;
mod pattern;
mod shell;
mod syntax;
mod tokenizer;
mod waiter;

fn main() {
    let invocation = match Invocation::parse(env::args()) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("rush: {message}\n{USAGE}");
            process::exit(2);
        }
    };

    let code = Executor::start(invocation);

    let _ = io::stdout().flush();
    process::exit(code);
}
