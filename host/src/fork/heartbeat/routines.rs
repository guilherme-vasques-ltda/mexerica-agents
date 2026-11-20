//! Heartbeat: a consulta da fonte no horário da rotina, dentro do agendador das Rotinas. Nada aqui chama a IA nem
//! executa efeito: a fonte só é lida, o estado e o resultado são gravados juntos, e o aviso sai pelo mesmo caminho
//! de publicação das demais execuções.
use super::{Arc, LockExt, Result, Routine, Routines, Run, Status, Store, Value, new_run};
use crate::fork::heartbeat::{
    Aviso, Concessao, Estado, Fonte, Item, Motivo, Resultado, Saida, avaliar, identidade_de, pode_checar,
};
use crate::fork::rotina_resultado::{Kind, Lenient, RunOutcome, Source};
use std::{fmt::Write as _, panic::AssertUnwindSafe};

const MAX_RESUMO: usize = 300;

/// Fonte que não existe para esta rotina, ou que falhou de forma inesperada: sempre indisponível, nunca vazia.
struct Indisponivel(Motivo);

impl Fonte for Indisponivel {
    fn marcador(&self, _: &str) -> Result<Option<String>, Motivo> {
        Err(self.0)
    }
    fn ler(&self, _: &str) -> Result<Vec<Item>, Motivo> {
        Err(self.0)
    }
}

/// Libera a vaga da rotina ao sair, mesmo se a fonte der pânico.
struct EmAndamento<'a> {
    routines: &'a Routines,
    id: &'a str,
}

impl Drop for EmAndamento<'_> {
    fn drop(&mut self) {
        self.routines.checando.locked().remove(self.id);
    }
}

#[allow(
    dead_code,
    reason = "o conector que registra fontes reais chega em outra entrega; os testes registram fontes fictícias"
)]
impl Routines {
    /// Concede ao bot a leitura de uma fonte. `identidade` diz qual conta ou vínculo ela representa e precisa ser a
    /// mesma entre reinícios para a mesma concessão: outra identidade começa do zero (sem marcador, espera nem avisos
    /// herdados). Sem concessão, a rotina do bot enxerga a fonte como indisponível.
    /// Mexe no registro com o estado das rotinas travado: nenhuma checagem grava entre a mudança e o que ela vê.
    pub(crate) fn registrar_fonte(&self, bot: &str, nome: &str, identidade: &str, fonte: Arc<dyn Fonte>) {
        let _estado = self.state.locked();
        let registro = self.concessoes.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let concessao = Concessao { registro, identidade: identidade_de(identidade), fonte };
        self.fontes.locked().insert((bot.to_owned(), nome.to_owned()), concessao);
    }

    /// Revoga a concessão: a próxima checagem já não consulta a fonte, e a que está lendo descarta o resultado.
    pub(crate) fn retirar_fonte(&self, bot: &str, nome: &str) {
        let _estado = self.state.locked();
        self.fontes.locked().remove(&(bot.to_owned(), nome.to_owned()));
    }
}

impl Routines {
    pub(super) fn check_due(&self, store: &Store, ids: &[String], now: i64) {
        let inicio = std::time::Instant::now();
        for id in ids {
            // Uma fonte lenta atrasa as seguintes: cada consulta usa o horário dela, não o do tick.
            let agora = now.saturating_add(i64::try_from(inicio.elapsed().as_millis()).unwrap_or(i64::MAX));
            if let Err(error) = self.check_one(store, id, agora) {
                tracing::warn!(routine = %id, error = format!("{error:#}"), "checagem silenciosa não foi gravada");
            }
        }
    }

