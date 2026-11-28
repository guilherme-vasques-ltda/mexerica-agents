#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static void wait_for_kill(void) {
    alarm(40);
    for (;;) pause();
}

int main(int argc, char **argv) {
    if (argc != 3 || signal(SIGTERM, SIG_IGN) == SIG_ERR) return 1;
    if (!strcmp(argv[2], "anchor")) {
        pid_t child = fork();
        if (child == -1) return 1;
        if (child > 0) return 0;
        if (setpgid(0, getsid(0)) == -1) return 1;
        FILE *marker = fopen(argv[1], "w");
        if (!marker) return 1;
        fprintf(marker, "%d\n", getpid());
        if (fclose(marker) == EOF) return 1;
        sleep(3);
        return 0;
    }
    pid_t same_group = fork();
    if (same_group == -1) return 1;
    if (same_group == 0) wait_for_kill();
    pid_t motor = fork();
    if (motor == -1) return 1;
    if (motor == 0) {
        if (setpgid(0, 0) == -1) return 1;
        pid_t leaf = fork();
        if (leaf == -1) return 1;
        if (leaf > 0) return 0;
        FILE *marker = fopen(argv[1], "w");
        if (!marker) return 1;
        fprintf(marker, "%d %d %d %d\n", getpid(), getpgrp(), same_group, getsid(0));
        if (fclose(marker) == EOF) return 1;
        wait_for_kill();
        return 1;
    }
    if (waitpid(motor, NULL, 0) != motor) return 1;
    if (!strcmp(argv[2], "exit")) return 0;
    wait_for_kill();
    return 1;
}
