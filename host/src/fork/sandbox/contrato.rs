//! Contrato único de provedor de sandbox (MX-175). Tipos e regras puras, sem efeitos.
//!
//! Todo executor (processo local, bwrap, Seatbelt e, depois, Instabox/Agent37/Maritime)
//! implementa [`Provedor`] e [`Sandbox`]. A escolha do executor vira dado; as regras de
//! admissão, limites e ciclo de vida ficam aqui, iguais para todos.
use anyhow::{Result, bail, ensure};
use std::{
    ffi::OsString,
    fmt,
    os::unix::ffi::OsStrExt as _,
    path::{Component, Path, PathBuf},
    time::Duration,
};

/// Quem controla o agente que vai rodar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Confianca {
    /// Bot ou agente do próprio dono (decisão do dono, MX-188).
    DoDono,
    /// Código ou agente de terceiros: só em máquina descartável.
    Desconhecida,
}

/// O que o provedor realmente contém, em ordem crescente de força.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Contencao {
    /// Processo comum no host, sem contenção. Só referência e bots do dono.
    Nenhuma,
    /// Arquivos e ambiente contidos no mesmo kernel (bwrap, Seatbelt). Rede aberta.
    ArquivosEAmbiente,
    /// Máquina ou VM descartável fora do host do Hub.
    MaquinaDescartavel,
}

/// Como a cópia de trabalho chega ao executor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Modo {
    /// A cópia vive no host e é montada dentro (processo local, bwrap, Seatbelt).
    Montagem,
    /// Sistema de arquivos próprio; entra e sai só por cópia (executores remotos).
    Isolado,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tipo {
    ProcessoLocal,
    Bwrap,
    Seatbelt,
}

/// Executor ausente ou inseguro. Nunca vira fallback para outro executor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Indisponivel {
    pub(crate) tipo: Tipo,
    pub(crate) motivo: String,
    /// Passo que a pessoa pode seguir para corrigir, quando houver.
    pub(crate) correcao: Option<String>,
}
impl fmt::Display for Indisponivel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "executor {:?} indisponível: {}", self.tipo, self.motivo)?;
        if let Some(correcao) = &self.correcao {
            write!(f, " (corrija com: {correcao})")?;
        }
        Ok(())
    }
}
impl std::error::Error for Indisponivel {}

/// Fail-closed: agente desconhecido nunca roda no host do Hub.
pub(crate) fn admitir(confianca: Confianca, contencao: Contencao) -> Result<()> {
    if confianca == Confianca::Desconhecida && contencao < Contencao::MaquinaDescartavel {
        bail!("agente_nao_confiavel_exige_maquina_descartavel");
    }
    Ok(())
}

/// Limites de uma execução. Sem limite implícito: todo campo é obrigatório.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limites {
    /// Tempo de parede até parar o grupo de processos.
    pub(crate) tempo: Duration,
    /// Teto somado de stdout e stderr guardados; o excedente é descartado.
    pub(crate) saida_bytes: usize,
    /// Espera entre SIGTERM e SIGKILL, e para drenar a saída após o fim.
    pub(crate) carencia: Duration,
}
impl Limites {
    pub(crate) const TEMPO_MIN: Duration = Duration::from_millis(100);
    pub(crate) const TEMPO_MAX: Duration = Duration::from_secs(24 * 60 * 60);
    pub(crate) const SAIDA_MAX: usize = 64 * 1024 * 1024;
    pub(crate) const CARENCIA_MAX: Duration = Duration::from_secs(60);

    pub(crate) fn validar(&self) -> Result<()> {
        ensure!((Self::TEMPO_MIN..=Self::TEMPO_MAX).contains(&self.tempo), "limite_tempo_invalido");
        ensure!((1..=Self::SAIDA_MAX).contains(&self.saida_bytes), "limite_saida_invalido");
        ensure!(self.carencia <= Self::CARENCIA_MAX, "limite_carencia_invalido");
        Ok(())
    }
}

/// Teto de cópia de um arquivo para dentro ou para fora.
pub(crate) const COPIA_MAX_BYTES: u64 = 256 * 1024 * 1024;
const ENV_MAX: usize = 128;
const ENV_VALOR_MAX: usize = 32 * 1024;

/// Variáveis definidas só pelo provedor (home/tmp privados, toolchain) ou que
/// injetam código no toolchain do host.
fn env_reservada(chave: &str) -> bool {
    matches!(chave, "HOME" | "TMPDIR" | "TMP" | "TEMP" | "PATH" | "USER" | "LOGNAME" | "SHELL")
        || chave.starts_with("LD_")
        || chave.starts_with("DYLD_")
}

