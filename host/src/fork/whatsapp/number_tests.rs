#![allow(clippy::unwrap_used)]
//! D70 do R2 com um wacli falso (script `sh`): pareamento por código e QR, vencer,
//! cancelar, supervisor, desligar/pausar/apagar, `logged_out`, sem binário e autoridade.
//! Nada de rede, número ou wacli real.
use super::api::call;
use crate::{
    LockExt,
    api::devices::Caller,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Scope, Store},
};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc, time::Duration};

const HUB: &str = "00000000-0000-4000-8000-0000000000aa";
/// IDs no formato real (UUID): o registro de plugins só aceita esse formato.
pub(super) const DONNA: &str = "00000000-0000-4000-8000-0000000000d1";
pub(super) const LIA: &str = "00000000-0000-4000-8000-0000000000d2";

const FAKE: &str = r#"#!/bin/sh
here=$(cd "$(dirname "$0")" && pwd)
store=""
while [ $# -gt 0 ]; do
  case "$1" in
    --store) store=$2; shift 2 ;;
    --events|--read-only|--json) shift ;;
    --timeout) shift 2 ;;
    *) break ;;
  esac
done
echo "$store $1" >> "$here/calls"
mode=$(cat "$here/mode" 2>/dev/null)
ev() { printf '{"event":"%s","data":%s}\n' "$1" "$2" >&2; }
if [ "$1" = auth ] && [ "$2" = status ]; then
  # Formato do wacli v0.25.1 (internal/out/out.go:9-25; cmd/wacli/auth.go:301-311).
  if [ -f "$store/session.db" ]; then
    echo '{"success":true,"data":{"authenticated":true,"linked_jid":"5511900000000@s.whatsapp.net","phone":"5511900000000"},"error":null}'
  else echo '{"success":true,"data":{"authenticated":false},"error":null}'; fi
  exit 0
fi
if [ "$1" = media ] && [ "$2" = download ]; then
  # Formato do wacli v0.25.1 (cmd/wacli/media_download.go:18-30; internal/out/out.go:9-25).
  out=""; while [ $# -gt 0 ]; do [ "$1" = --output ] && out=$2; shift; done
  echo media >> "$here/media"
  [ -f "$here/media-fails" ] && exit 1
  printf 'OggS-voz-de-teste' > "$out"; n=$(wc -c < "$out" | tr -d ' ')
  printf '{"success":true,"data":{"path":"%s","bytes":%s,"media_type":"audio","mime_type":"audio/ogg; codecs=opus","downloaded":true,"source":"server","read_only":true,"recorded":false},"error":null}\n' "$out" "$n"
  exit 0
fi
if [ "$1" = auth ] && [ "$2" = logout ]; then
  [ -f "$here/logout-fails" ] && exit 1
  rm -f "$store/session.db"; echo logout >> "$here/logouts"; exit 0
fi
if [ "$1" = chats ] && [ "$2" = cleanup ]; then
  # R8: precisa da trava do store (cmd/wacli/root.go:120-128); com o sync de pé, recusa.
  pid=$(cat "$store/fake-sync.pid" 2>/dev/null)
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then exit 1; fi
  shift 2; echo "$*" >> "$here/cleanups"; exit 0
fi
if [ "$1" = sync ]; then
  echo $$ > "$store/fake-sync.pid"; echo sync >> "$here/syncs"; echo "$@" > "$store/fake-sync.args"
  if [ "$mode" = silent ]; then exec sleep 600; fi
  ev connected '{}'
  if [ "$mode" = logged-out ]; then ev logged_out '{}'; exit 0; fi
  if [ "$mode" = stale ]; then sleep 0.5; ev stale '{}'; fi
  exec sleep 600
fi
if [ "$1" = auth ]; then
  echo $$ > "$store/fake-auth.pid"
  if [ "$2" = --phone ]; then ev pair_code '{"phone":"x","code":"ABCD-EFGH"}'
  else ev qr_code '{"code":"QR-1"}'; sleep 0.3; ev qr_code '{"code":"QR-2"}'; fi
  case "$mode" in
    expire) echo 'QR code timed out; run wacli auth again' >&2; exit 1 ;;
    refuse) ev error '{"message":"passkey not supported"}'; exit 1 ;;
    hang) exec sleep 600 ;;
  esac
  sleep 0.3; : > "$store/session.db"; ev connected '{}'; exit 0
