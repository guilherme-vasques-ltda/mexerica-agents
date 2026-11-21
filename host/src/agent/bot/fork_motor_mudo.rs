//! Aviso quando o motor fica mudo no meio de um turno.
//!
//! O Hub só avisa; nunca encerra o processo nem repete o turno. O silêncio conta desde o último sinal do motor
//! (uma atualização da sessão do turno, um pedido de permissão ou de leitura de skill), e só
//! vale enquanto o pedido `session/prompt` está aberto e nada justifica a quietude: cartão de permissão pendente,
//! espera do limite da conta, ferramenta em execução (pode ser longa e muda), compactação (do Hub ou do próprio motor) ou parada já pedida.
//! Rotina fica de fora: tem prazo próprio e o Hub a encerra ao estourar.
use super::{Actor, CompactionUsage, EntryKind};
use serde_json::json;
use std::time::Duration;
use tokio::time::Instant;

/// Dez minutos: o mesmo prazo que os SDKs da `OpenAI` e da Anthropic usam por padrão para uma chamada, e bem acima
/// das pausas normais de um motor que pensa ou espera a rede.
const LIMITE_PADRAO: Duration = Duration::from_secs(600);

/// Só para teste: encurta o limite (em segundos) para o aviso caber no tempo do CI.
const LIMITE_ENV: &str = "MEXERICA_MOTOR_MUDO_SEGUNDOS";

#[derive(Debug, PartialEq, Eq)]
enum Aviso {
    Nenhum,
    Ativo(String),
    Encerrado,
}

pub(super) struct Vigia {
    limite: Duration,
    ultimo_sinal: Option<Instant>,
    aviso: Aviso,
}

/// O que, no ator, justifica o silêncio neste momento: motivos independentes, não estados excludentes.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
pub(super) struct Quietude {
    pub turno_aberto: bool,
    pub permissao_pendente: bool,
    pub espera_de_limite: bool,
    pub ferramenta_em_curso: bool,
    pub compactando: bool,
    pub parando: bool,
    pub rotina: bool,
}

impl Quietude {
    fn justificada(&self) -> bool {
        self.permissao_pendente
            || self.espera_de_limite
            || self.ferramenta_em_curso
            || self.compactando
            || self.parando
            || self.rotina
    }
}

impl Vigia {
    pub(super) fn new() -> Self {
        let limite = std::env::var(LIMITE_ENV)
            .ok()
            .and_then(|valor| valor.parse::<u64>().ok())
            .filter(|segundos| *segundos > 0)
            .map_or(LIMITE_PADRAO, Duration::from_secs);
        Self { limite, ultimo_sinal: None, aviso: Aviso::Nenhum }
    }

    /// O pedido `session/prompt` foi enviado: o relógio começa agora.
    fn armar(&mut self, agora: Instant) {
        self.ultimo_sinal = Some(agora);
    }

    /// Decide se é hora de avisar. Um aviso por turno: depois de emitido (ou encerrado) não volta. Enquanto algo
    /// justifica a quietude o relógio fica parado, para o silêncio só contar depois que a justificativa acaba.
    fn deve_avisar(&mut self, agora: Instant, quietude: &Quietude) -> bool {
        let Some(sinal) = self.ultimo_sinal else { return false };
        if quietude.justificada() {
            self.ultimo_sinal = Some(agora);
            return false;
        }
        self.aviso == Aviso::Nenhum && quietude.turno_aberto && agora.saturating_duration_since(sinal) >= self.limite
    }
}

impl Actor {
    /// Turno novo: zera o relógio e fecha o aviso do turno anterior, se sobrou algum.
    pub(super) fn mudo_iniciar(&mut self) {
        self.mudo_encerrar();
        self.mudo.ultimo_sinal = None;
        self.mudo.aviso = Aviso::Nenhum;
    }

    pub(super) fn mudo_armar(&mut self) {
        self.mudo.armar(Instant::now());
    }

    /// O motor disse algo: o silêncio recomeça e, se havia aviso, ele passa a dizer que a conversa seguiu.
    pub(super) fn mudo_sinal(&mut self) {
        if self.mudo.ultimo_sinal.is_none() {
            return;
        }
        self.mudo.ultimo_sinal = Some(Instant::now());
        self.mudo_encerrar();
    }

