#![allow(clippy::unwrap_used)]
//! D70 do serviço R1 sobre a Connection real: origem/alvo exatos, CAS, replay, entrada final,
//! sentinela e reinício. O percurso MCP/ator real fica em `chat/team/tests.rs`.
use super::*;
use crate::fork::sessoes::{
    origin::SessionOrigin,
    store::{NewSession, create},
};
use crate::hub::Hub;
use crate::remote::identity::Identity;
use crate::store::{BotConfig, EntryKind, Lane};
use serde_json::json;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

const SENTINELA: &str = "SENTINELA-R1-PRIVADA";

pub(super) struct Fixture {
    pub(super) hub: Arc<Hub>,
    dir: PathBuf,
    pub(super) execucao: ExecutionRef,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-retornos-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("r1.db")).unwrap();
        for id in ["a", "b"] {
            let cfg: BotConfig =
                serde_json::from_value(json!({"id": id, "name": id, "backend": "claude", "cwd": dir})).unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        let execucao = bindings::fixture_execution(&hub, "a");
        Self { hub, dir, execucao }
    }

    pub(super) fn entrada(&self, lane: &Lane, kind: EntryKind) -> String {
        let data = json!({"text": format!("{SENTINELA} texto"), "botMessage": {"text": SENTINELA}});
        self.hub.store.insert_entry(lane, kind, 1, &data).unwrap().id
    }

    /// Pedido aceito como na admissão Team; devolve o ID.
    pub(super) fn aceitar(&self) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let origem = self.entrada(&Lane::main("a"), EntryKind::Notice);
        let destino = self.entrada(&Lane::main("b"), EntryKind::Notice);
        self.aceitar_com(&id, &origem, &destino).map(|()| id)
    }

    fn aceitar_com(&self, id: &str, origem: &str, destino: &str) -> Result<()> {
        let recibo = Recibo {
            chain_id: "cadeia".into(),
            admission_id: format!("adm-{id}"),
            run_id: id.into(),
            bot_id: "b".into(),
            level: 2,
            ordinal: 2,
        };
        let pedido = NovoPedido {
            id,
            modo: Modo::Espera,
            de: "a",
            para: "b",
            execucao: &self.execucao,
            recibo: &recibo,
            aviso_origem: origem,
            aviso_destino: destino,
        };
        com_tx(&self.hub.store, |c| aceitar_na(c, &pedido))
    }

    pub(super) fn termino(&self, pedido: &str, fence: i64, motivo: MotivoFim, conteudo: Conteudo) -> Termino {
        let binding = bindings::fixture_binding(&self.hub.store, "b");
        let produtor = Produtor::de(binding.acp_session, Some("turno-b".into()));
        let fim = Encerramento { desfecho: Desfecho::Concluiu, motivo, conteudo, produtor };
        Termino { pedido: pedido.into(), bot: "b".into(), attempt: 1, fence, fim }
    }

    pub(super) fn estado(&self, id: &str) -> (String, Option<String>, i64) {
        self.hub
            .store
            .db
            .locked()
            .query_row("SELECT state,end_reason,fence FROM donna_session_requests WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap()
    }

    pub(super) fn resultados(&self) -> i64 {
        self.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_session_results", [], |r| r.get(0)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Todas as colunas das duas tabelas, para a sentinela.
pub(crate) fn despejo(c: &Connection) -> String {
    let mut out = String::new();
    for table in ["donna_session_requests", "donna_session_results"] {
        let mut st = c.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let n = st.column_count();
        let mut rows = st.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..n {
                write!(out, "{:?}|", row.get_ref(i).unwrap()).unwrap();
            }
        }
    }
    out
}

#[test]
fn aceite_usa_sessoes_exatas_e_recusa_aviso_de_outra_sessao_ou_binding_revogado() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    let row: (String, String, String, String, String, String, String) = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT mode,requester_chat_id,requester_session_id,target_chat_id,target_session_id,state,
               requester_execution_ref FROM donna_session_requests WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();
    assert_eq!((&*row.0, &*row.1, &*row.2, &*row.3, &*row.4, &*row.5), ("wait", "a", "a", "b", "b", "accepted"));
    assert!(row.6.starts_with("acp:") && !row.6.contains("fixture-turn"), "token só como digest");

    // Aviso trocado de sessão/chat não vira origem nem alvo.
    let origem = f.entrada(&Lane::main("a"), EntryKind::Notice);
    let destino = f.entrada(&Lane::main("b"), EntryKind::Notice);
    assert!(f.aceitar_com("trocado", &destino, &origem).is_err());
    let extra = create(
        &f.hub.store.db.locked(),
        &NewSession {
            chat: "b".into(),
            origin: SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: "x".into(),
            runs_on: None,
        },
    )
    .unwrap();
    let noutra = f.entrada(&Lane { chat: "b".into(), session: Some(extra.id) }, EntryKind::Notice);
    assert!(f.aceitar_com("noutra", &origem, &noutra).is_err());

    // Binding revogado: nenhuma identidade nova, nenhum pedido.
    bindings::revoke(&f.hub.store, "a", None).unwrap();
    assert!(f.aceitar_com("revogado", &origem, &destino).is_err());
    let total: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_session_requests", [], |r| r.get(0)).unwrap();
    assert_eq!(total, 1);
}