/// Uma execução: argv literal (sem shell), ambiente explícito e pasta relativa à cópia.
#[derive(Clone, Debug)]
pub(crate) struct Execucao {
    pub(crate) argv: Vec<OsString>,
    /// Só estas variáveis entram, além das do provedor; nada é herdado do Hub.
    pub(crate) env: Vec<(String, OsString)>,
    /// Relativa à raiz da cópia; vazia é a própria raiz.
    pub(crate) pasta: PathBuf,
    pub(crate) limites: Limites,
}
impl Execucao {
    pub(crate) fn validar(&self) -> Result<()> {
        ensure!(self.argv.first().is_some_and(|p| !p.is_empty()), "argv_vazio");
        ensure!(self.argv.iter().all(|a| !a.as_bytes().contains(&0)), "argv_invalido");
        ensure!(self.env.len() <= ENV_MAX, "env_excessivo");
        for (index, (chave, valor)) in self.env.iter().enumerate() {
            ensure!(nome_env(chave), "env_nome_invalido");
            ensure!(!env_reservada(chave), "env_reservada");
            ensure!(valor.len() <= ENV_VALOR_MAX && !valor.as_bytes().contains(&0), "env_valor_invalido");
            ensure!(self.env[..index].iter().all(|(outra, _)| outra != chave), "env_duplicada");
        }
        relativo(&self.pasta)?;
        self.limites.validar()
    }
}

fn nome_env(chave: &str) -> bool {
    let bytes = chave.as_bytes();
    (1..=128).contains(&bytes.len())
        && !bytes[0].is_ascii_digit()
        && bytes.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

/// Caminho dentro da cópia: só componentes normais, nunca absoluto ou `..`.
pub(crate) fn relativo(caminho: &Path) -> Result<()> {
    ensure!(caminho.components().all(|c| matches!(c, Component::Normal(_))), "caminho_fora_da_copia");
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fim {
    Saiu(i32),
    Sinal(i32),
    /// Passou de [`Limites::tempo`]; o grupo foi parado.
    TempoEsgotado,
}

#[derive(Clone, Debug)]
pub(crate) struct Saida {
    pub(crate) fim: Fim,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    /// A saída passou de [`Limites::saida_bytes`].
    pub(crate) truncada: bool,
    /// Algum processo escapado do grupo segurou a saída após a carência.
    pub(crate) incompleta: bool,
    pub(crate) duracao: Duration,
}

/// Ciclo de vida comum. Execuções são sequenciais; cópias só fora de execução.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Estado {
    Preparado,
    Executando,
    /// Falha sem prova do que aconteceu: nunca executa de novo, só encerra.
    Desconhecido,
    Encerrado,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Passo {
    Executar,
    FimExecucao,
    Copiar,
    Falha,
    Encerrar,
}

pub(crate) fn avancar(estado: Estado, passo: Passo) -> Result<Estado> {
    use {Estado as E, Passo as P};
    Ok(match (estado, passo) {
        (E::Encerrado, _) => bail!("sandbox_encerrado"),
        (E::Preparado, P::Executar) => E::Executando,
        (E::Executando, P::FimExecucao) | (E::Preparado, P::Copiar) => E::Preparado,
        (_, P::Falha) => E::Desconhecido,
        (_, P::Encerrar) => E::Encerrado,
        (E::Desconhecido, _) => bail!("sandbox_em_estado_desconhecido"),
        (E::Executando, _) => bail!("sandbox_ocupado"),
        (E::Preparado, P::FimExecucao) => bail!("transicao_invalida"),
    })
}

/// Pedido de preparo: a cópia já existe no host (criada pelo Hub a partir de um commit).
#[derive(Clone, Debug)]
pub(crate) struct Preparo {
    pub(crate) copia: PathBuf,
    pub(crate) confianca: Confianca,
}

pub(crate) trait Provedor {
    type Sandbox: Sandbox;
    fn tipo(&self) -> Tipo;
    fn modo(&self) -> Modo;
    fn contencao(&self) -> Contencao;
    /// Confere ferramenta, plataforma e permissões; falha ruidosa, sem fallback.
    fn verificar(&self) -> Result<(), Indisponivel>;
    /// Admite pelo par confiança × contenção antes de qualquer efeito.
    async fn preparar(&self, preparo: &Preparo) -> Result<Self::Sandbox>;
}

pub(crate) trait Sandbox {
    fn id(&self) -> &str;
    fn estado(&self) -> Estado;
    /// Raiz da cópia vista pelo Hub (no host, para [`Modo::Montagem`]).
    fn copia(&self) -> &Path;
    async fn executar(&mut self, execucao: &Execucao) -> Result<Saida>;
    /// Copia um arquivo comum do host para `destino` relativo à cópia.
    async fn copiar_para_dentro(&mut self, origem: &Path, destino: &Path) -> Result<()>;
    /// Copia um arquivo comum da cópia para `destino` novo no host; nunca sobrescreve.
    async fn copiar_para_fora(&mut self, origem: &Path, destino: &Path) -> Result<()>;
    /// Para tudo e apaga home/tmp privados. A cópia fica para exportação e limpeza do Hub.
    async fn encerrar(&mut self) -> Result<()>;
}
