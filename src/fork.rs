use std::process::exit;

use nix::{
    sys::wait::{waitpid, WaitStatus},
    unistd::{fork, ForkResult},
};

pub fn fork_fn(fun: impl FnOnce(), blocking: bool) -> nix::unistd::Pid {
    let child = match unsafe { fork() } {
        Ok(ForkResult::Parent { child, .. }) => {
            if blocking {
                waitpid(child, None).unwrap();
            }
            child
        }
        Ok(ForkResult::Child) => {
            fun();
            exit(0);
        }
        Err(_) => panic!("Fork failed"),
    };

    child
}
