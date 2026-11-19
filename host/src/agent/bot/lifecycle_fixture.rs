//! First-party process fixture for lifecycle tests. Every observation is scoped to its own PIDs.
use super::{background_work, session};
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::time::{Instant, sleep, timeout, timeout_at};

const DEADLINE: Duration = Duration::from_secs(5);

// A separate process owns the synthetic tree. In particular, neither the Hub nor this test
// process becomes a subreaper. Keep a synchronous Child so unwinding/cancelling a future also
// waits for the supervisor's final cleanup, even after the Tokio runtime has stopped polling.
struct Supervisor {
    child: std::process::Child,
    pid: Option<u32>,
}

impl Supervisor {
    fn new(child: std::process::Child) -> Self {
        Self { pid: Some(child.id()), child }
    }

    fn id(&self) -> Option<u32> {
        self.pid
    }

    async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.pid = None;
                return Ok(status);
            }
            sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            // An unreaped direct child cannot have its PID reused. Only this handle waits for
            // it. TERM interrupts protocol reads and runs the Perl owner's END cleanup.
            // SAFETY: this PID is still our unreaped direct child, never a group or a lookup.
            unsafe { libc::kill(libc::pid_t::try_from(self.child.id()).expect("owned PID"), libc::SIGTERM) };
        }
        // No async reaper or outer test supervisor is needed to finish fixture cleanup.
        if let Err(error) = self.child.wait() {
            eprintln!("synthetic supervisor reap failed: {error}");
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct Process {
    pid: u32,
    ppid: u32,
    sid: i32,
    pgid: u32,
    start: String,
    state: String,
}

#[cfg(target_os = "linux")]
fn process(pid: u32) -> Option<Process> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = stat[stat.rfind(')')? + 2..].split_whitespace().collect();
    Some(Process {
        pid,
        ppid: fields.get(1)?.parse().ok()?,
        sid: fields.get(3)?.parse().ok()?,
        pgid: fields.get(2)?.parse().ok()?,
        start: fields.get(19)?.to_string(), // /proc stat field22, clock ticks since boot.
        state: fields.first()?.to_string(),
    })
}

#[cfg(not(target_os = "linux"))]
fn process(pid: u32) -> Option<Process> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid=,ppid=,pgid=,lstart=,stat="])
        .output()
        .expect("read only the synthetic PID");
    let text = String::from_utf8(output.stdout).expect("ps output");
    let fields: Vec<_> = text.split_whitespace().collect();
    Some(Process {
        pid: fields.first()?.parse().ok()?,
        ppid: fields.get(1)?.parse().ok()?,
        sid: session(pid)?,
        pgid: fields.get(2)?.parse().ok()?,
        start: fields.get(3..8)?.join(" "),
        state: fields.get(8)?.to_string(),
    })
}

pub(super) struct ProcessTree {
    pub mode: &'static str,
    // Fields drop in declaration order: keep stdin open until Supervisor has
    // signalled/waited. Closing it first races EOF's END with the same TERM.
    parent: Supervisor,
    input: Option<ChildStdin>,
    output: Lines<BufReader<ChildStdout>>,
    pids: Vec<u32>,
    starts: Vec<String>,
    deadline: Instant,
}

