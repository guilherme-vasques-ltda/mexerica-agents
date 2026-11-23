//! F6 fatia B (MEX-1391): os ganchos do ator do bot para a troca de conta (regras (m), (b) e (o) do plano).
//!
//! - **Antes do envio** (`fallback_prepare`, início de um turno humano novo na conversa do dono): a decisão escolhe a
//!   primeira conta livre da fila `escolha → reservas`; trocar grava a sobreposição da conversa, avisa na conversa e
//!   leva a passagem (resumo) para a sessão nova. Sem conta livre, o preparo para com `quota_near_limit`.
//! - **Antes da permissão** (`fallback_reprovar`): sob uma troca, a prova da conta (k) é refeita imediatamente antes
//!   do envio; sem prova, nada é enviado.
//! - **Cota esgotada no meio** (`fallback_on_limit`): a conta descansa e, se há reserva, o pedido sem efeito volta à
//!   fila para sair na conta nova; com efeito possível, o turno para como incerto ("confira antes de refazer").
//!
//! Só a conversa do dono (principal e extras do Pessoal). Rotina, grupo, canal e conversa de outro Space seguem como
//! antes: a espera por limite (D72).
use super::{Actor, Seg};
use crate::LockExt;
use crate::fork::fallback_contas::{
    Motivo, Sobreposicao,
    escolha::{self, Decisao, Pedido},
    estado::{self, Troca},
    prova,
};
use crate::fork::sessoes::{admissao, engine_choice};
use crate::store::{EntryKind, Lane};
use anyhow::{Result, anyhow};
use serde_json::json;

/// O que o ator guarda do fallback no turno.
#[derive(Default)]
pub(super) struct Turno {
    /// Nenhuma conta da fila pode receber o pedido: o preparo para depois da reserva.
    parar: Option<Parada>,
    /// O aviso que substitui o texto genérico de limite (o fallback decidiu o que fazer com o pedido).
    pub(super) aviso_limite: Option<String>,
}

struct Parada {
    conta: String,
    motivo: Motivo,
    volta_em: Option<i64>,
}

/// "sex 14:00", ou só "14:00" quando é hoje.
fn quando(at: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(at) = Local.timestamp_millis_opt(at).single() else { return "assim que liberar".into() };
    let dias = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
    let hoje = Local::now().date_naive() == at.date_naive();
    let hora = at.format("%H:%M");
    if hoje { hora.to_string() } else { format!("{} {hora}", dias[at.weekday().num_days_from_monday() as usize]) }
}

fn incerto_key(session: &str) -> String {
    format!("fork.fallback-contas.incerto.{session}")
}

fn parada_key(session: &str) -> String {
    format!("fork.fallback-contas.parada.{session}")
}

/// O fallback só vale na conversa do dono no Pessoal (principal e extras): conversa de outro Space, de canal, rotina e bot
/// com comando próprio seguem como antes (espera por limite D72). O mesmo portão vale antes do envio e no limite.
pub(super) fn aplica_a(cfg: &crate::store::BotConfig, lane: &Lane) -> bool {
    !crate::agent::backends::is_antigravity(&cfg.backend)
        && !crate::fork::assinatura_pessoal::claude(&cfg.backend)
        && cfg.context.is_none()
        && cfg.space_id == crate::fork::spaces::PERSONAL
        && cfg.command.is_none()
        && lane.chat == cfg.id
}

impl Actor {
    fn conta_rotulo(&self, id: &str) -> String {
        crate::fork::engine_accounts::Registry::load(&self.hub.store)
            .ok()
            .and_then(|registry| registry.account(id).ok().map(|a| a.dto().label))
            .unwrap_or_else(|| "outra conta".into())
    }

    fn aviso_fallback(&self, lane: &Lane, text: &str, extra: serde_json::Value) {
        let turn = self.hub.store.max_turn(&lane.chat);
        let mut data = json!({"text": text, "style": "info", "author": self.cfg.id});
        data["contaTroca"] = extra;
        if let Some(session) = &lane.session {
            data["session"] = json!(session);
        }
        if self.hub.add_entry(lane, EntryKind::Notice, turn, &data).is_none() {
            tracing::warn!(bot = %self.cfg.id, "não foi possível publicar o aviso da troca de conta");
        }
    }

