//! Heartbeat: a consulta da fonte no horário da rotina, dentro do agendador das Rotinas. Nada aqui chama a IA nem
//! executa efeito: a fonte só é lida, o estado e o resultado são gravados juntos, e o aviso sai pelo mesmo caminho
//! de publicação das demais execuções.
use super::{Arc, LockExt, Result, Routine, Routines, Run, Status, Store, Value, new_run};
use crate::fork::heartbeat::{
    Aviso, Concessao, Estado, Fonte, Item, Motivo, Resultado, Saida, avaliar, identidade_de, pode_checar,
};
use crate::fork::rotina_resultado::{Kind, Lenient, RunOutcome, Source};
use std::{
    fmt::Write as _,
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

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

/// A resposta do agendador a um pedido de admissão.
enum Resposta {
    /// Admitida sobre o estado de uma época (veja `EstadoRotinas`): a linha de execução ainda confere a época viva.
    Admitida(u64),
    /// Sem autorização: pausa, rotina mudada ou apagada, concessão retirada ou trocada.
    Recusada,
    /// O prazo da chamada já venceu.
    Expirada,
}

/// O que a linha de execução de uma chamada diz ao agendador: um pedido de admissão (com a caixa da resposta) ou o fim.
enum Pedido<T> {
    Pedir(mpsc::Sender<Resposta>),
    Fim(std::thread::Result<Result<T, Motivo>>),
}

/// A fonte com prazo por chamada: cada `marcador` ou `ler` roda numa linha de execução à parte e, se não voltar a tempo
/// (ou entrar em pânico), vira `Motivo::Fora`. Uma chamada que ficou para trás mantém a fonte `ocupada`: nova chamada
/// não é aberta até ela terminar, para uma fonte travada não acumular linhas de execução.
///
/// A admissão tem a última palavra na própria linha de execução, depois de qualquer espera: ela pede a decisão ao
/// agendador (que lê o estado sob lock e devolve a época em que decidiu) e, ao recebê-la, confere sozinha, sem lock,
/// imediatamente antes de chamar a fonte: o prazo absoluto da chamada, a bandeira de abandono que o agendador liga ao
/// desistir, a validade da concessão e se a época do estado ainda é a da decisão. Qualquer divergência: se mudou o
/// estado, pede de novo (e a nova decisão recusa o que deixou de valer); se venceu o prazo ou foi abandonada, não chama.
/// Sobra apenas a janela de poucas instruções entre a última leitura e a chamada. Uma chamada já em curso não é
/// cancelada: seu resultado é descartado na gravação.
struct ComPrazo<'a> {
    fonte: Arc<dyn Fonte>,
    prazo: Duration,
    ocupada: Arc<AtomicBool>,
    valida: Arc<AtomicBool>,
    epoca: Arc<AtomicU64>,
    admitir: &'a (dyn Fn() -> Option<u64> + Sync),
    /// Só para registrar o motivo de uma negativa dentro da linha de execução da chamada.
    rotina: String,
    fonte_nome: String,
    #[cfg(test)]
    gancho: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ComPrazo<'_> {
    fn chamar<T: Send + 'static>(
        &self,
        chamada: impl FnOnce(&dyn Fonte) -> Result<T, Motivo> + Send + 'static,
    ) -> Result<T, Motivo> {
        if self.ocupada.swap(true, Ordering::AcqRel) {
            return Err(Motivo::Fora);
        }
        let limite = Instant::now() + self.prazo;
        let abandonada = Arc::new(AtomicBool::new(false));
        let (envia, recebe) = mpsc::channel();
        let (fonte, ocupada, valida, epoca) =
            (self.fonte.clone(), self.ocupada.clone(), self.valida.clone(), self.epoca.clone());
        let (rotina, fonte_nome) = (self.rotina.clone(), self.fonte_nome.clone());
        let abandono = abandonada.clone();
        #[cfg(test)]
        let gancho = self.gancho.clone();
        let aberta = std::thread::Builder::new().spawn(move || {
            let resultado = loop {
                let (pede, resposta) = mpsc::channel();
                if envia.send(Pedido::Pedir(pede)).is_err() {
                    break Ok(Err(Motivo::Fora));
                }
                let decisao = match resposta.recv() {
                    Ok(Resposta::Admitida(decisao)) => decisao,
                    Ok(Resposta::Recusada) => break Ok(Err(Motivo::SemAcesso)),
                    Ok(Resposta::Expirada) | Err(_) => break Ok(Err(Motivo::Fora)),
                };
                #[cfg(test)]
                if let Some(gancho) = &gancho {
                    gancho();
                }
                if abandono.load(Ordering::Acquire) || Instant::now() >= limite {
                    break Ok(Err(Motivo::Fora));
                }
                if !valida.load(Ordering::Acquire) {
                    tracing::warn!(routine = %rotina, fonte = %fonte_nome, "leitura não admitida: concessão retirada ou trocada durante a espera");
                    break Ok(Err(Motivo::SemAcesso));
                }
                if epoca.load(Ordering::SeqCst) != decisao {
                    continue;
                }
                break std::panic::catch_unwind(AssertUnwindSafe(|| chamada(&*fonte)));
            };
            ocupada.store(false, Ordering::Release);
            let _ = envia.send(Pedido::Fim(resultado));
        });
        if aberta.is_err() {
            self.ocupada.store(false, Ordering::Release);
            return Err(Motivo::Fora);
        }
        loop {
            match recebe.recv_timeout(limite.saturating_duration_since(Instant::now())) {
                Ok(Pedido::Pedir(resposta)) => {
                    let decisao = if Instant::now() >= limite {
                        Resposta::Expirada
                    } else {
                        match (self.admitir)() {
                            None => Resposta::Recusada,
                            // A espera pelo estado pode ter passado do prazo: admissão atrasada não vira chamada.
                            Some(epoca) if Instant::now() < limite => Resposta::Admitida(epoca),
                            Some(_) => Resposta::Expirada,
                        }
                    };
                    let _ = resposta.send(decisao);
                }
                Ok(Pedido::Fim(Ok(resultado))) if Instant::now() < limite => return resultado,
                _ => {
                    abandonada.store(true, Ordering::Release);
                    return Err(Motivo::Fora);
                }
            }
        }
    }
}

