# First-party lifecycle fixture. Only synthetic descendants; no provider or user data.
use strict;
use warnings;
use POSIX ();
use IO::Handle;
use Errno qw(ECHILD EINTR);

my $mode = shift @ARGV;
die "mode" unless $mode =~ /^(plain|grouped|detached|descendant)$/;
STDOUT->autoflush(1);
pipe(my $go_read, my $go_write) or die "pipe: $!";
pipe(my $ready_read, my $ready_write) or die "pipe: $!";
$go_write->autoflush(1);
$ready_write->autoflush(1);
# The sole writer lives in this supervisor, never in the intermediate/worker. EOF ends
# the worker even if its immediate parent was killed before announcing the worker PID.
pipe(my $life_read, my $life_write) or die "lifeline: $!";
my $owner = $$;
my $child = 0;
# A handled signal must not unwind between the kernel consuming an identity and
# clearing its owner slot. Poll only the nonblocking wait under this short mask;
# live-child waiting stays outside it so TERM/watchdog can still cancel promptly.
my $wait_signals = POSIX::SigSet->new(
    POSIX::SIGTERM(), POSIX::SIGINT(), POSIX::SIGHUP(), POSIX::SIGALRM(), POSIX::SIGPIPE()
);
sub wait_owned {
    my ($slot) = @_;
    die "no owned child" unless $$slot > 0; # never wait for PID0/a group
    while (1) {
        my $previous = POSIX::SigSet->new();
        POSIX::sigprocmask(POSIX::SIG_BLOCK(), $wait_signals, $previous) or die "block wait signals: $!";
        my $pid = waitpid($$slot, POSIX::WNOHANG());
        my ($status, $error) = ($?, 0 + $!);
        # ECHILD also proves we no longer own a signalable identity, but remains
        # an error below. EINTR does not consume it and must preserve ownership.
        $$slot = 0 if $pid > 0 || ($pid == -1 && $error == ECHILD);
        POSIX::sigprocmask(POSIX::SIG_SETMASK(), $previous) or die "restore wait signals: $!";
        return $status if $pid > 0;
        if ($pid == -1) {
            next if $error == EINTR;
            $! = $error;
            die "wait owned child: $!";
        }
        select undef, undef, undef, 0.01;
    }
}
my $cleanup = sub {
    local $SIG{TERM} = 'IGNORE';
    local $SIG{INT} = 'IGNORE';
    local $SIG{HUP} = 'IGNORE';
    local $SIG{ALRM} = 'IGNORE';
    local $SIG{PIPE} = 'IGNORE';
    alarm 0;
    close $life_write if defined fileno($life_write);
    close $go_write if defined fileno($go_write);
    if ($child) {
        # Still an unreaped direct child: its PID cannot be reused. No group signalling.
        my $signal = $^O eq 'linux' ? 'KILL' : 'TERM';
        kill $signal, $child;
    }
    # Linux adopts the killed intermediate's worker here, including a worker whose PID
    # never reached the readiness pipe. Only this separate supervisor's own children
    # can be returned; wait consumes each identity exactly once. No process-table scan.
    while (1) {
        my $pid = waitpid(-1, 0);
        $child = 0 if $pid == $child || ($pid == -1 && $! == ECHILD);
        next if $pid > 0 || $! == EINTR;
        die "wait descendants: $!" unless $pid == -1 && $! == ECHILD;
        last;
    }
    $child = 0;
};
our $lifecycle_before_cleanup; # Optional first-party test probe, before signal masking.
END {
    my $status = $?;
    if ($$ == $owner && $cleanup) {
        $lifecycle_before_cleanup->() if $lifecycle_before_cleanup;
        $cleanup->();
    }
    $? = $status; # waitpid's final ECHILD must not turn normal EOF into exit255
}
sub interrupt {
    # EOF/protocol failure can already have entered END when Rust's Drop sends TERM.
    # A second die would abandon that END block before cleanup masks the signals.
    # The interpreter phase is established before the first opcode of any END block.
    return if ${^GLOBAL_PHASE} eq 'END';
    die $_[0];
}
$SIG{TERM} = sub { interrupt("fixture cancelled\n") };
$SIG{INT} = sub { interrupt("fixture interrupted\n") };
$SIG{HUP} = sub { interrupt("fixture disconnected\n") };
$SIG{ALRM} = sub { interrupt("fixture deadline\n") };
$SIG{PIPE} = sub { interrupt("fixture pipe closed\n") };
# A process group does not reset the inherited mask. Only this fresh fixture
# and its future children are affected; Drop must be able to deliver TERM.
POSIX::sigprocmask(POSIX::SIG_UNBLOCK(), $wait_signals) or die "unblock fixture signals: $!";
if ($^O eq 'linux') {
    # Confined to this freshly exec'd fixture process, before any fork. syscall.ph is
    # Perl's system-provided syscall mapping; no dependency or runner configuration.
    require 'syscall.ph';
    syscall(&SYS_prctl, 36, 1, 0, 0, 0) == 0 or die "fixture subreaper: $!";
}
alarm 20;
$child = fork() // die "fork: $!";
if (!$child) {
    close STDIN;
    close STDOUT; # only the supervisor may keep the Rust protocol stream open
    close $life_write;
    close $go_write;
    close $ready_read;
    my $worker = 0;
    my $stop = sub {
        # TERM can enter this stop from the protocol path; it must not reenter
        # after the worker wait consumes its identity or before kill reads it.
        local $SIG{TERM} = 'IGNORE';
        if ($worker) {
            kill 'TERM', $worker;
            wait_owned(\$worker);
        }
    };
    $SIG{TERM} = sub { $stop->(); exit 0 };
    my $release = <$go_read>;
    POSIX::_exit(0) unless defined($release) && $release eq "release\n";
    POSIX::setpgid(0, 0) == 0 or die "setpgid: $!" if $mode eq 'grouped';
    POSIX::setsid() >= 0 or die "setsid: $!" if $mode eq 'detached';
    $worker = fork() // die "fork worker: $!";
    if (!$worker) {
        $SIG{TERM} = 'DEFAULT';
        POSIX::setsid() >= 0 or die "worker setsid: $!" if $mode eq 'descendant';
        close $go_read;
        print $ready_write "worker $$\n";
        close $ready_write;
        # TERM from a live intermediate or EOF from the independent supervisor both
        # end this worker. It inherits no writer that could keep its own lifeline open.
        my $byte;
        while (1) {
            my $read = sysread($life_read, $byte, 1);
            next if !defined($read) && $! == EINTR;
            die "worker lifeline: $!" unless defined $read;
            last unless $read;
        }
        POSIX::_exit(0);
    }
    print $ready_write "ready $$\n";
    my $command = <$go_read>;
    die "stop command" unless defined($command) && $command eq "stop\n";
    $stop->();
    print $ready_write "stopped $$\n";
    close $ready_write;
    exit 0;
}
close $go_read;
close $ready_write;
close $life_read;
print "forked $child\n";
while (my $command = <STDIN>) {
    if ($command eq "release\n") {
        print $go_write $command;
        for (1..2) {
            my $event = <$ready_read> // die "missing readiness";
            print $event;
        }
    } elsif ($command eq "stop\n") {
        print $go_write $command;
        my $event = <$ready_read> // die "missing stop";
        print $event;
    } elsif ($command eq "reap\n") {
        my $pid = $child;
        my $status = wait_owned(\$child);
        die "child exit $status" if $status;
        print "reaped $pid\n";
    } else {
        die "unknown command";
    }
}
# END runs on EOF, normal completion, die, TERM and the protocol watchdog alike.
