//! Fixtures do ciclo real dos chamadores; override de ajudante existe só no binário de testes.
#![allow(clippy::unwrap_used)]
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) struct Case {
    pub(crate) dir: PathBuf,
}

/// Isola o modo fictício em outro processo, sem alterar ambiente global dos testes paralelos.
pub(crate) fn case(test: &str) -> Option<Case> {
    if let Some(dir) = std::env::var_os("DONNABOT_MOTOR_CASE_DIR") {
        return Some(Case { dir: dir.into() });
    }
    let dir = std::env::temp_dir().join(format!("motor-cancel-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let helper = dir.join("helper");
    std::fs::write(&helper, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\nif [ \"$1\" = kill-group ]; then /bin/kill -KILL -- -\"$2\" 2>/dev/null || true; exit 0; fi\nif [ \"$1\" = background ] && [ \"$2\" = criar ]; then d=$(/usr/bin/mktemp -d); /bin/mkdir -m 700 \"$d/work\" \"$d/config\" \"$d/data\" \"$d/cache\" \"$d/state\"; printf \"%s\\n\" \"$d\"; exit 0; fi\nif [ \"$1\" = background ] && [ \"$2\" = remover ]; then /bin/rm -rf -- \"$3\"; exit 0; fi\nif [ \"$1\" = --cwd ]; then cd \"$2\" 2>/dev/null || true; fi\nshift 3\nexec \"$@\"\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A raiz de política do subprocesso aponta para uma pasta vazia do caso:
    // o /etc real da máquina não pode derrubar o spawn do motor.
    let policy_root = dir.join("policy-root");
    std::fs::create_dir(&policy_root).unwrap();
    let node = crate::agent::backends::which("node");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    if let Some(node) = node {
        command.env("DONNABOT_MEX_NODE_TEST", node);
    }
    let status = command
        .args(["--exact", test, "--nocapture"])
        .env("DONNABOT_MOTOR_CASE_DIR", &dir)
        .env("DONNABOT_MOTOR_TEST_HELPER", &helper)
        .env("DONNABOT_TEST_POLICY_ROOT", &policy_root)
        .env("MEXERICA_MOTOR", "1")
        .env("HOME", "/tmp")
        .env("MEXERICA_HOME", "/tmp")
        .status()
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(status.success());
    None
}

impl Case {
    pub(crate) fn fixture(&self, name: &str, hang: bool) -> PathBuf {
        let path = self.dir.join(name);
        let code = r"#!/usr/bin/python3
import json, os, subprocess, sys, time
from pathlib import Path
child = subprocess.Popen(['/bin/sleep', '120'])
Path(sys.argv[0]+'.pid').write_text(json.dumps([os.getpid(), child.pid, os.getuid(), os.getpgrp()]))
if HANG:
    time.sleep(120)
for line in sys.stdin:
    msg = json.loads(line)
    if 'id' not in msg:
        continue
    method = msg.get('method')
    if method == 'initialize':
        result = {'protocolVersion':'2025-03-26','capabilities':{}}
    elif method == 'account/read':
        result = {'account':{'type':'chatgpt','email':'fixture@example.invalid','planType':'pro'}}
    elif method == 'model/list':
        result = {'data':[{'id':'gpt-fixture','supportedReasoningEfforts':[]}]}
    else:
        result = {'tools':[]}
    print(json.dumps({'jsonrpc':'2.0','id':msg['id'],'result':result}),flush=True)
time.sleep(120)
"
        .replace("HANG", if hang { "True" } else { "False" });
        std::fs::write(&path, code).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    pub(crate) async fn started(&self, path: &Path) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !path.with_extension("pid").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    pub(crate) async fn stopped(&self, path: &Path) {
        self.started(path).await;
        let values: Vec<u32> = serde_json::from_slice(&std::fs::read(path.with_extension("pid")).unwrap()).unwrap();
        // SAFETY: getter sem efeitos colaterais; no ensaio root o esperado vem do usuário descartável.
        let expected = std::env::var("DONNABOT_MOTOR_EXPECT_UID")
            .ok()
            .map_or_else(|| unsafe { libc::getuid() }, |v| v.parse().unwrap());
        assert_eq!(values[2], expected, "identidade da fixture");
        if let Some(helper) = std::env::var_os("DONNABOT_MOTOR_TEST_HELPER") {
            let calls = std::fs::read_to_string(format!("{}.calls", PathBuf::from(helper).display())).unwrap();
            assert!(
                calls.lines().any(|line| line == format!("kill-group {}", values[3])),
                "kill pelo ajudante: {calls}"
            );
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let alive = values[..2].iter().any(|pid| {
                    std::fs::read_to_string(format!("/proc/{pid}/status"))
                        .is_ok_and(|s| !s.lines().any(|l| l.starts_with("State:\tZ") || l.starts_with("State:\tX")))
                });
                if !alive {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn motor_timeout_output_encerra_grupo() {
    let Some(case) = case("fork::motor_exec::test_support::motor_timeout_output_encerra_grupo") else { return };
    let fixture = case.fixture("usage-hang", true);
    let mut command = super::command(&fixture, Path::new("/tmp"));
    assert!(tokio::time::timeout(Duration::from_secs(1), super::output(&mut command)).await.is_err());
    case.stopped(&fixture).await;
}