fi
if [ "$1" = send ]; then
  # R4c: `send text --json` do wacli v0.25.1 (cmd/wacli/send.go:146-151, internal/out/out.go).
  printf '%s\n' "$*" >> "$here/sends"
  case "$(cat "$here/send-mode" 2>/dev/null)" in
    fail) echo '{"success":false,"data":null,"error":"boom"}'; exit 1 ;;
    garbage) echo 'Sent?'; exit 0 ;;
    *) echo '{"success":true,"data":{"sent":true,"to":"x","id":"3EB0FAKE"},"error":null}'; exit 0 ;;
  esac
fi
exit 2
"#;

pub(super) struct Fixture {
    pub(super) hub: Arc<Hub>,
    pub(super) temp: PathBuf,
}

impl Fixture {
    pub(super) fn new(with_binary: bool) -> Self {
        Self::create(with_binary, false)
    }

    /// Como `new`, com o banco do Hub em arquivo, para reiniciar de verdade (`restart`).
    pub(super) fn on_disk(with_binary: bool) -> Self {
        Self::create(with_binary, true)
    }

    fn create(with_binary: bool, on_disk: bool) -> Self {
        let temp = std::env::temp_dir().join(format!("donnabot-whatsapp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(temp.join("fake")).unwrap();
        let path = if on_disk { temp.join("hub.db") } else { PathBuf::from(":memory:") };
        let store = Store::open(&path).unwrap();
        for id in [DONNA, LIA] {
            let cfg: BotConfig =
                serde_json::from_value(json!({"id":id,"name":id,"backend":"fixture","cwd":"/synthetic"})).unwrap();
            store.save_bot(&cfg).unwrap();
        }
        store.set_team_settings(&json!({"primaryBotId":DONNA})).unwrap();
        Self::boot(temp, store, with_binary)
    }

    /// Reinício de verdade: para o `WhatsApp`, larga o Hub e abre outro sobre o mesmo banco.
    pub(super) async fn restart(self) -> Self {
        self.hub.whatsapp.shutdown().await;
        let Self { hub, temp } = self;
        let with_binary = hub.whatsapp.binary().is_some();
        drop(hub);
        let store = Store::open(&temp.join("hub.db")).unwrap();
        Self::boot(temp, store, with_binary)
    }

    fn boot(temp: PathBuf, store: Store, with_binary: bool) -> Self {
        let identity = Identity::load_or_create(&temp).unwrap();
        let hub = Hub::new(store, HUB.into(), identity, "t".into(), 0);
        let bin = temp.join("fake").join("wacli");
        crate::fork::test_support::write_executable(&bin, FAKE.as_bytes(), 0o755);
        hub.whatsapp.set_paths(temp.join("whatsapp"), with_binary.then_some(bin));
        super::outbox::start(&hub);
        Self { hub, temp }
    }

    pub(super) async fn call(&self, method: &str, body: Value) -> anyhow::Result<Value> {
        call(&self.hub, &Caller::Local, method, body).await
    }

    fn mode(&self, mode: &str) {
        std::fs::write(self.temp.join("fake/mode"), mode).unwrap();
    }

    pub(super) fn dir(&self, bot: &str) -> PathBuf {
        self.temp.join("whatsapp").join(bot)
    }

    pub(super) fn lines(&self, name: &str) -> usize {
        std::fs::read_to_string(self.temp.join("fake").join(name)).map_or(0, |s| s.lines().count())
    }

    fn pid(&self, bot: &str, name: &str) -> String {
        std::fs::read_to_string(self.dir(bot).join(name)).unwrap().trim().to_owned()
    }

    /// Lê o número do bot até a condição valer (o wacli falso é assíncrono).
    pub(super) async fn until(&self, bot: &str, check: impl Fn(&Value) -> bool) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let view = self.call("donnaWhatsapp", json!({})).await.unwrap();
            let number = view["bots"].as_array().unwrap().iter().find(|b| b["botId"] == bot).unwrap()["number"].clone();
            if check(&number) {
                return number;
            }
            assert!(tokio::time::Instant::now() < deadline, "estado esperado não chegou: {number}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub(super) async fn connect(&self, bot: &str) {
        self.call("donnaWhatsappPair", json!({"botId":bot,"method":"code","phone":"+55 (11) 90000-0000"}))
            .await
            .unwrap();
        self.until(bot, |n| n["state"] == "connected").await;
    }

    pub(super) async fn done(self) {
        self.hub.whatsapp.shutdown().await;
        let _ = std::fs::remove_dir_all(&self.temp);
    }
}

fn number<'a>(view: &'a Value, bot: &str) -> &'a Value {
    &view["bots"].as_array().unwrap().iter().find(|b| b["botId"] == bot).unwrap()["number"]
}