    /// Início de um turno humano novo, antes de capturar a configuração do turno. Nunca falha o turno por si: se a
    /// decisão não puder ser lida, a conversa segue na conta que já tinha (como antes do fallback).
    pub(super) async fn fallback_prepare(
        &mut self,
        lane: &Lane,
        entry_ids: &[String],
        hidden: bool,
        prepared_token: Option<&str>,
    ) {
        self.fallback.parar = None;
        let novo_humano = prepared_token.is_none()
            && self.waiting.is_none()
            && self.dispatch.is_none()
            && !hidden
            && !entry_ids.is_empty()
            && self.active_request.is_none()
            && self.active_group.is_none()
            && self.active_routine.is_none();
        if !novo_humano {
            return;
        }
        if let Err(error) = self.fallback_decidir(lane).await {
            tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "fallback entre contas indisponível neste turno");
        }
    }

    async fn fallback_decidir(&mut self, lane: &Lane) -> Result<()> {
        let original =
            self.hub.store.bot(&self.cfg.id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
        if !aplica_a(&original, lane) {
            return Ok(());
        }
        let (session, own, efetiva) = {
            let db = self.hub.store.db.locked();
            let Some(session) = admissao::engine_session(&db, lane, &original)? else { return Ok(()) };
            let own = engine_choice::read_own(&db, &session)?;
            let efetiva = engine_choice::read(&db, &session)?;
            (session, own, efetiva)
        };
        let (Some(own), Some(efetiva)) = (own, efetiva) else { return Ok(()) };
        if crate::agent::backends::is_antigravity(&own.backend)
            || crate::agent::backends::is_antigravity(&efetiva.backend)
        {
            return Ok(());
        }
        let (Some(origem), Some(atual)) = (own.account_id.clone(), efetiva.account_id.clone()) else { return Ok(()) };
        let mut cfg = original.clone();
        own.apply(&mut cfg);
        if !aplica_a(&cfg, lane) {
            return Ok(());
        }
        // Cada tentativa de prova que falha bloqueia o destino por 1 h e a decisão roda de novo sem ele.
        for _ in 0..=estado::MAX_RESERVAS {
            let pedido = Pedido { space: &cfg.space_id, bot: &cfg.id, origem: &origem, atual: &atual, cfg: &cfg };
            match escolha::decidir(&self.hub, &pedido).await? {
                Decisao::SemFallback => return Ok(()),
                Decisao::Manter => {
                    let _ = self.hub.store.kv_delete(&parada_key(&session));
                    return Ok(());
                }
                Decisao::Voltar => {
                    self.fallback_aplicar(lane, &session, &cfg, &atual, &origem, Motivo::Voltou, None)?;
                    return Ok(());
                }
                Decisao::Trocar { para, motivo, volta_em } => {
                    let mut alvo = cfg.clone();
                    alvo.fork_editor.engine_account_id = Some(para.clone());
                    match prova::provar(&self.hub, &alvo, &para).await {
                        Ok(()) => {
                            self.fallback_aplicar(lane, &session, &cfg, &atual, &para, motivo, volta_em)?;
                            return Ok(());
                        }
                        Err(falha) => {
                            self.fallback_bloquear(lane, &para, falha);
                        }
                    }
                }
                Decisao::Parar { conta, motivo, volta_em } => {
                    self.fallback.parar = Some(Parada { conta, motivo, volta_em });
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Destino que falhou ao assumir: 1 h sem nova troca para ele (regra (o).3) e um aviso (regra (k)).
    fn fallback_bloquear(&self, lane: &Lane, conta: &str, falha: prova::Falha) {
        let ate = crate::store::now_ms() + estado::BLOQUEIO_MS;
        if let Err(error) = estado::alterar(&self.hub.store, |e| {
            e.bloqueio.insert(conta.to_owned(), ate);
            Ok(())
        }) {
            tracing::warn!(%error, "bloqueio do destino não gravado");
        }
        let nome = self.conta_rotulo(conta);
        self.aviso_fallback(
            lane,
            &format!("Não passei para {nome}: {}.", falha.motivo()),
            json!({"kind": "recusada", "para": conta, "codigo": falha.codigo()}),
        );
    }

    /// Grava a troca (ou a volta), avisa na conversa e leva a passagem para a sessão da conta nova.
    #[allow(clippy::too_many_arguments)]
    fn fallback_aplicar(
        &mut self,
        lane: &Lane,
        session: &str,
        cfg: &crate::store::BotConfig,
        de: &str,
        para: &str,
        motivo: Motivo,
        volta_em: Option<i64>,
    ) -> Result<()> {
        let origem = cfg.fork_editor.engine_account_id.clone().ok_or_else(|| anyhow!("fallback_without_origin"))?;
        let agora = crate::store::now_ms();
        {
            let db = self.hub.store.db.locked();
            let sobre = (para != origem).then(|| Sobreposicao {
                space: cfg.space_id.clone(),
                bot: cfg.id.clone(),
                origem: origem.clone(),
                atual: para.to_owned(),
                desde: agora,
                motivo,
            });
            estado::gravar_sobreposicao(&db, session, sobre.as_ref())?;
            estado::alterar_em(&db, |e| {
                e.trocas.push(Troca {
                    id: uuid::Uuid::new_v4().to_string(),
                    em: agora,
                    space: cfg.space_id.clone(),
                    bot: cfg.id.clone(),
                    sessao: session.to_owned(),
                    de: de.to_owned(),
                    para: para.to_owned(),
                    motivo,
                    volta_em,
                });
                Ok(())
            })?;
        }
        let (nome_de, nome_para) = (self.conta_rotulo(de), self.conta_rotulo(para));
        let texto = match motivo {
            Motivo::PertoDoLimite => format!("{nome_de} perto do limite; seguindo com {nome_para}."),
            Motivo::Esgotada => format!(
                "Passei para {nome_para} porque {nome_de} acabou{}.",
                volta_em.map_or_else(String::new, |at| format!(" (volta {})", quando(at)))
            ),
            Motivo::Voltou => format!("Voltei para {nome_para}: {nome_de} já pode receber pedidos."),
        };
        self.aviso_fallback(
            lane,
            &texto,
            json!({"kind": "troca", "de": de, "para": para, "motivo": motivo, "voltaEm": volta_em}),
        );
        let _ = self.hub.store.kv_delete(&parada_key(session));
        // A conta nova abre sessão nova (carimbo diferente): a conversa vai junto, em resumo, e o que ficou incerto
        // no pedido anterior vai marcado (regra (c)).
        if lane.session.is_none()
            && let Some(sid) = self.session_id.clone()
        {
            let incerto = self.hub.store.kv_get(&incerto_key(session)).is_some();
            let _ = self.hub.store.kv_delete(&incerto_key(session));
            let mut diz = format!(
                "A conta mudou de {nome_de} para {nome_para}. Sua próxima mensagem começa na conta nova levando este resumo."
            );
            if incerto {
                diz.push_str(
                    " O pedido anterior parou no meio: o que ele já fez pode ter efeito, confira antes de refazer.",
                );
            }
            self.engine_switch_handoff(&sid, &diz);
            if super::fork_troca_motor::pending_switch(&self.hub.store, &self.cfg.id) {
                self.handoff = self.take_handoff();
                self.handoff_chat = Some(lane.clone());
            }
        }
        Ok(())
    }

    /// Depois da reserva do despacho: sem conta livre, o preparo para com o motivo (o app mostra a hora e o botão
    /// "Permitir uso extra"); a fila tenta de novo sozinha e só segue quando uma conta liberar ou o dono permitir.
    pub(super) fn fallback_parar(&mut self) -> Result<()> {
        let Some(parada) = self.fallback.parar.take() else { return Ok(()) };
        let session = engine_session_of(self).unwrap_or_default();
        let marca = format!("{}|{}", parada.conta, parada.volta_em.unwrap_or_default());
        if self.hub.store.kv_get(&parada_key(&session)).as_deref() != Some(marca.as_str()) {
            let nome = self.conta_rotulo(&parada.conta);
            let situacao = if parada.motivo == Motivo::PertoDoLimite { "está perto do limite" } else { "acabou" };
            let texto = parada.volta_em.map_or_else(
                || format!("{nome} {situacao} e não há outra conta livre. Parei aqui."),
                |at| format!("{nome} {situacao} e não há outra conta livre. Parei aqui; volta {}.", quando(at)),
            );
            self.aviso_fallback(
                &self.lane.clone(),
                &texto,
                json!({"kind": "parada", "conta": parada.conta, "voltaEm": parada.volta_em}),
            );
            let _ = self.hub.store.kv_set(&parada_key(&session), &marca);
        }
        let code = if parada.motivo == Motivo::PertoDoLimite { "quota_near_limit" } else { "quota_exhausted" };
        Err(anyhow!("{code}: nenhuma conta livre; o limite volta quando a janela renovar."))
    }

    /// Regra (k), de novo imediatamente antes do envio: sob uma troca, sem prova da conta o prompt não sai.
    pub(super) async fn fallback_reprovar(&mut self) -> Result<()> {
        let sobre = {
            let db = self.hub.store.db.locked();
            match admissao::engine_session(&db, &self.lane, &self.cfg)? {
                Some(session) => estado::sobreposicao(&db, &session)?,
                None => None,
            }
        };
        let Some(sobre) = sobre.filter(|s| self.cfg.fork_editor.engine_account_id.as_deref() == Some(s.atual.as_str()))
        else {
            return Ok(());
        };
        match prova::provar(&self.hub, &self.cfg, &sobre.atual).await {
            Ok(()) => Ok(()),
            Err(falha) => {
                let lane = self.lane.clone();
                self.fallback_bloquear(&lane, &sobre.atual, falha);
                Err(anyhow!("fallback_no_proof: {}.", falha.motivo()))
            }
        }
    }

    /// Regra (b): a conta acabou no meio do pedido. Devolve se o fallback tratou (o limite não vira espera D72).
    pub(super) fn fallback_on_limit(&mut self, resets_at: Option<i64>) -> bool {
        let Some(input) = self.input.as_ref() else { return false };
        if self.stop_requested
            || self.waiting.is_some()
            || self.compacting.is_some()
            || self.cfg.context.is_some()
            || self.active_routine.is_some()
            || self.active_group.is_some()
            || self.active_request.is_some()
            || input.hidden
            || input.transient
            || input.routine.is_some()
            || input.entries.is_empty()
        {
            return false;
        }
        let (entries, lane) = (input.entries.clone(), input.lane());
        // Mesmo portão do preparo: outro Space (ou canal, ou comando próprio) nunca é reenfileirado nem tem a espera D72
        // suprimida; sem isto o pedido voltaria à mesma conta esgotada em laço.
        if !aplica_a(&self.cfg, &lane) {
            return false;
        }
        let Some(atual) = self.cfg.fork_editor.engine_account_id.clone() else { return false };
        let Ok((session, origem)) = (|| -> Result<(String, String)> {
            let db = self.hub.store.db.locked();
            let session =
                admissao::engine_session(&db, &lane, &self.cfg)?.ok_or_else(|| anyhow!("fallback_not_applicable"))?;
            let own = engine_choice::read_own(&db, &session)?.and_then(|e| e.account_id);
            Ok((session, own.ok_or_else(|| anyhow!("fallback_not_applicable"))?))
        })() else {
            return false;
        };
        let agora = crate::store::now_ms();
        let Ok(registry) = crate::fork::engine_accounts::Registry::load(&self.hub.store) else { return false };
        let Ok(estado) = estado::ler(&self.hub.store) else { return false };
        let pedido =
            Pedido { space: &self.cfg.space_id, bot: &self.cfg.id, origem: &origem, atual: &atual, cfg: &self.cfg };
        // Há outra conta que, pelo que o Hub já sabe, pode receber o pedido? A prova e o teto valem na troca.
        let outra = escolha::fila(&estado, &pedido).into_iter().any(|conta| {
            conta != atual
                && !estado.bloqueada(&conta, agora)
                && estado.descansando(&conta, agora).is_none()
                && escolha::elegivel(&registry, &self.cfg, &conta)
        });
        let ate = resets_at.filter(|at| *at > agora).unwrap_or(agora + estado::DESCANSO_SEM_LEITURA_MS);
        // A conta esgotada descansa até o reset informado, ou 1 h sem leitura (plano, "Volta").
        let descansou = estado::alterar(&self.hub.store, |e| {
            e.descanso.insert(atual.clone(), estado::Descanso { ate, motivo: Motivo::Esgotada });
            Ok(())
        });
        if descansou.is_err() || !outra {
            return false;
        }
        let nome = self.conta_rotulo(&atual);
        // Sem efeito possível (nada executado, nada dito) o pedido volta à fila e sai na conta nova.
        let limpo = self.tools.is_empty()
            && self.sent.is_empty()
            && self.last_text.is_none()
            && matches!(self.seg, Seg::None)
            && matches!(self.reacted, super::Reacted::No);
        if limpo && self.fallback_requeue(&entries) {
            self.fallback.aviso_limite =
                Some(format!("{nome} acabou antes de responder; seu pedido vai de novo na próxima conta."));
        } else {
            let _ = self.hub.store.kv_set(&incerto_key(&session), "1");
            self.fallback.aviso_limite = Some(format!(
                "{nome} acabou no meio do pedido. O que ela já fez pode ter efeito: confira antes de refazer. O próximo pedido segue na próxima conta."
            ));
        }
        true
    }

    /// Devolve as mensagens do turno à fila: o envio foi recusado antes de qualquer efeito.
    fn fallback_requeue(&mut self, ids: &[String]) -> bool {
        for id in ids {
            let Some(mut entry) = self.hub.store.entry(id) else { return false };
            if entry.kind != EntryKind::User.as_str() {
                return false;
            }
            entry.data["status"] = json!("queued");
            if self.hub.set_entry(id, &entry.data).is_none() {
                return false;
            }
        }
        match self.hub.store.queued_user_entries_for_identity(&self.cfg.id) {
            Ok(entries) => {
                // `requeue_humans_from_snapshot` ignora o que já está na fila; a reserva do turno acaba antes.
                self.requeue_humans_from_snapshot(ids, &entries, None);
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mensagens do pedido recusado não voltaram à fila");
                false
            }
        }
    }
}

fn engine_session_of(actor: &Actor) -> Option<String> {
    let db = actor.hub.store.db.locked();
    admissao::engine_session(&db, &actor.lane, &actor.cfg).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::aplica_a;
    use crate::store::{BotConfig, Lane};
    use serde_json::json;

    fn cfg(extra: &serde_json::Value) -> BotConfig {
        let mut base = json!({"id":"bot","name":"Bot","backend":"codex"});
        base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn so_a_conversa_do_dono_no_pessoal_entra_no_fallback() {
        let main = Lane::main("bot");
        assert!(aplica_a(&cfg(&json!({})), &main));
        assert!(!aplica_a(&cfg(&json!({"backend":"claude"})), &main), "assinatura não roda fallback");
        assert!(
            aplica_a(&cfg(&json!({})), &Lane { chat: "bot".into(), session: Some("s".into()) }),
            "extra do Pessoal"
        );
        // Outro Space, comando próprio e outra conversa (canal, grupo) seguem como antes.
        assert!(!aplica_a(&cfg(&json!({"spaceId":"00000000-0000-4000-8000-0000000000aa"})), &main), "outro Space");
        assert!(!aplica_a(&cfg(&json!({"command":"meu-cli --acp"})), &main), "comando próprio");
        assert!(!aplica_a(&cfg(&json!({})), &Lane::main("outro")), "outra conversa");
    }
}