#[test]
fn conclusao_repetida_identica_devolve_recibo_e_divergente_conflita() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    let fence = despachar(&f.hub.store, &id).unwrap().unwrap();
    assert_eq!(despachar(&f.hub.store, &id).unwrap(), None, "um único envio por tentativa");
    let final_entry = f.entrada(&Lane::main("b"), EntryKind::Agent);
    let t = f.termino(&id, fence, MotivoFim::FimDoTurno, Conteudo::Entrada(final_entry.clone()));
    let recibo = concluir(&f.hub.store, &t).unwrap();
    assert_eq!(recibo, Conclusao::Gravada(format!("{id}#1")));
    assert_eq!(concluir(&f.hub.store, &t).unwrap(), recibo);
    let divergente = f.termino(&id, fence, MotivoFim::Recusa, Conteudo::Entrada(final_entry.clone()));
    assert!(format!("{:#}", concluir(&f.hub.store, &divergente).unwrap_err()).contains("retorno_conflito"));
    assert_eq!(f.estado(&id).0, "finished");
    assert_eq!(f.resultados(), 1);
    let (content, entry, producer): (String, Option<String>, Option<String>) = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT content,final_entry_id,producer_execution_ref FROM donna_session_results", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((content.as_str(), entry.as_deref()), ("final_entry", Some(final_entry.as_str())));
    assert!(producer.is_some_and(|p| !p.contains("turno-b")));
    // Só IDs e metadados: o texto das entradas não foi copiado.
    assert!(!despejo(&f.hub.store.db.locked()).contains(SENTINELA));
}

#[test]
fn cancelamento_e_cerca_vencida_vencem_conclusao_tardia() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    let fence = despachar(&f.hub.store, &id).unwrap().unwrap();
    let antiga = f.termino(&id, fence - 1, MotivoFim::FimDoTurno, Conteudo::Nenhum);
    assert_eq!(concluir(&f.hub.store, &antiga).unwrap(), Conclusao::Vencida);
    assert_eq!(f.estado(&id).0, "running");
    assert!(encerrar(&f.hub.store, &id, Decisao::Cancelado).unwrap());
    let tardia = f.termino(&id, fence, MotivoFim::FimDoTurno, Conteudo::Nenhum);
    assert_eq!(concluir(&f.hub.store, &tardia).unwrap(), Conclusao::Vencida);
    assert!(!encerrar(&f.hub.store, &id, Decisao::Prazo).unwrap(), "decisão anterior prevalece");
    assert_eq!(f.estado(&id), ("cancelled".into(), Some("cancelled".into()), fence + 1));
    assert_eq!(f.resultados(), 0);

    // Conclusão gravada também não é desfeita por decisão posterior.
    let outro = f.aceitar().unwrap();
    let fence = despachar(&f.hub.store, &outro).unwrap().unwrap();
    concluir(&f.hub.store, &f.termino(&outro, fence, MotivoFim::FimDoTurno, Conteudo::Nenhum)).unwrap();
    assert!(!encerrar(&f.hub.store, &outro, Decisao::Cancelado).unwrap());
    assert_eq!(f.estado(&outro).0, "finished");
}