fn alive(pid: &str) -> bool {
    std::process::Command::new("kill").args(["-0", pid]).status().unwrap().success()
}

async fn gone(pid: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(tokio::time::Instant::now() < deadline, "processo {pid} ficou de pé");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn code_pairing_connects_starts_the_supervisor_and_stops_with_the_plugin_or_the_hub() {
    let f = Fixture::new(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    let view = f
        .call("donnaWhatsappPair", json!({"botId":DONNA,"method":"code","phone":"+55 (11) 90000-0000"}))
        .await
        .unwrap();
    assert_eq!(number(&view, DONNA)["state"], "pairing");
    assert_eq!(number(&view, DONNA)["pairing"], json!({"method":"code","result":"waiting","code":"ABCD-EFGH"}));
    let connected = f.until(DONNA, |n| n["state"] == "connected").await;
    assert_eq!(connected["pairing"], json!({"method":"code","result":"connected"}), "código some depois");
    // R3: o número vem do `auth status` do wacli depois de conectar.
    f.until(DONNA, |n| n["phone"] == "+5511900000000").await;
    let mode = std::fs::metadata(f.dir(DONNA)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let calls = std::fs::read_to_string(f.temp.join("fake/calls")).unwrap();
    assert!(calls.lines().all(|l| l.starts_with(&*f.dir(DONNA).to_string_lossy())), "--store sempre explícito");

    // Desligar o plugin para o processo; religar sobe de novo (o número continua conectado).
    let sync = f.pid(DONNA, "fake-sync.pid");
    f.call("donnaWhatsappSave", json!({"enabled":false})).await.unwrap();
    f.until(DONNA, |n| n["state"] == "stopped").await;
    gone(&sync).await;
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.until(DONNA, |n| n["state"] == "connected").await;

    // Hub parando: nenhum wacli fica para trás.
    let sync = f.pid(DONNA, "fake-sync.pid");
    assert!(alive(&sync));
    f.hub.whatsapp.shutdown().await;
    assert!(!alive(&sync));
    f.done().await;
}

#[tokio::test]
async fn qr_pairing_renews_and_cancel_kills_the_process() {
    let f = Fixture::new(true);
    f.mode("hang");
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    let view = f.call("donnaWhatsappPair", json!({"botId":DONNA,"method":"qr"})).await.unwrap();
    assert!(number(&view, DONNA)["pairing"]["qr"].as_str().unwrap().starts_with("QR-"));
    f.until(DONNA, |n| n["pairing"]["qr"] == "QR-2").await;
    let auth = f.pid(DONNA, "fake-auth.pid");
    let view = f.call("donnaWhatsappPairCancel", json!({"botId":DONNA})).await.unwrap();
    assert!(number(&view, DONNA)["pairing"].get("qr").is_none_or(|q| q == "QR-2"));
    let number = f.until(DONNA, |n| n["pairing"]["result"] == "cancelled").await;
    assert_eq!(number, json!({"state":"none","pairing":{"method":"qr","result":"cancelled"}}));
    gone(&auth).await;
    assert_eq!(f.lines("syncs"), 0);
    f.done().await;
}

#[tokio::test]
async fn expired_and_refused_pairings_report_the_result() {
    let f = Fixture::new(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    for (mode, result) in [("expire", "expired"), ("refuse", "refused")] {
        f.mode(mode);
        f.call("donnaWhatsappPair", json!({"botId":DONNA,"method":"code","phone":"5511900000000"})).await.unwrap();
        let number = f.until(DONNA, |n| n["pairing"]["result"] == result).await;
        assert_eq!(number["state"], "none", "{mode}");
        assert!(number["pairing"].get("code").is_none(), "{mode}");
    }
    assert_eq!(f.lines("syncs"), 0);
    f.done().await;
}

#[tokio::test]
async fn logged_out_asks_to_connect_again_without_restarting() {
    let f = Fixture::new(true);
    f.mode("logged-out");
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.call("donnaWhatsappPair", json!({"botId":DONNA,"method":"code","phone":"5511900000000"})).await.unwrap();
    let number = f.until(DONNA, |n| n["state"] == "needs_reconnect").await;
    assert!(number.get("phone").is_none(), "sessão caída não mostra o número antigo");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(f.lines("syncs"), 1, "logout remoto não reinicia o wacli");
    // Conectar de novo é permitido.
    f.mode("");
    f.connect(DONNA).await;
    f.done().await;
}

#[tokio::test]
async fn disconnect_logs_out_and_erase_removes_the_folder() {
    let f = Fixture::new(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.connect(DONNA).await;
    let sync = f.pid(DONNA, "fake-sync.pid");
    let view = f.call("donnaWhatsappDisconnect", json!({"botId":DONNA})).await.unwrap();
    assert_eq!(view["disconnect"], json!({"logout":"done"}));
    assert_eq!(number(&view, DONNA)["state"], "none");
    assert!(!alive(&sync));
    assert_eq!(f.lines("logouts"), 1);
    assert!(f.dir(DONNA).is_dir() && !f.dir(DONNA).join("session.db").exists());

    // Logout falhou: avisa o dono e mesmo assim tira a sessão local.
    f.connect(DONNA).await;
    std::fs::write(f.temp.join("fake/logout-fails"), "").unwrap();
    let view = f.call("donnaWhatsappDisconnect", json!({"botId":DONNA,"erase":true})).await.unwrap();
    assert_eq!(view["disconnect"]["logout"], "failed");
    assert!(view["disconnect"]["warning"].as_str().unwrap().contains("Aparelhos conectados"));
    assert!(!f.dir(DONNA).exists(), "apagar leva sessão, mensagens e mídia");
    let rows: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_whatsapp_numbers", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 0);
    f.done().await;
}

#[tokio::test]
async fn pausing_stops_the_process_and_deleting_the_bot_erases_it() {
    let f = Fixture::new(true);
    f.call("donnaWhatsappSave", json!({"enabled":true,"mode":"per_bot"})).await.unwrap();
    f.connect(LIA).await;
    let sync = f.pid(LIA, "fake-sync.pid");
    f.call("donnaWhatsappSave", json!({"mode":"donna_only"})).await.unwrap();
    f.until(LIA, |n| n["state"] == "stopped").await;
    gone(&sync).await;
    f.call("donnaWhatsappSave", json!({"mode":"per_bot"})).await.unwrap();
    f.until(LIA, |n| n["state"] == "connected").await;
    let sync = f.pid(LIA, "fake-sync.pid");
    f.hub.delete_bot(LIA).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while f.dir(LIA).exists() {
        assert!(tokio::time::Instant::now() < deadline, "pasta do bot apagado ficou");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!alive(&sync));
    assert_eq!(f.lines("logouts"), 1);
    f.done().await;
}

#[tokio::test]
async fn without_the_binary_pairing_is_refused_clearly() {
    let f = Fixture::new(false);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    let err = f.call("donnaWhatsappPair", json!({"botId":DONNA,"method":"qr"})).await.unwrap_err();
    assert!(err.to_string().starts_with("wacli_not_installed"));
    let view = f.call("donnaWhatsapp", json!({})).await.unwrap();
    assert_eq!(number(&view, DONNA), &json!({"state":"not_installed"}));
    assert_eq!(view["plugin"]["wacli"]["installed"], false);
    f.done().await;
}

#[tokio::test]
async fn only_the_owner_pairs_and_only_allowed_bots() {
    let f = Fixture::new(true);
    let pair = json!({"botId":DONNA,"method":"qr"});
    for caller in [
        Caller::Device { key: "executor".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Pairing { key: "pairing".into() },
    ] {
        for method in super::api::NUMBER {
            let err = call(&f.hub, &caller, method, pair.clone()).await.unwrap_err();
            assert!(err.to_string().starts_with("not_allowed"), "{method}");
        }
    }
    let err = f.call("donnaWhatsappPair", pair.clone()).await.unwrap_err();
    assert!(err.to_string().starts_with("plugin_disabled"));
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    let err = f.call("donnaWhatsappPair", json!({"botId":LIA,"method":"qr"})).await.unwrap_err();
    assert!(err.to_string().starts_with("not_allowed"), "Só a Donna pausa os outros bots");
    for bad in [
        json!({"botId":DONNA,"method":"code"}),
        json!({"botId":DONNA,"method":"code","phone":"12"}),
        json!({"botId":DONNA,"method":"qr","phone":"5511900000000"}),
        json!({"botId":"../x","method":"qr"}),
    ] {
        assert!(f.call("donnaWhatsappPair", bad.clone()).await.is_err(), "{bad}");
    }
    assert!(!f.temp.join("whatsapp").exists(), "nada foi aberto");
    f.done().await;
}

/// O telefone que a leitura mostra para o bot (ausente = nenhum).
async fn phone(f: &Fixture) -> Option<String> {
    let view = f.call("donnaWhatsapp", json!({})).await.unwrap();
    number(&view, DONNA).get("phone").and_then(|p| p.as_str()).map(str::to_owned)
}

#[tokio::test]
async fn a_late_phone_from_another_run_or_pairing_is_discarded() {
    let f = Fixture::new(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.connect(DONNA).await;
    f.until(DONNA, |n| n["phone"] == "+5511900000000").await;
    let account = |f: &Fixture| -> String {
        f.hub.store.db.locked().query_row("SELECT account_id FROM donna_whatsapp_numbers", [], |r| r.get(0)).unwrap()
    };
    let (run_a, epoch_a) = f.hub.whatsapp.current(DONNA).unwrap();
    let old = super::number::Confirm { run: run_a, epoch: epoch_a, account: account(&f) };
    // Repareamento: outra conta e outro run do sync.
    f.call("donnaWhatsappDisconnect", json!({"botId":DONNA})).await.unwrap();
    f.connect(DONNA).await;
    f.until(DONNA, |n| n["phone"] == "+5511900000000").await;
    let (run_b, epoch_b) = f.hub.whatsapp.current(DONNA).unwrap();
    assert_ne!(run_a, run_b);
    let late = Some("+5511911112222".to_owned());
    // Resultado atrasado do pareamento A: descartado; B fica intacto.
    assert!(!super::number::apply_phone(&f.hub, DONNA, &old, late.clone()));
    let stale_epoch = super::number::Confirm { run: run_b, epoch: epoch_b + 1, account: account(&f) };
    assert!(!super::number::apply_phone(&f.hub, DONNA, &stale_epoch, late.clone()));
    let other_account = super::number::Confirm { run: run_b, epoch: epoch_b, account: old.account.clone() };
    assert!(!super::number::apply_phone(&f.hub, DONNA, &other_account, late.clone()));
    assert_eq!(phone(&f).await.as_deref(), Some("+5511900000000"));
    let stored: Option<String> =
        f.hub.store.db.locked().query_row("SELECT phone FROM donna_whatsapp_numbers", [], |r| r.get(0)).unwrap();
    assert_eq!(stored.as_deref(), Some("+5511900000000"));
    f.done().await;
}

#[tokio::test]
async fn the_phone_only_shows_while_connected_and_confirmed() {
    let f = Fixture::new(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.connect(DONNA).await;
    f.until(DONNA, |n| n["phone"] == "+5511900000000").await;
    // Troca o modo do wacli falso, derruba o sync e espera o supervisor subir o próximo.
    let restart = async |mode: &str| {
        f.mode(mode);
        let before = f.lines("syncs");
        let pid = f.pid(DONNA, "fake-sync.pid");
        std::process::Command::new("kill").arg(&pid).status().unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while f.lines("syncs") == before {
            assert!(tokio::time::Instant::now() < deadline, "o supervisor não subiu o sync de novo");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    // Caiu (stale): desconectado e sem número.
    restart("stale").await;
    let n = f.until(DONNA, |n| n["state"] == "disconnected").await;
    assert!(n.get("phone").is_none(), "{n}");
    // Voltando sem confirmar (conectando): sem número.
    restart("silent").await;
    let n = f.until(DONNA, |n| n["state"] == "connecting").await;
    assert!(n.get("phone").is_none(), "{n}");
    // Conectado e confirmado de novo: o número volta.
    restart("").await;
    f.until(DONNA, |n| n["phone"] == "+5511900000000").await;
    // Parado (plugin desligado): sem número.
    f.call("donnaWhatsappSave", json!({"enabled":false})).await.unwrap();
    let n = f.until(DONNA, |n| n["state"] == "stopped").await;
    assert!(n.get("phone").is_none(), "{n}");
    f.done().await;
}
