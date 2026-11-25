//! Ligação ao caminho de partida do Motor no macOS no perfil rc.2 —
//! DORMENTE enquanto a recusa #117 estiver ativa. O caminho de produção
//! vigente continua negando antes de qualquer Motor/MCP (`ensure_runnable`,
//! recusa `motor_macos_indisponivel`); esta ligação não tem chamador de
//! produção e não existe fonte de entrada rc.2 no Hub hoje. Sem a retirada
//! da recusa e sem entrada verificada do runtime rc.2, nada inicia.
//!
//! Contrato da ligação: o Motor e seus descendentes só iniciam por admissão
//! verificada — identidade do artefato e do par em ARM64, partida sob perfil
//! Seatbelt comprovado cobrindo o motor e cada descendente, capacidades só
//! mediadas (sudo sempre negado, shell desligado por padrão, tela
//! desligada). Toda recusa do caminho de admissão vira o código estável
//! `motor_macos_indisponivel` antes de Motor/MCP. A unidade admitida carrega
//! a porta auditada e o cofre da geração: "Parar tudo" fecha os dois juntos
//! — o fechamento nunca é negado nem desfeito; a gravação durável da
//! pendência ocorre no mesmo caminho e pode custar tempo, mas a parada não
//! depende dela para valer. A parada confirmada exige evidência dentro do
//! prazo de 2 s — sobrevivente é visível, nunca sucesso falso.
//!
//! Gates de ativação, declarados e NÃO provados aqui: o Motor nasce
//! SUSPENSO pelo supervisor — antes de retomar, o supervisor OBSERVA PID
//! e início, confere o PID contra o admitido no censo da admissão e
//! registra a instância no censo; nenhum descendente nasce fora do
//! censo, e PID reutilizado não re-cria a marca. A saída é verificada
//! antes de confirmada e o grupo ou a sessão do Motor e de cada
//! descendente seguem vazios. Essas provas são nativas no macOS e
//! precedem a retirada da recusa #117; os dublês das fixtures só
//! exercitam o contrato.

use std::num::NonZeroU64;

use super::auditoria::{Falha, PortaAuditada};
use super::cofre;
use super::identidade;
use super::partida;
use super::porta::{Decisao, Escopo, Pedido};
use super::recusas::Registro;

/// Entrada de identidade de uma tentativa de partida rc.2: manifesto
/// esperado, geração, e as observações do artefato e do par validados.
pub(super) struct IdentidadeEntrada<'a> {
    pub(super) manifesto: identidade::Artefato<'a>,
    pub(super) geracao: NonZeroU64,
    pub(super) artefato: identidade::Observacao<identidade::Artefato<'a>>,
    pub(super) par: identidade::Observacao<identidade::ParValidado<'a>>,
}

/// Entrada de partida: perfil Seatbelt esperado, observações de perfil e
/// censo, e as capacidades pedidas — só a mediada existe nesta candidata.
pub(super) struct PartidaEntrada<'a> {
    pub(super) esperado: partida::Perfil<'a>,
    pub(super) perfil: partida::ObservacaoPerfil<'a>,
    pub(super) censo: partida::ObservacaoCenso<'a>,
    pub(super) capacidades: Vec<partida::Capacidade>,
}

/// Unidade rc.2 admitida: partida move-only, porta auditada e cofre da
/// geração. Só existe com identidade e partida verificadas e coerentes com o
/// escopo; o Motor e seus descendentes não iniciam sem ela. A unidade segue
/// viva como supervisora depois de entregar a partida: porta, cofre e censo
/// continuam sob seu controle, e a partida é recusada após a parada.
pub(super) struct UnidadeRc2<'a> {
    motor_pid: NonZeroU64,
    partida: Option<partida::Partida<'a>>,
    porta: PortaAuditada<'a>,
    cofre: cofre::Cofre<'a>,
    parada: bool,
}

/// Regra permanente da ligação: "Sempre permitir" não existe para shell —
/// proibição categórica, distinta da indisponibilidade das outras.
pub(super) fn regra_permanente(capacidade: partida::Capacidade) -> Result<(), cofre::RecusaRegra> {
    cofre::regra_permanente(capacidade)
}