    fn mudo_encerrar(&mut self) {
        let Aviso::Ativo(id) = &self.mudo.aviso else { return };
        let id = id.clone();
        self.mudo.aviso = Aviso::Encerrado;
        self.mudo_fechar_aviso(&id);
    }

    fn mudo_chave(&self) -> String {
        format!("motor_mudo.ativo.{}", self.cfg.id)
    }

    fn mudo_fechar_aviso(&self, id: &str) {
        if let Some(mut entry) = self.hub.store.entry(id) {
            entry.data["motorMudo"]["estado"] = "voltou".into();
            entry.data["text"] = TEXTO_VOLTOU.into();
            if self.hub.set_entry(id, &entry.data).is_none() {
                tracing::warn!("não foi possível fechar o aviso de motor mudo; o registro fica para a próxima partida");
                return;
            }
        }
        if let Err(error) = self.hub.store.kv_delete(&self.mudo_chave()) {
            tracing::warn!(%error, "não foi possível limpar o registro do aviso de motor mudo");
        }
    }

    /// Hub reiniciado com um aviso ativo: o ator novo não o conhece, então o registro durável o fecha na partida.
    pub(super) fn mudo_restaurar(&self) {
        if let Some(id) = self.hub.store.kv_get(&self.mudo_chave()).filter(|id| !id.is_empty()) {
            self.mudo_fechar_aviso(&id);
        }
    }

    /// Chamado a cada batida do relógio do ator.
    pub(super) fn mudo_conferir(&mut self) {
        let turno = self.turn;
        if turno.is_none() {
            self.mudo_encerrar();
            return;
        }
        let quietude = Quietude {
            turno_aberto: true,
            permissao_pendente: !self.perms.is_empty(),
            espera_de_limite: self.waiting_for_reset(),
            ferramenta_em_curso: self.ferramenta_em_curso(),
            compactando: self.compacting.is_some()
                || self
                    .conn
                    .as_ref()
                    .is_some_and(|conn| matches!(conn.compaction_usage, CompactionUsage::Running { .. })),
            parando: self.stop_requested || self.cancel_deadline.is_some(),
            rotina: self.active_routine.is_some(),
        };
        let agora = Instant::now();
        if !self.mudo.deve_avisar(agora, &quietude) {
            return;
        }
        let segundos = self.mudo.limite.as_secs();
        let data = json!({
            "text": texto_silencio(segundos),
            "style": "info",
            "author": self.cfg.id,
            "motorMudo": {"estado": "mudo", "segundos": segundos},
        });
        let Some(entry) = self.add(EntryKind::Notice, turno.unwrap_or_default(), data) else {
            tracing::error!("não foi possível salvar o aviso de motor mudo");
            return;
        };
        if let Err(error) = self.hub.store.kv_set(&self.mudo_chave(), &entry.id) {
            tracing::warn!(%error, "não foi possível registrar o aviso de motor mudo");
        }
        self.mudo.aviso = Aviso::Ativo(entry.id);
    }

    fn ferramenta_em_curso(&self) -> bool {
        self.tools.values().any(|id| {
            self.hub
                .store
                .entry(id)
                .is_some_and(|entry| matches!(entry.data["status"].as_str(), Some("pending" | "in_progress")))
        })
    }
}

const TEXTO_VOLTOU: &str = "O bot voltou a dar sinal ou o turno terminou; este aviso não vale mais.";