    fn check_one(&self, store: &Store, id: &str, now: i64) -> Result<()> {
        let (bot, declaracao, anterior, geracao, pausas) = {
            let state = self.state.locked();
            if state.paused {
                return Ok(());
            }
            let Some(routine) = state.routines.iter().find(|r| r.id == id && !r.deleted && r.enabled) else {
                return Ok(());
            };
            let Some(declaracao) = &routine.heartbeat else { return Ok(()) };
            (
                routine.bot_id.clone(),
                declaracao.clone(),
                routine.heartbeat_state.clone().unwrap_or_default(),
                routine.updated_at,
                state.pausas,
            )
        };
        let chave = (bot, declaracao.fonte.clone());
        let concedida = self.fontes.locked().get(&chave).map(|c| (c.registro, c.identidade.clone(), c.fonte.clone()));
        let numero = concedida.as_ref().map(|(registro, ..)| *registro);
        let identidade = concedida.as_ref().map(|(_, identidade, _)| identidade.clone());
        // Outra conta ou vínculo não herda marcador, espera nem avisos. Sem concessão (por exemplo, logo depois de
        // reiniciar, antes do conector registrar a fonte) o que se sabia fica: só a falha é registrada.
        let anterior =
            if identidade.is_some() && anterior.concessao != identidade { Estado::default() } else { anterior };
        if !pode_checar(&anterior, &declaracao, now) || !self.checando.locked().insert(id.to_owned()) {
            return Ok(());
        }
        let _vaga = EmAndamento { routines: self, id };
        let fonte: Arc<dyn Fonte> =
            concedida.map_or_else(|| Arc::new(Indisponivel(Motivo::SemAcesso)) as Arc<dyn Fonte>, |(.., fonte)| fonte);
        let mut saida = std::panic::catch_unwind(AssertUnwindSafe(|| avaliar(&*fonte, &declaracao, &anterior, now)))
            .unwrap_or_else(|_| avaliar(&Indisponivel(Motivo::Fora), &declaracao, &anterior, now));
        if identidade.is_some() {
            saida.estado.concessao.clone_from(&identidade);
        }
        self.change(store, |s| {
            // Pausa geral (mesmo que desfeita antes da gravação), exclusão, desligamento, outra edição da rotina
            // (mesmo que desfeita depois) ou concessão retirada ou trocada entre a leitura e a gravação: o resultado
            // é descartado. Tudo isso muda sob o mesmo lock que esta gravação segura, então nada passa no meio.
            if s.paused || s.pausas != pausas {
                return Ok(());
            }
            let atual = self.fontes.locked().get(&chave).map(|c| c.registro);
            if atual != numero {
                return Ok(());
            }
            let Some(routine) = s.routines.iter_mut().find(|r| {
                r.id == id
                    && !r.deleted
                    && r.enabled
                    && r.updated_at == geracao
                    && r.heartbeat.as_ref() == Some(&declaracao)
            }) else {
                return Ok(());
            };
            routine.heartbeat_state = Some(saida.estado.clone());
            if let Some(run) = registro(routine, &saida, now) {
                s.runs.push(run);
            }
            Ok(())
        })
    }
}

/// A execução que fica no histórico: nenhuma para «sem mudança»; só histórico para «mudança sem decisão» e para a
/// primeira falha de uma sequência; aviso (notice e alerta) para decisão pendente e para falha que pede ação.
fn registro(routine: &Routine, saida: &Saida, now: i64) -> Option<Run> {
    let (status, kind, summary, publicar) = match (saida.resultado, &saida.aviso) {
        (Resultado::SemMudanca, _) => return None,
        (Resultado::MudancaSemDecisao, _) => {
            (Status::Succeeded, Kind::Reported, "Houve mudança na fonte, sem decisão pendente.".to_owned(), false)
        }
        (Resultado::DecisaoPendente, aviso) => {
            let texto = match aviso {
                Some(Aviso::Decisao { itens, alem }) => texto_decisao(itens, *alem),
                _ => "Há uma decisão pendente.".to_owned(),
            };
            (Status::Failed, Kind::NeedsOwner, texto, true)
        }
        (Resultado::FonteIndisponivel, aviso) => {
            if aviso.is_none() && saida.estado.falhas_seguidas != 1 {
                return None;
            }
            let texto = match aviso {
                Some(Aviso::Falha { motivo }) => texto_falha(*motivo),
                _ => "A fonte não respondeu.".to_owned(),
            };
            (Status::Failed, Kind::Failed, texto, aviso.is_some())
        }
    };
    let mut run = new_run(routine, Value::Null, None);
    run.status = status;
    run.created_at = now;
    run.started_at = Some(now);
    run.finished_at = Some(now);
    // `Declared` só para que a falha mostre o texto acima; ninguém declarou nada, não houve IA.
    let source = if kind == Kind::Failed { Source::Declared } else { Source::Observed };
    // O histórico guarda só o desfecho; o texto do aviso (fato e escolha, vindos da fonte) fica num campo privado que
    // só a publicação lê e depois apaga, e que nenhuma listagem devolve.
    let generico = match kind {
        Kind::NeedsOwner => "Há uma decisão pendente.",
        Kind::Failed => "A fonte não respondeu.",
        _ => summary.as_str(),
    };
    run.outcome = Some(Lenient::Ok(RunOutcome {
        version: 1,
        kind,
        source,
        reason: None,
        summary: Some(generico.to_owned()),
        visible_reply: false,
    }));
    run.heartbeat_texto = publicar.then_some(summary);
    run.report_pending = publicar;
    run.heartbeat = true;
    Some(run)
}

fn texto_decisao(itens: &[(String, String)], alem: usize) -> String {
    let mut texto = itens.iter().map(|(fato, escolha)| format!("{fato} — {escolha}")).collect::<Vec<_>>().join("; ");
    if alem > 0 {
        let _ = write!(texto, " (e mais {alem})");
    }
    if texto.chars().count() > MAX_RESUMO {
        texto = texto.chars().take(MAX_RESUMO - 1).collect::<String>() + "…";
    }
    texto
}

fn texto_falha(motivo: Motivo) -> String {
    match motivo {
        Motivo::SemAcesso => "A fonte está sem acesso: confira a conexão e a permissão desta rotina.",
        Motivo::Fora => "A fonte continua fora do ar ou sem resposta.",
        Motivo::Limite => "A fonte devolveu mais do que a checagem aceita ou limitou o uso.",
    }
    .to_owned()
}