/// Recusa estável da ligação: sempre o mesmo código, antes de Motor/MCP.
pub(super) fn recusa_estavel_de_identidade(recusa: &identidade::Recusa) -> anyhow::Error {
    let motivo = match recusa {
        identidade::Recusa::Manifesto => "manifesto inválido",
        identidade::Recusa::SemArtefatoArm64 => "artefato arm64 ausente",
        identidade::Recusa::ArtefatoDivergente => "artefato divergente do manifesto",
        identidade::Recusa::SemParArm64 => "par arm64 ausente",
        identidade::Recusa::ParDivergente => "par divergente do manifesto",
        identidade::Recusa::Geracao => "geração divergente",
    };
    anyhow::anyhow!("{}: {motivo}", partida::CODIGO)
}

impl std::fmt::Debug for UnidadeRc2<'_> {
    /// Projeção fixa: a unidade nunca vaza conteúdo de porta, cofre ou
    /// partida em mensagens de erro.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UnidadeRc2 (partida, porta e cofre da geração; sem projeção de conteúdo)")
    }
}

fn recusa_estavel_de_incoerencia() -> anyhow::Error {
    anyhow::anyhow!("{}: escopo incoerente com a admissão", partida::CODIGO)
}

/// Recusa estável da partida: mesmo código, motivo tipado.
fn recusa_de_partida(motivo: &'static str) -> anyhow::Error {
    anyhow::anyhow!("{}: {motivo}", partida::CODIGO)
}

/// Abre a tentativa de partida rc.2. Falha-closed: identidade primeiro,
/// partida depois, coerência de escopo por último — toda recusa vira o
/// código estável antes de qualquer porta, cofre ou efeito existir.
pub(super) fn iniciar<'a>(
    identidade_entrada: &IdentidadeEntrada<'a>,
    partida_entrada: PartidaEntrada<'a>,
    escopo: Escopo,
    instancia: NonZeroU64,
    registro: &'a Registro,
    vinculo: cofre::Vinculo<'a>,
) -> Result<UnidadeRc2<'a>, anyhow::Error> {
    identidade::admitir(
        &identidade_entrada.manifesto,
        identidade_entrada.geracao,
        identidade_entrada.artefato,
        identidade_entrada.par,
    )
    .map_err(|recusa| recusa_estavel_de_identidade(&recusa))?;
    // Identidade admitida da instância principal: o PID do Motor observado
    // no censo da admissão. É a ele que a marca do censo fica fixada —
    // registrar motor divergente não marca e não substitui a marca.
    let motor_pid = match &partida_entrada.censo {
        partida::ObservacaoCenso::Observado { motor, .. } => Some(motor.pid),
        partida::ObservacaoCenso::SemCenso => None,
    };
    let partida_verificada = partida::partir(
        partida_entrada.esperado,
        partida_entrada.perfil,
        partida_entrada.censo,
        &partida_entrada.capacidades,
    )
    .map_err(partida::recusa_estavel)?;
    // Sem censo observado a partida já foi recusada com barreira acima.
    let Some(motor_pid) = motor_pid else {
        return Err(partida::recusa_estavel(partida::Recusa::Barreira));
    };
    if escopo.geracao != identidade_entrada.geracao || vinculo.escopo() != escopo {
        return Err(recusa_estavel_de_incoerencia());
    }
    let porta = PortaAuditada::nova(escopo, instancia, registro);
    let cofre = cofre::Cofre::abrir(vinculo);
    Ok(UnidadeRc2 { motor_pid, partida: Some(partida_verificada), porta, cofre, parada: false })
}

impl<'a> UnidadeRc2<'a> {
    /// Fechamento unificado: quando a porta fecha por QUALQUER caminho —
    /// "Parar tudo" ou falha de auditoria — o cofre da geração fecha junto
    /// e a partida é recusada dali em diante.
    fn sincronizar_fechamento(&mut self) {
        if !self.parada && self.porta.fechada() {
            self.cofre.parar();
            self.parada = true;
        }
    }