/// Texto de reserva para app que não conhece o campo `motorMudo`; o app novo mostra a frase do seu catálogo.
fn texto_silencio(segundos: u64) -> String {
    let minutos = (segundos / 60).max(1);
    let unidade = if minutos == 1 { "minuto" } else { "minutos" };
    format!("O bot está há {minutos} {unidade} sem dar sinal. Se parecer travado, use Parar.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vigia() -> Vigia {
        Vigia { limite: Duration::from_secs(600), ultimo_sinal: None, aviso: Aviso::Nenhum }
    }

    fn aberto() -> Quietude {
        Quietude { turno_aberto: true, ..Quietude::default() }
    }

    #[tokio::test(start_paused = true)]
    async fn avisa_so_depois_do_limite_sem_sinal() {
        let mut vigia = vigia();
        vigia.armar(Instant::now());
        tokio::time::advance(Duration::from_secs(599)).await;
        assert!(!vigia.deve_avisar(Instant::now(), &aberto()));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(vigia.deve_avisar(Instant::now(), &aberto()));
    }

    #[tokio::test(start_paused = true)]
    async fn sem_pedido_aberto_nao_avisa() {
        let mut vigia = vigia();
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert!(!vigia.deve_avisar(Instant::now(), &aberto()), "relógio não armado");
    }

    #[tokio::test(start_paused = true)]
    async fn turno_fechado_nao_avisa() {
        let mut vigia = vigia();
        vigia.armar(Instant::now());
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert!(!vigia.deve_avisar(Instant::now(), &Quietude::default()));
    }

    #[tokio::test(start_paused = true)]
    async fn cada_motivo_de_quietude_segura_o_aviso() {
        let motivos: [fn(&mut Quietude); 6] = [
            |q| q.permissao_pendente = true,
            |q| q.espera_de_limite = true,
            |q| q.ferramenta_em_curso = true,
            |q| q.compactando = true,
            |q| q.parando = true,
            |q| q.rotina = true,
        ];
        for (i, motivo) in motivos.iter().enumerate() {
            let mut vigia = vigia();
            vigia.armar(Instant::now());
            tokio::time::advance(Duration::from_secs(3600)).await;
            let mut quietude = aberto();
            motivo(&mut quietude);
            assert!(!vigia.deve_avisar(Instant::now(), &quietude), "motivo {i}");
        }
        let mut vigia = vigia();
        vigia.armar(Instant::now());
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert!(vigia.deve_avisar(Instant::now(), &aberto()), "sem motivo, o mesmo silêncio avisa");
    }

    #[tokio::test(start_paused = true)]
    async fn silencio_so_conta_depois_que_a_justificativa_acaba() {
        let mut vigia = vigia();
        vigia.armar(Instant::now());
        let cartao = Quietude { permissao_pendente: true, ..aberto() };
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert!(!vigia.deve_avisar(Instant::now(), &cartao));
        assert!(!vigia.deve_avisar(Instant::now(), &aberto()), "cartão respondido agora: o relógio recomeça");
        tokio::time::advance(Duration::from_secs(600)).await;
        assert!(vigia.deve_avisar(Instant::now(), &aberto()));
    }

    #[tokio::test(start_paused = true)]
    async fn aviso_emitido_nao_se_repete_no_turno() {
        let mut vigia = vigia();
        vigia.armar(Instant::now());
        tokio::time::advance(Duration::from_secs(700)).await;
        vigia.aviso = Aviso::Ativo("n1".into());
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert!(!vigia.deve_avisar(Instant::now(), &aberto()));
        vigia.aviso = Aviso::Encerrado;
        assert!(!vigia.deve_avisar(Instant::now(), &aberto()), "voltar e calar de novo não gera segundo aviso");
    }

    #[tokio::test(start_paused = true)]
    async fn sinal_do_motor_recomeca_a_contagem() {
        let mut vigia = vigia();
        vigia.armar(Instant::now());
        tokio::time::advance(Duration::from_secs(500)).await;
        vigia.ultimo_sinal = Some(Instant::now());
        tokio::time::advance(Duration::from_secs(500)).await;
        assert!(!vigia.deve_avisar(Instant::now(), &aberto()));
        tokio::time::advance(Duration::from_secs(100)).await;
        assert!(vigia.deve_avisar(Instant::now(), &aberto()));
    }

    #[test]
    fn texto_de_reserva_usa_minutos() {
        assert_eq!(texto_silencio(600), "O bot está há 10 minutos sem dar sinal. Se parecer travado, use Parar.");
        assert_eq!(texto_silencio(60), "O bot está há 1 minuto sem dar sinal. Se parecer travado, use Parar.");
    }
}