impl Fonte for ComPrazo<'_> {
    fn marcador(&self, escopo: &str) -> Result<Option<String>, Motivo> {
        let escopo = escopo.to_owned();
        self.chamar(move |fonte| fonte.marcador(&escopo))
    }
    fn ler(&self, escopo: &str) -> Result<Vec<Item>, Motivo> {
        let escopo = escopo.to_owned();
        self.chamar(move |fonte| fonte.ler(&escopo))
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
        self.state.epoca.fetch_add(1, Ordering::SeqCst);
        let registro = self.concessoes.fetch_add(1, Ordering::SeqCst) + 1;
        // A trava de chamadas é da instância da fonte, não do registro: recadastrar a mesma fonte (ou retirá-la e
        // registrá-la de novo) enquanto uma chamada anterior não voltou não libera outra chamada em paralelo.
        let ocupada = {
            let mut travas = self.travas.locked();
            travas.retain(|(viva, _)| viva.strong_count() > 0);
            let existente = travas
                .iter()
                .find(|(viva, _)| std::ptr::addr_eq(viva.as_ptr(), Arc::as_ptr(&fonte)))
                .map(|(_, trava)| trava.clone());
            existente.unwrap_or_else(|| {
                let nova = Arc::new(AtomicBool::new(false));
                travas.push((Arc::downgrade(&fonte), nova.clone()));
                nova
            })
        };
        let concessao = Concessao {
            registro,
            identidade: identidade_de(identidade),
            fonte,
            ocupada,
            valida: Arc::new(AtomicBool::new(true)),
        };
        let anterior = self.fontes.locked().insert((bot.to_owned(), nome.to_owned()), concessao);
        if let Some(anterior) = anterior {
            tracing::warn!(bot = %bot, fonte = %nome, "concessão substituída: a leitura em andamento com a antiga é descartada");
            anterior.valida.store(false, Ordering::Release);
        }
    }

    /// Revoga a concessão: a próxima checagem já não consulta a fonte, e a que está lendo descarta o resultado.
    pub(crate) fn retirar_fonte(&self, bot: &str, nome: &str) {
        let _estado = self.state.locked();
        self.state.epoca.fetch_add(1, Ordering::SeqCst);
        let retirada = self.fontes.locked().remove(&(bot.to_owned(), nome.to_owned()));
        if let Some(retirada) = retirada {
            tracing::warn!(bot = %bot, fonte = %nome, "concessão retirada: a leitura em andamento é descartada");
            retirada.valida.store(false, Ordering::Release);
        }
        // A concessão retirada já não segura a fonte: some a trava de quem não tem mais chamada viva.
        self.travas.locked().retain(|(viva, _)| viva.strong_count() > 0);
    }
}

impl Routines {
    /// As checagens vencidas neste tick, cada uma presa à versão da rotina (`updated_at`) de quando foi coletada: se a
    /// rotina mudou entre a coleta e a vez dela, aquela ocorrência fica para trás. `relogio` dá o horário de cada
    /// consulta: uma fonte lenta atrasa as seguintes, então cada uma usa o instante em que de fato começa.
    pub(super) fn check_collected(&self, store: &Store, vencidas: &[(String, i64)], mut relogio: impl FnMut() -> i64) {
        for (id, versao) in vencidas {
            if let Err(error) = self.check_one(store, id, *versao, relogio()) {
                tracing::warn!(routine = %id, error = format!("{error:#}"), "checagem silenciosa não foi gravada");
            }
        }
    }