    /// Aceite do aviso versionado daquele bot: sem aceite, a porta não abre
    /// pedido.
    pub(super) fn aceitar_aviso(&mut self, escopo: Escopo) -> Result<(), Falha> {
        let resultado = self.porta.aceitar_aviso(escopo);
        self.sincronizar_fechamento();
        resultado
    }

    /// Partida verificada da unidade: sai UMA única vez, EMPRESTADA, no
    /// ponto do efeito. A unidade NÃO deixa de existir: segue supervisora
    /// com porta, cofre e censo, e a partida é recusada após a parada ou se
    /// já foi executada — nada parte duas vezes, e nada parte depois de
    /// "Parar tudo".
    /// Efeito de partida: a checagem e o início são a MESMA operação da
    /// unidade — quem parte o Motor entrega o efeito para cá; a unidade
    /// revalida (aberta, aceite feito, motor registrado no censo) e executa
    /// DENTRO da operação, EMPRESTANDO a concessão por `&mut` e
    /// descartando a partida no mesmo passo. A concessão não escapa para
    /// uso posterior: o efeito só vê o empréstimo, que vive dentro da
    /// chamada — reter a partida fora dela não compila; o contrato é
    /// fixado e EXECUTADO pelo pino `exige_emprestimo` dos testes
    /// (`tests/fork_motor_macos_ligacao.rs`), que empresta a concessão de
    /// verdade e quebra a suite numa variante por valor.
    /// O lançamento EFETIVO do Motor exige a autorização mediada pela
    /// unidade e é gate pré-ativação: só sai da dormência depois de
    /// comprovado nativamente no macOS — o Motor nasce SUSPENSO pelo
    /// supervisor; antes de retomar, o supervisor OBSERVA PID e início,
    /// confere o PID contra o admitido no censo da admissão e registra a
    /// instância no censo, para que nenhum descendente nasça fora dele;
    /// saída verificada antes de confirmada e grupo ou sessão vazios do
    /// Motor e de cada descendente. A fixture usa dublês: o censo exigido
    /// aqui é CONTRATO, não prova do processo real.
    pub(super) fn partir_para_o_motor(
        &mut self,
        efeito: impl FnOnce(&mut partida::Partida<'a>),
    ) -> Result<(), anyhow::Error> {
        if self.parada || self.porta.fechada() {
            return Err(recusa_de_partida("unidade parada ou porta fechada"));
        }
        if !self.porta.aceita() {
            return Err(recusa_de_partida("partida exige o aceite do aviso"));
        }
        if !self.porta.motor_no_censo() {
            return Err(recusa_de_partida("partida exige o motor registrado no censo"));
        }
        let Some(mut partida) = self.partida.take() else {
            return Err(recusa_de_partida("partida já executada"));
        };
        efeito(&mut partida);
        Ok(())
    }

    /// Registra descendente observado no censo de alvos da parada: torna
    /// sobrevivente visível na parada — não abre partida. CONTRATO: em
    /// runtime a instância vem do supervisor (PID e início OBSERVADOS),
    /// não do chamador livre; a entrada da fixture é dublê.
    pub(super) fn registrar(&mut self, instancia: super::porta::Instancia) -> bool {
        self.porta.registrar(instancia)
    }

    /// Registra a instância principal observada (o Motor), FIXADA à
    /// identidade admitida: só o PID do motor do censo da admissão marca;
    /// instância divergente não marca, não substitui marca existente e
    /// descendente não é promovido. O início é fixado na primeira
    /// observação: mesma PID com início diferente não vale — nem para
    /// substituir, nem para re-criar a marca após a saída confirmada (PID
    /// reutilizado é outro processo). É a única marca que abre a partida.
    /// CONTRATO: em runtime o Motor nasce SUSPENSO pelo supervisor; antes
    /// de retomar, o supervisor OBSERVA PID e início, confere o PID contra
    /// o admitido e registra a instância; a fixture é dublê.
    pub(super) fn registrar_motor(&mut self, instancia: super::porta::Instancia) -> bool {
        if instancia.pid != self.motor_pid {
            return false;
        }
        self.porta.registrar_motor(instancia)
    }

    /// Confirma a saída de uma instância registrada; a saída confirmada do
    /// motor retira a marca que abre a partida. CONTRATO: em runtime a
    /// saída é verificada pelo supervisor antes de confirmar; a
    /// confirmação da fixture não prova a morte de processo real.
    pub(super) fn confirmar_saida(&mut self, instancia: super::porta::Instancia) -> bool {
        self.porta.confirmar_saida(instancia)
    }

    /// Caminho de `pedir` da porta auditada. Falha de gravação fecha a
    /// porta — e o fechamento unificado fecha o cofre junto.
    pub(super) fn pedir(&mut self, acao: super::porta::Acao, agora_ms: u64) -> Result<Pedido, Falha> {
        let resultado = self.porta.pedir(acao, agora_ms);
        self.sincronizar_fechamento();
        resultado
    }

    /// Caminho de `resolver` da porta auditada, com fechamento unificado.
    pub(super) fn resolver(
        &mut self,
        pedido: Pedido,
        respondente: Escopo,
        decisao: Decisao,
        agora_ms: u64,
    ) -> Result<(), Falha> {
        let resultado = self.porta.resolver(pedido, respondente, decisao, agora_ms);
        self.sincronizar_fechamento();
        resultado
    }

    /// Caminho de `efetuar` da porta auditada: a intenção é gravada antes do
    /// efeito; gravação falha fecha a porta — e o fechamento unificado
    /// fecha o cofre junto, sem efeito e sem retry.
    pub(super) fn efetuar(
        &mut self,
        pedido: Pedido,
        acao: super::porta::Acao,
        agora_ms: impl FnMut() -> u64,
        efeito_imediato: impl FnOnce(super::porta::Acao),
    ) -> Result<(), Falha> {
        let resultado = self.porta.efetuar(pedido, acao, agora_ms, efeito_imediato);
        self.sincronizar_fechamento();
        resultado
    }

    /// Autoriza inferência pelo cofre da geração: item de outro app negado;
    /// identidade exata do item; nenhuma autorização atravessa gerações.
    pub(super) fn autorizar_inferencia(&self, pedido: cofre::Pedido<'_>) -> Result<cofre::Autorizacao, cofre::Recusa> {
        self.cofre.autorizar(pedido)
    }

    /// Redenção da autorização: SÓ pelo cofre da geração ainda aberto — a
    /// emitida antes da parada morre com ela, nunca por estado do portador.
    pub(super) fn redimir(&self, autorizacao: cofre::Autorizacao) -> Result<NonZeroU64, cofre::Recusa> {
        self.cofre.redimir(autorizacao)
    }

    /// "Parar tudo" da unidade: o cofre da geração fecha PRIMEIRO,
    /// incondicional — nenhuma gravação de auditoria, que pode custar
    /// tempo ou falhar, precede o fechamento do cofre, e o prazo de 2 s
    /// não depende dela. Depois a porta fecha (gravando a pendência como
    /// `fechada`: a gravação pode custar tempo e, se falhar, deixa o
    /// fechamento valendo — nunca é negado nem desfeito). A partida é
    /// recusada dali em diante.
    pub(super) fn parar_tudo(&mut self, agora_ms: u64) -> Result<(), Falha> {
        self.cofre.parar();
        self.parada = true;
        let resultado = self.porta.parar(agora_ms);
        self.sincronizar_fechamento();
        resultado
    }

    /// Estado da parada com evidência: confirmada no prazo de 2 s só com
    /// unidade fechada e saídas confirmadas; sobrevivente é visível com
    /// identidade; sem evidência após o prazo é incerteza, nunca sucesso
    /// falso.
    pub(super) fn estado_da_parada(&self, agora_ms: u64, evidencia: super::porta::Evidencia) -> super::porta::Parada {
        self.porta.estado(agora_ms, evidencia)
    }
}