impl ProcessTree {
    pub async fn spawn(mode: &'static str) -> Self {
        let mut command = std::process::Command::new("perl");
        command.args([concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/lifecycle_process.pl"), mode]);
        command.stderr(Stdio::inherit());
        Self::spawn_command(mode, command).await
    }

    async fn spawn_command(mode: &'static str, mut command: std::process::Command) -> Self {
        let deadline = Instant::now() + DEADLINE;
        let mut parent = Supervisor::new(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .process_group(0)
                .spawn()
                .expect("synthetic Perl process runs"),
        );
        // The supervisor guard already owns cleanup if converting either pipe fails.
        let input = Some(ChildStdin::from_std(parent.child.stdin.take().expect("stdin")).expect("async stdin"));
        let output =
            BufReader::new(ChildStdout::from_std(parent.child.stdout.take().expect("stdout")).expect("async stdout"))
                .lines();
        let pid = parent.id().expect("parent PID");
        let mut tree = Self { mode, parent, input, output, pids: vec![pid], starts: Vec::new(), deadline };
        let (event, child) = tree.event().await;
        assert_eq!(event, "forked");
        tree.pids.push(child);
        for &pid in &tree.pids {
            tree.starts.push(process(pid).expect("live synthetic process").start);
        }
        tree.check(false);
        tree
    }

    async fn event(&mut self) -> (String, u32) {
        let line = timeout_at(self.deadline, self.output.next_line())
            .await
            .expect("fixture deadline")
            .expect("read event")
            .expect("fixture stays open until cancellation");
        let (event, pid) = line.split_once(' ').expect("event and PID");
        (event.to_owned(), pid.parse().expect("PID"))
    }

    async fn send(&mut self, command: &str) {
        timeout_at(self.deadline, self.input.as_mut().expect("stdin").write_all(command.as_bytes()))
            .await
            .expect("command deadline")
            .expect("command delivered");
    }

    pub async fn ready(&mut self) {
        self.send("release\n").await;
        let events = [self.event().await, self.event().await];
        assert!(events.contains(&("ready".into(), self.pids[1])));
        let worker = events.iter().find(|(event, _)| event == "worker").expect("worker readiness").1;
        assert!(!self.pids.contains(&worker));
        self.pids.push(worker);
        self.starts.push(process(worker).expect("live worker").start);
        self.check(true);
    }

    fn check(&self, ready: bool) {
        let rows = self.rows();
        assert_eq!(rows.len(), self.pids.len());
        for (row, start) in rows.iter().zip(&self.starts) {
            assert_eq!(&row.start, start, "same process identity");
            assert!(!row.state.starts_with('Z'), "readiness requires live work");
            assert_eq!(session(row.pid), Some(row.sid));
        }
        let parent = &rows[0];
        let child = &rows[1];
        assert_eq!(parent.pgid, parent.pid);
        assert_eq!(child.ppid, parent.pid);
        let sid = if ready && self.mode == "detached" { i32::try_from(child.pid).expect("SID") } else { parent.sid };
        let pgid = if ready && self.mode != "plain" { child.pid } else { parent.pid };
        assert_eq!(child.sid, sid);
        assert_eq!(child.pgid, if ready && self.mode == "descendant" { parent.pid } else { pgid });
        if ready {
            let worker = &rows[2];
            assert_eq!(worker.ppid, child.pid);
            let worker_detached = self.mode == "descendant";
            assert_eq!(worker.sid, if worker_detached { i32::try_from(worker.pid).expect("SID") } else { sid });
            assert_eq!(worker.pgid, if worker_detached { worker.pid } else { pgid });
            assert_eq!(self.mode == "detached", child.sid != parent.sid);
        }
    }

    fn rows(&self) -> Vec<Process> {
        self.pids.iter().filter_map(|&pid| process(pid)).collect()
    }

    pub async fn background(&self, phase: &str) -> bool {
        let rows = self.rows();
        let busy = background_work(self.parent.id()).await;
        eprintln!(
            "LIFECYCLE_SYNTHETIC {}",
            serde_json::json!({
                "mode": self.mode, "phase": phase, "rows": rows, "background_work": busy
            })
        );
        busy
    }

    pub async fn held(&mut self) {
        // Adversarial control: no readiness can occur until release. This exceeds the old300ms
        // without relying on a scheduler delay or extending the actual readiness deadline.
        assert!(timeout(Duration::from_millis(350), self.output.next_line()).await.is_err());
        self.check(false);
        assert!(!self.background("held_before_setsid").await);
    }

    pub async fn finish(mut self) {
        self.deadline = Instant::now() + DEADLINE;
        self.send("stop\n").await;
        assert_eq!(self.event().await, ("stopped".into(), self.pids[1]));
        timeout(DEADLINE, async {
            loop {
                if process(self.pids[1]).is_some_and(|p| p.state.starts_with('Z')) {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("child exited, deliberately not reaped yet");
        assert!(process(self.pids[2]).is_none(), "worker was reaped by its parent");
        let _ = self.background("zombie_before_reap").await;
        self.send("reap\n").await;
        assert_eq!(self.event().await, ("reaped".into(), self.pids[1]));
        assert!(process(self.pids[1]).is_none());
        assert!(!self.background("reaped_idle_parent").await);
        self.cancel().await;
    }

    pub async fn cancel(mut self) {
        // EOF is an explicit cancellation command: the parent terminates/reaps its descendants.
        self.input.take();
        assert!(timeout(DEADLINE, self.parent.wait()).await.expect("cancel deadline").expect("reap parent").success());
        assert!(self.pids.iter().all(|&pid| process(pid).is_none()), "every synthetic PID was reaped");
        eprintln!(
            "LIFECYCLE_SYNTHETIC {}",
            serde_json::json!({"mode": self.mode, "phase": "cancelled_and_reaped", "pids": self.pids})
        );
    }
}

#[cfg(target_os = "linux")]
mod cleanup_tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    // Force TERM in END before cleanup's local IGNOREs. Prove both the pending
    // request and its actual handler invocation, even with an inherited mask.
    const END_SIGNAL_PROBE: &str = r#"
use strict;
use warnings;
use POSIX ();
our $lifecycle_before_cleanup = sub {
    die "probe outside END" unless ${^GLOBAL_PHASE} eq 'END';
    print STDERR "END_SIGNAL_PROBE entered\n";
    my $term = POSIX::SigSet->new(POSIX::SIGTERM());
    my $previous = POSIX::SigSet->new();
    POSIX::sigprocmask(POSIX::SIG_BLOCK(), $term, $previous) or die "probe block TERM: $!";
    my $handler = $SIG{TERM};
    die "probe requires real TERM handler" unless ref($handler) eq 'CODE';
    my ($requested, $acknowledged) = (0, 0);
    # Reinstall the real fixture handler: leaving RUN can reset the kernel
    # disposition while %SIG still contains the Perl closure. Do not replace
    # its behavior: a die in the original handler must still fail this probe.
    local $SIG{TERM} = sub {
        $handler->(@_);
        if ($requested && ${^GLOBAL_PHASE} eq 'END' && $_[0] eq 'TERM') {
            $acknowledged = 1;
            print STDERR "END_SIGNAL_PROBE handler_ack TERM request=1\n";
        }
    };
    kill('TERM', $$) == 1 or die "probe TERM: $!";
    my $pending = POSIX::SigSet->new();
    POSIX::sigpending($pending) or die "probe pending TERM: $!";
    die "probe TERM not pending" unless $pending->ismember(POSIX::SIGTERM());
    print STDERR "END_SIGNAL_PROBE queued TERM request=1\n";
    # Arm only after our real kill has queued TERM under the mask. A prior
    # Drop notification cannot acknowledge this request. Standard signals may
    # coalesce; at least our own queued TERM must reach the real handler here.
    $requested = 1;
    POSIX::sigprocmask(POSIX::SIG_UNBLOCK(), $term) or die "probe unblock TERM: $!";
    my $current = POSIX::SigSet->new();
    POSIX::sigprocmask(POSIX::SIG_BLOCK(), POSIX::SigSet->new(), $current) or die "probe read mask: $!";
    die "probe TERM still blocked" if $current->ismember(POSIX::SIGTERM());
    die "probe TERM handler not acknowledged" unless $acknowledged;
    $requested = 0;
    print STDERR "END_SIGNAL_PROBE resumed\n";
    POSIX::sigprocmask(POSIX::SIG_SETMASK(), $previous) or die "probe restore mask: $!";
};
my $path = shift @ARGV;
die "probe expects fixture path and one mode" unless defined($path) && @ARGV == 1;
do $path; die $@ if $@;
"#;

    async fn spawn_cleanup_tree(mode: &'static str) -> (ProcessTree, tokio::process::ChildStderr) {
        let mut command = std::process::Command::new("perl");
        command.args([
            "-e",
            END_SIGNAL_PROBE,
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/lifecycle_process.pl"),
            mode,
        ]);
        command.stderr(Stdio::piped());
        let mut tree = ProcessTree::spawn_command(mode, command).await;
        let errors = tokio::process::ChildStderr::from_std(tree.parent.child.stderr.take().expect("probe stderr"))
            .expect("async probe stderr");
        (tree, errors)
    }

    async fn assert_cleanup_probe(mut errors: tokio::process::ChildStderr) {
        let mut observed = String::new();
        timeout(DEADLINE, errors.read_to_string(&mut observed))
            .await
            .expect("probe EOF deadline")
            .expect("probe output");
        eprintln!("LIFECYCLE_CLEANUP_PROBE {observed}");
        assert!(observed.contains("END_SIGNAL_PROBE entered"), "cleanup entry was exercised: {observed}");
        assert!(observed.contains("END_SIGNAL_PROBE resumed"), "TERM must not abandon END: {observed}");
        let entered = observed.find("END_SIGNAL_PROBE entered").expect("probe entry");
        let queued = observed.find("END_SIGNAL_PROBE queued TERM request=1").expect("our TERM request queued");
        let acknowledged = observed
            .find("END_SIGNAL_PROBE handler_ack TERM request=1")
            .expect("real END handler acknowledged request");
        let resumed = observed.find("END_SIGNAL_PROBE resumed").expect("probe resumed");
        assert!(entered < queued && queued < acknowledged && acknowledged < resumed, "probe event order: {observed}");
    }

    fn assert_reaped(rows: &[Process]) {
        for original in rows {
            assert!(
                process(original.pid).is_none_or(|now| now.start != original.start),
                "synthetic identity still alive or unreaped: {original:?}"
            );
        }
    }

    #[tokio::test]
    async fn killed_intermediate_is_cleaned_before_and_after_worker_readiness() {
        for ready in [false, true] {
            for path in ["finish", "cancel", "drop", "abort"] {
                let (mut tree, errors) = spawn_cleanup_tree("detached").await;
                if ready {
                    tree.ready().await;
                    assert!(tree.background("live_before_intermediate_kill").await);
                }
                let rows = tree.rows();
                assert_eq!(rows.len(), if ready { 3 } else { 2 });
                // The supervisor deliberately has not received reap/EOF, so the child is
                // still its unreaped child and cannot be replaced by a reused PID.
                assert_eq!(process(tree.pids[1]).expect("owned child").start, tree.starts[1]);
                assert_eq!(
                    // SAFETY: only that synthetic, unreaped intermediate is signalled.
                    unsafe { libc::kill(libc::pid_t::try_from(tree.pids[1]).expect("owned PID"), libc::SIGKILL) },
                    0
                );
                match path {
                    "finish" => {
                        let task = tokio::spawn(async move { tree.finish().await });
                        assert!(task.await.expect_err("missing stop must fail").is_panic());
                    }
                    "cancel" => tree.cancel().await,
                    "drop" => drop(tree),
                    "abort" => {
                        let task = tokio::spawn(async move {
                            let _tree = tree;
                            std::future::pending::<()>().await;
                        });
                        task.abort();
                        assert!(task.await.expect_err("cancelled fixture future").is_cancelled());
                    }
                    _ => unreachable!(),
                }
                // No external supervisor, global reap or delayed cleanup makes this pass.
                assert_reaped(&rows);
                assert_cleanup_probe(errors).await;
            }
        }
    }

    #[tokio::test]
    async fn readiness_failures_and_panic_reap_the_synthetic_tree() {
        for failure in ["eof", "timeout", "false_handshake", "panic"] {
            let (mut tree, errors) =
                spawn_cleanup_tree(if failure == "false_handshake" { "plain" } else { "detached" }).await;
            if failure == "panic" {
                tree.ready().await;
            }
            let rows = tree.rows();
            let task = tokio::spawn(async move {
                match failure {
                    "eof" => {
                        tree.input.take();
                        tree.event().await;
                    }
                    "timeout" => {
                        // Shrink this negative control's absolute deadline, never renew it.
                        tree.deadline = Instant::now() + Duration::from_millis(100);
                        tree.event().await;
                    }
                    "false_handshake" => {
                        tree.mode = "detached";
                        tree.ready().await;
                    }
                    "panic" => panic!("synthetic fixture unwind after real readiness"),
                    _ => unreachable!(),
                }
            });
            assert!(task.await.expect_err("protocol failure must stay a failure").is_panic());
            assert_reaped(&rows);
            assert_cleanup_probe(errors).await;
        }
    }
}

#[cfg(target_os = "linux")]
mod ownership_tests {
    use super::*;
    use std::io::Read;

    // First-party fault/observation wrapper: every positive wait is the real kernel wait.
    // Injected EINTR owns an unreaped child; ECHILD follows an actual consuming wait.
    // A signal at that return boundary must not run cleanup with the consumed identity.
    const OBSERVE: &str = r#"
use strict;
use warnings;
use Errno qw(ECHILD EINTR);
our ($case, $owner, $injected, %consumed);
BEGIN {
    *CORE::GLOBAL::waitpid = sub {
        my ($pid, $flags) = @_;
        my $target = $pid > 0 && ($$ == $owner || $case eq 'worker_TERM');
        if ($target && $$ == $owner && $case eq 'eintr' && !$injected++) {
            print STDERR "PROBE EINTR\n";
            $! = EINTR; return -1;
        }
        my $got = CORE::waitpid($pid, $flags);
        my ($status, $error) = ($?, 0 + $!);
        if ($got == 0 && $target && $$ == $owner && $case eq 'live_TERM' && !$injected++) {
            print STDERR "PROBE signal live TERM\n";
            CORE::kill('TERM', $$);
        }
        if ($got > 0) {
            $consumed{$got} = 1;
            print STDERR "PROBE consumed $$ $got\n";
            if ($target && !$injected++) {
                if ($case eq 'echild') {
                    print STDERR "PROBE ECHILD\n";
                    $! = ECHILD; return -1;
                }
                if ($case =~ /^(TERM|INT|HUP|ALRM|PIPE)$/ || ($case eq 'worker_TERM' && $$ != $owner)) {
                    my $signal = $case eq 'worker_TERM' ? 'TERM' : $case;
                    print STDERR "PROBE signal $signal\n";
                    CORE::kill($signal, $$);
                }
            }
        }
        $? = $status; $! = $error; return $got;
    };
    *CORE::GLOBAL::exit = sub {
        my ($status) = @_;
        if ($$ != $owner && $case eq 'nonzero') {
            print STDERR "PROBE exit7 after stopped\n";
            CORE::exit(7);
        }
        CORE::exit($status);
    };
    *CORE::GLOBAL::kill = sub {
        my ($signal, @pids) = @_;
        for my $pid (@pids) {
            print STDERR "STALE_SIGNAL $$ $pid\n" if $consumed{$pid};
            die "nonpositive signal target" unless $pid > 0;
        }
        return CORE::kill($signal, @pids);
    };
}
my $path = shift @ARGV;
$case = pop @ARGV; $owner = $$; $injected = 0;
do $path; die $@ if $@;
"#;

    #[tokio::test]
    async fn consumed_identity_is_not_signalled_on_error_or_interruption() {
        let sentinel = Supervisor::new(
            std::process::Command::new("perl").args(["-e", "sleep 20"]).process_group(0).spawn().expect("own sentinel"),
        );
        let sentinel_row = process(sentinel.id().expect("sentinel PID")).expect("live sentinel");
        for case in [
            "normal",
            "nonzero",
            "eintr",
            "echild",
            "already_reaped",
            "live_TERM",
            "TERM",
            "INT",
            "HUP",
            "ALRM",
            "PIPE",
            "worker_TERM",
        ] {
            let mut command = std::process::Command::new("perl");
            command.args([
                "-e",
                OBSERVE,
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/lifecycle_process.pl"),
                "detached",
                case,
            ]);
            command.stderr(Stdio::piped());
            let mut tree = ProcessTree::spawn_command("detached", command).await;
            let mut errors = tree.parent.child.stderr.take().expect("probe stderr");
            tree.ready().await;
            let rows = tree.rows();
            if case != "live_TERM" {
                tree.send("stop\n").await;
                assert_eq!(tree.event().await, ("stopped".into(), tree.pids[1]));
            }
            if case == "already_reaped" {
                tree.send("reap\n").await;
                assert_eq!(tree.event().await, ("reaped".into(), tree.pids[1]));
            }
            tree.send("reap\n").await;
            let succeeds = matches!(case, "normal" | "eintr" | "worker_TERM");
            if succeeds {
                assert_eq!(tree.event().await, ("reaped".into(), tree.pids[1]));
                tree.input.take();
            }
            let status =
                timeout(DEADLINE, tree.parent.wait()).await.expect("reap deadline").expect("supervisor status");
            assert_eq!(status.success(), succeeds, "nonzero/protocol failure must remain failure: {case}");
            assert!(
                rows.iter().all(|old| process(old.pid).is_none_or(|now| now.start != old.start)),
                "own tree not reaped: {case}"
            );
            let mut observed = String::new();
            errors.read_to_string(&mut observed).expect("probe output after exit");
            eprintln!("OWNERSHIP_PROBE {case} {status}: {observed}");
            assert!(!observed.contains("STALE_SIGNAL"), "signal after consuming identity: {case}: {observed}");
            assert!(observed.contains("PROBE consumed"), "real wait observed: {case}");
            if case == "nonzero" {
                assert!(observed.contains("PROBE exit7 after stopped"));
            }
            if case == "live_TERM" {
                assert!(observed.contains("PROBE signal live TERM"));
            }
            if case == "eintr" {
                assert!(observed.contains("PROBE EINTR"));
            }
            if case == "echild" {
                assert!(observed.contains("PROBE ECHILD"));
            }
            if matches!(case, "TERM" | "INT" | "HUP" | "ALRM" | "PIPE" | "worker_TERM") {
                assert!(observed.contains("PROBE signal"));
            }
            let neighbor = process(sentinel_row.pid).expect("sentinel intact");
            assert_eq!(neighbor.start, sentinel_row.start);
            assert!(!neighbor.state.starts_with('Z'), "neighbor remains alive");
        }
        drop(sentinel);
        assert!(process(sentinel_row.pid).is_none(), "own sentinel reaped by its handle");
    }
}