    /// Testes: a versão de cada rotina é a de agora.
    #[cfg(test)]
    pub(super) fn check_due(&self, store: &Store, ids: &[String], mut relogio: impl FnMut() -> i64) {
        let vencidas: Vec<(String, i64)> = {
            let state = self.state.locked();
            ids.iter()
                .filter_map(|id| state.routines.iter().find(|r| &r.id == id).map(|r| (id.clone(), r.updated_at)))
                .collect()
        };
        self.check_collected(store, &vencidas, &mut relogio);
    }

    fn check_one(&self, store: &Store, id: &str, coletada: i64, now: i64) -> Result<()> {
        let (bot, declaracao, anterior, geracao, pausas) = {
            let state = self.state.locked();
            if state.paused {
                return Ok(());
            }
            let Some(routine) =
                state.routines.iter().find(|r| r.id == id && !r.deleted && r.enabled && r.updated_at == coletada)
            else {
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
        let concedida = self
            .fontes
            .locked()
            .get(&chave)
            .map(|c| (c.registro, c.identidade.clone(), c.fonte.clone(), c.ocupada.clone(), c.valida.clone()));
        let numero = concedida.as_ref().map(|(registro, ..)| *registro);
        let identidade = concedida.as_ref().map(|(_, identidade, ..)| identidade.clone());
        // Outra conta ou vínculo não herda marcador, espera nem avisos. Sem concessão (por exemplo, logo depois de
        // reiniciar, antes do conector registrar a fonte) o que se sabia fica: só a falha é registrada.
        let anterior =
            if identidade.is_some() && anterior.concessao != identidade { Estado::default() } else { anterior };
        if !pode_checar(&anterior, &declaracao, now) || !self.checando.locked().insert(id.to_owned()) {
            return Ok(());
        }
        let _vaga = EmAndamento { routines: self, id };
        let prazo = Duration::from_millis(self.prazo_fonte_ms.load(Ordering::Relaxed));
        // Imediatamente antes de cada chamada à fonte (não só ao gravar): pausa geral, exclusão, desligamento, outra
        // edição da rotina e concessão retirada ou trocada fecham a porta para leituras novas. Os dois locks são pegos
        // na mesma ordem das mudanças e soltos antes da chamada, que nunca roda com lock.
        let decide = || {
            let s = self.state.locked();
            if s.paused || s.pausas != pausas {
                tracing::warn!(routine = %id, fonte = %declaracao.fonte, "leitura não admitida: pausa geral");
                return None;
            }
            if self.fontes.locked().get(&chave).map(|c| c.registro) != numero {
                tracing::warn!(routine = %id, fonte = %declaracao.fonte, "leitura não admitida: concessão retirada ou trocada");
                return None;
            }
            let viva = s.routines.iter().any(|r| {
                r.id == id
                    && !r.deleted
                    && r.enabled
                    && r.updated_at == geracao
                    && r.heartbeat.as_ref() == Some(&declaracao)
            });
            if !viva {
                tracing::warn!(routine = %id, fonte = %declaracao.fonte, "leitura não admitida: rotina pausada, apagada ou editada");
            }
            viva.then(|| self.state.epoca.load(Ordering::SeqCst))
        };
        let admitir = || {
            #[cfg(test)]
            {
                let gancho = self.entre_despacho_e_chamada.locked().take();
                if let Some(gancho) = gancho {
                    gancho();
                }
            }
            decide()
        };
        if concedida.is_none() {
            tracing::warn!(routine = %id, fonte = %declaracao.fonte, "checagem sem concessão registrada: a fonte é lida como sem acesso");
        }
        let fonte: Box<dyn Fonte + '_> = concedida.map_or_else(
            || Box::new(Indisponivel(Motivo::SemAcesso)) as Box<dyn Fonte + '_>,
            |(_, _, fonte, ocupada, valida)| {
                Box::new(ComPrazo {
                    fonte,
                    prazo,
                    ocupada,
                    valida,
                    epoca: self.state.epoca.clone(),
                    admitir: &admitir,
                    rotina: id.to_owned(),
                    fonte_nome: declaracao.fonte.clone(),
                    #[cfg(test)]
                    gancho: self.depois_da_admissao.locked().clone(),
                }) as Box<dyn Fonte + '_>
            },
        );
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
                tracing::warn!(routine = %id, fonte = %declaracao.fonte, "resultado da checagem descartado: pausa geral");
                return Ok(());
            }
            let atual = self.fontes.locked().get(&chave).map(|c| c.registro);
            if atual != numero {
                tracing::warn!(routine = %id, fonte = %declaracao.fonte, "resultado da checagem descartado: concessão retirada ou trocada");
                return Ok(());
            }
            let Some(routine) = s.routines.iter_mut().find(|r| {
                r.id == id
                    && !r.deleted
                    && r.enabled
                    && r.updated_at == geracao
                    && r.heartbeat.as_ref() == Some(&declaracao)
            }) else {
                tracing::warn!(routine = %id, fonte = %declaracao.fonte, "resultado da checagem descartado: rotina pausada, apagada ou editada");
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
