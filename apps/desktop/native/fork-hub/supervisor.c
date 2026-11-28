#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/sysctl.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

#ifndef HUB_EOF_MS
#define HUB_EOF_MS 15000
#endif
#ifndef HUB_TERM_MS
#define HUB_TERM_MS 3000
#endif
#ifndef HUB_KILL_MS
#define HUB_KILL_MS 5000
#endif

static int wait_child(pid_t pid, int *status) {
    pid_t result;
    do {
        result = waitpid(pid, status, 0);
    } while (result == -1 && errno == EINTR);
    return result == pid ? 0 : -1;
}

static int signal_owned_group(pid_t session, pid_t group, int signal) {
    // The helper pins the joined group and checks its actual SID before kill(0).
    pid_t helper = fork();
    if (helper == -1) return -1;
    if (helper == 0) {
        if (setpgid(0, group) == -1) _exit(errno == EPERM || errno == ESRCH ? 2 : 1);
        if (getsid(0) != session || getpgrp() != group) _exit(1);
        _exit(kill(0, signal) == 0 ? 0 : 1);
    }
    int status;
    if (wait_child(helper, &status) == -1) return -1;
    if (WIFSIGNALED(status) && WTERMSIG(status) == signal) return 0;
    if (WIFEXITED(status) && (WEXITSTATUS(status) == 0 || WEXITSTATUS(status) == 2)) return 0;
    return -1;
}

static int visit_session(pid_t session, int signal) {
    int mib[] = {CTL_KERN, KERN_PROC, KERN_PROC_ALL};
    size_t length;
    struct kinfo_proc *procs = NULL;
    for (int retry = 0; retry < 8; retry++) {
        if (sysctl(mib, 3, NULL, &length, NULL, 0) == -1 || length == 0) return -1;
        procs = malloc(length);
        if (!procs) return -1;
        if (sysctl(mib, 3, procs, &length, NULL, 0) == 0) break;
        int error = errno;
        free(procs);
        procs = NULL;
        if (error != ENOMEM) return -1;
    }
    if (!procs) return -1;
    int found = length == 0 || length % sizeof(*procs) != 0 ? -1 : 0;
    for (size_t i = 0; found >= 0 && i < length / sizeof(*procs); i++) {
        pid_t pid = procs[i].kp_proc.p_pid;
        if (pid <= 0 || pid == session) continue;
        pid_t sid = getsid(pid);
        if (sid == -1) {
            if (errno == ESRCH) continue;
            found = -1;
            break;
        }
        if (sid != session) continue;
        found = 1;
        pid_t group = getpgid(pid);
        if (group == -1 && errno == ESRCH) continue;
        if (group <= 0 || group == session) {
            found = -1;
            break;
        }
        if (signal != 0 && signal_owned_group(session, group, signal) == -1) {
            found = -1;
            break;
        }
    }
    free(procs);
    return found;
}

static int child_exited(pid_t child) {
    siginfo_t info = {0};
    if (waitid(P_PID, (id_t)child, &info, WEXITED | WNOHANG | WNOWAIT) == -1) return -1;
    return info.si_pid != 0;
}

static int drain(pid_t session, pid_t child, unsigned milliseconds, int signal) {
    uint64_t deadline = clock_gettime_nsec_np(CLOCK_MONOTONIC) + (uint64_t)milliseconds * 1000000;
    bool empty = false;
    do {
        int exited = child_exited(child);
        if (exited < 0) return -1;
        int remaining = exited || signal != 0 ? visit_session(session, signal) : 1;
        if (remaining < 0) return -1;
        if (exited && remaining == 0) {
            if (empty) return 0;
            empty = true;
        } else {
            empty = false;
        }
        if (poll(NULL, 0, 50) == -1 && errno != EINTR) return -1;
    } while (clock_gettime_nsec_np(CLOCK_MONOTONIC) < deadline);
    return 1;
}

static pid_t spawn_hub(char **argv, int *input) {
    int fds[2];
    if (pipe(fds) == -1) return -1;
    posix_spawnattr_t attr;
    posix_spawn_file_actions_t actions;
    pid_t pid = -1;
    if (posix_spawnattr_init(&attr) == 0) {
        if (posix_spawn_file_actions_init(&actions) == 0) {
            sigset_t defaults;
            sigemptyset(&defaults);
            sigaddset(&defaults, SIGPIPE);
            if (posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_SETSIGDEF) == 0 &&
                posix_spawnattr_setpgroup(&attr, 0) == 0 &&
                posix_spawnattr_setsigdefault(&attr, &defaults) == 0 &&
                posix_spawn_file_actions_adddup2(&actions, fds[0], STDIN_FILENO) == 0 &&
                posix_spawn_file_actions_adddup2(&actions, STDERR_FILENO, STDOUT_FILENO) == 0 &&
                posix_spawn_file_actions_addclose(&actions, fds[0]) == 0 &&
                posix_spawn_file_actions_addclose(&actions, fds[1]) == 0) {
                pid_t spawned;
                if (posix_spawn(&spawned, argv[0], &actions, &attr, argv, environ) == 0) pid = spawned;
            }
            posix_spawn_file_actions_destroy(&actions);
        }
        posix_spawnattr_destroy(&attr);
    }
    close(fds[0]);
    if (pid == -1) close(fds[1]);
    else *input = fds[1];
    return pid;
}

int main(int argc, char **argv) {
    pid_t session = getpid();
    if (argc < 2 || argv[1][0] != '/' || getsid(0) != session || getpgrp() != session) return 1;
    char start;
    ssize_t n;
    do {
        n = read(STDIN_FILENO, &start, 1);
    } while (n == -1 && errno == EINTR);
    if (n == 0) return 0;
    if (n != 1 || start != 's' || signal(SIGPIPE, SIG_IGN) == SIG_ERR ||
        signal(SIGCHLD, SIG_DFL) == SIG_ERR) return 1;
    int input = -1;
    pid_t hub = spawn_hub(argv + 1, &input);
    if (hub == -1) return 0;
    struct pollfd control = {STDIN_FILENO, POLLIN, 0};
    if (dprintf(STDOUT_FILENO, "%d\n", hub) >= 0) {
        for (;;) {
            int result = poll(&control, 1, 50);
            if (result == -1 && errno == EINTR) continue;
            if (result != 0) break;
            int exited = child_exited(hub);
            if (exited == 0) continue;
            // Hold the PID unreaped until the app closes the control pipe.
            if (exited < 0 || dprintf(STDOUT_FILENO, "x\n") < 0) break;
            do {
                result = poll(&control, 1, -1);
            } while (result == -1 && errno == EINTR);
            break;
        }
    }
    close(input);
    int drained = drain(session, hub, HUB_EOF_MS, 0);
    if (drained == 1) drained = drain(session, hub, HUB_TERM_MS, SIGTERM);
    if (drained == 1) drained = drain(session, hub, HUB_KILL_MS, SIGKILL);
    int status;
    return drained == 0 && wait_child(hub, &status) == 0 ? 0 : 1;
}