#[test]
fn entrada_final_ausente_de_outra_sessao_ou_tipo_errado_e_recusada_sem_registro() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    let fence = despachar(&f.hub.store, &id).unwrap().unwrap();
    let outro_chat = f.entrada(&Lane::main("a"), EntryKind::Agent);
    let aviso = f.entrada(&Lane::main("b"), EntryKind::Notice);
    for entrada in [outro_chat, aviso, "inexistente".to_owned()] {
        let t = f.termino(&id, fence, MotivoFim::FimDoTurno, Conteudo::Entrada(entrada));
        assert!(format!("{:#}", concluir(&f.hub.store, &t).unwrap_err()).contains("retorno_entrada_invalida"));
    }
    assert_eq!((f.estado(&id).0.as_str(), f.resultados()), ("running", 0));

    // Sem proveniência do produtor, a referência não vira sucesso de conteúdo.
    let final_entry = f.entrada(&Lane::main("b"), EntryKind::Agent);
    let mut t = f.termino(&id, fence, MotivoFim::FimDoTurno, Conteudo::Entrada(final_entry));
    t.fim.produtor = None;
    concluir(&f.hub.store, &t).unwrap();
    let (content, entry): (String, Option<String>) = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT content,final_entry_id FROM donna_session_results", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((content.as_str(), entry), ("unverified", None));
}

#[test]
fn reabertura_preserva_concluido_e_distingue_aceito_de_envio_incerto_sem_reenvio() {
    let f = Fixture::new();
    let concluido = f.aceitar().unwrap();
    let fence = despachar(&f.hub.store, &concluido).unwrap().unwrap();
    concluir(&f.hub.store, &f.termino(&concluido, fence, MotivoFim::FimDoTurno, Conteudo::Nenhum)).unwrap();
    let aceito = f.aceitar().unwrap();
    let incerto = f.aceitar().unwrap();
    let fence = despachar(&f.hub.store, &incerto).unwrap().unwrap();

    let reaberta = Store::open(&f.dir.join("r1.db")).unwrap();
    assert_eq!(recuperar(&reaberta).unwrap(), 2);
    assert_eq!(recuperar(&reaberta).unwrap(), 0, "idempotente");
    drop(reaberta);
    assert_eq!(f.estado(&aceito).1.as_deref(), Some("restart_before_start"));
    assert_eq!(f.estado(&incerto), ("interrupted".into(), Some("restart_uncertain".into()), fence + 1));
    assert_eq!(f.estado(&concluido).0, "finished");
    assert_eq!(f.resultados(), 1);
    // Incerto não vira concluído nem aceita a conclusão tardia da tentativa anterior.
    let tardia = f.termino(&incerto, fence, MotivoFim::FimDoTurno, Conteudo::Nenhum);
    assert_eq!(concluir(&f.hub.store, &tardia).unwrap(), Conclusao::Vencida);
    assert_eq!(despachar(&f.hub.store, &aceito).unwrap(), None, "sem novo envio após reinício");
}

#[test]
fn fim_do_turno_distingue_sucesso_falha_parada_e_recusa() {
    let caso = |parou: bool, limite: bool, erro: bool, reason: &str, texto: Option<&str>, entrada: Option<&str>| {
        let Fim::Turno(e) = fim_do_turno(parou, limite, erro, reason, texto, entrada.map(Into::into), None) else {
            unreachable!()
        };
        (e.desfecho, e.motivo, e.conteudo)
    };
    assert_eq!(
        caso(false, false, false, "end_turn", Some("ok"), Some("e")),
        (Desfecho::Concluiu, MotivoFim::FimDoTurno, Conteudo::Entrada("e".into()))
    );
    assert_eq!(
        caso(false, false, false, "end_turn", Some("ok"), None),
        (Desfecho::Concluiu, MotivoFim::FimDoTurno, Conteudo::SemReferencia)
    );
    assert_eq!(caso(false, false, false, "end_turn", Some(" "), Some("e")).2, Conteudo::Nenhum);
    assert_eq!(
        caso(true, false, true, "cancelled", Some("x"), Some("e")),
        (Desfecho::Parou, MotivoFim::Cancelado, Conteudo::Nenhum)
    );
    assert_eq!(caso(false, false, true, "error", None, None).0, Desfecho::Falhou);
    assert_eq!(caso(false, false, false, "refusal", Some("x"), Some("e")).1, MotivoFim::Recusa);
    assert_eq!(caso(false, true, false, "end_turn", Some("x"), Some("e")).1, MotivoFim::LimiteDaAssinatura);
}
