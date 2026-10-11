/*
void Waiter::process_wstatus(Job &job, pid_t pid, int wstatus) {
    assertm(pid != 0,
            "A pid=0 likely means a return from waitpid(...,WNOHANG) no "
            "signal was received from any child");

    pid_t pgid = job.pgid;

    if (pid == -1) {
        throw std::runtime_error(std::format("wait_job: waitpid({}): {}", -pgid,
                                             std::strerror(errno)));
    }

    if (!job.jobs.contains(pid)) {
        throw std::runtime_error(
            std::format("pid={} is not part of pgid={}", pid, pgid));
    }

    auto &stats = job.jobs.at(pid);

    if (WIFSTOPPED(wstatus)) {
        stats.stopped = true;
        std::println(stderr, "{}: stopped by {}({})", pid,
                     strsignal(WSTOPSIG(wstatus)), WSTOPSIG(wstatus));
        return;
    }

    stats.completed = true;

    if (WIFEXITED(wstatus)) {
        stats.exit_code = WEXITSTATUS(wstatus);
    } else if (WIFSIGNALED(wstatus)) {
        stats.exit_code = 1;
        stats.signaled = WTERMSIG(wstatus);

        std::println(stderr, "{}: Terminated by signal {}({})", pid,
                     strsignal(WTERMSIG(wstatus)), WTERMSIG(wstatus));
    }
}

Job Waiter::wait_job(Job &&job) {
    // TODO: this pgid, when executing in non-interctive shell, should be
    // equal for all the jobs
    pid_t pgid = job.pgid;

    assertm((pgid != 0) || (pgid == 0 && job.completed()),
            "A job with a pgid unitialized must be completed.");

    while (!job.completed() && !job.stopped()) {
        int wstatus;
        const pid_t pid = waitpid(-pgid, &wstatus, WUNTRACED);

        process_wstatus(job, pid, wstatus);
    }

    return job;
}

void Waiter::update_status(Job &job) {
    // TODO: this pgid, when executing in non-interctive shell, should
    // be equal for all the jobs
    pid_t pgid = job.pgid;

    assertm((pgid != 0) || (pgid == 0 && job.completed()),
            "A job with a pgid unitialized must be completed.");

    for (;;) {
        int wstatus;
        const pid_t pid = waitpid(-pgid, &wstatus, WUNTRACED | WNOHANG);

        if (pid == 0) {
            /* No processes ready to report.  */
            break;
        }

        if (pid == -1 && errno == ECHILD) {
            /* No processes ready to report.  */
            break;
        }

        process_wstatus(job, pid, wstatus);
    }
}

void Waiter::wait(Job &job) const {
    Job retval;

    if (shell.is_interactive) {
        /* Wait for it to report.  */
        retval = Waiter::wait_job(std::move(job));

        /* Put the shell back in the foreground.  */
        if (tcsetpgrp(shell.terminal, shell.pgid) == -1) {
            std::println(stderr, "tcsetpgrp({}, {}): {}", shell.terminal,
                         shell.pgid, std::strerror(errno));
        }

        /* Restore the shell’s terminal modes.  */
        job.set_modes(shell);
        // TODO: this is not fine when calling `stty`. Decide how do handle this
        tcsetattr(shell.terminal, TCSADRAIN, &shell.tmodes);
    } else {
        retval = Waiter::wait_job(std::move(job));
    }

    job = retval;
}

void Waiter::wait_inside_async(Job &job) const {
    pid_t pgid = job.pgid;

    /* Don't check for stopped jobs. An asyn list will terminate only when all
     * the childred are completed. Some of them might get stopped by the tty. We
     * don't want to loose them before exiting the async list.
     */
    while (!job.completed()) {
        int wstatus;
        const pid_t pid = waitpid(-pgid, &wstatus, WUNTRACED);

        process_wstatus(job, pid, wstatus);
    }
}

void Waiter::bg(Job &job) const {
    if (kill(-job.pgid, SIGCONT) < 0) {
        std::println(stderr, "kill({}, SIGCONG): {}", -job.pgid,
                     std::strerror(errno));
    }

    // TODO: error handling?
    job.mark_running();
}

void Waiter::fg(Job &job) const {
    /* Put the job in foreground */
    tcsetpgrp(shell.terminal, job.pgid);

    /* Restore the tmodes on the terminal before sening SIGCONT */
    job.restore_modes(shell);

    if (kill(-job.pgid, SIGCONT) < 0) {
        std::println(stderr, "kill({}, SIGCONT): {}", -job.pgid,
                     std::strerror(errno));
    }

    // TODO: error handling?
    job.mark_running();

    this->wait(job);
}
*/

use crate::{
    job::Job,
    nix::{waitpid, Pid, WaitPid},
};

pub struct Waiter {
    is_interactive: bool,
}

impl Waiter {
    fn process_wstatus(&self, job: Job, pid: Pid, wstatus: i32) {
        /*
            assertm(pid != 0,
                    "A pid=0 likely means a return from waitpid(...,WNOHANG) no "
                    "signal was received from any child");

            pid_t pgid = job.pgid;

            if (pid == -1) {
                throw std::runtime_error(std::format("wait_job: waitpid({}): {}", -pgid,
                                                     std::strerror(errno)));
            }

            if (!job.jobs.contains(pid)) {
                throw std::runtime_error(
                    std::format("pid={} is not part of pgid={}", pid, pgid));
            }

            auto &stats = job.jobs.at(pid);

            if (WIFSTOPPED(wstatus)) {
                stats.stopped = true;
                std::println(stderr, "{}: stopped by {}({})", pid,
                             strsignal(WSTOPSIG(wstatus)), WSTOPSIG(wstatus));
                return;
            }

            stats.completed = true;

            if (WIFEXITED(wstatus)) {
                stats.exit_code = WEXITSTATUS(wstatus);
            } else if (WIFSIGNALED(wstatus)) {
                stats.exit_code = 1;
                stats.signaled = WTERMSIG(wstatus);

                std::println(stderr, "{}: Terminated by signal {}({})", pid,
                             strsignal(WTERMSIG(wstatus)), WTERMSIG(wstatus));
            }
        */
        let pgid = job.pgid;

        let child = job.jobs.get_mut(&pid).unwrap();

        let wait = child.wait().unwrap();
    }

    /*
    Job Waiter::wait_job(Job &&job) {
        // TODO: this pgid, when executing in non-interctive shell, should be
        // equal for all the jobs
        pid_t pgid = job.pgid;

        assertm((pgid != 0) || (pgid == 0 && job.completed()),
                "A job with a pgid unitialized must be completed.");

        while (!job.completed() && !job.stopped()) {
            int wstatus;
            const pid_t pid = waitpid(-pgid, &wstatus, WUNTRACED);

            process_wstatus(job, pid, wstatus);
        }

        return job;
    }
        */
    fn wait_job(&self, job: Job) {
        let pgid = job.pgid;

        waitpid(WaitPid::Group(pgid));
    }

    pub fn wait(&self, job: Job) {
        if (self.is_interactive) {}
    }
}
