//! Fase 2b das Mexeriquinhas: gerador "Crie o seu" do estilo 3D executado pelo Hub.
//! A chave `OpenAI` do usuário fica no cofre (`chave`); o renderer só envia intenções e
//! renderiza estado. Sem credencial configurada nenhum caminho de geração roda.
use crate::LockExt;
use crate::api::devices::Caller;
use crate::hub::Hub;
use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) mod api;
mod atlas;
mod briefing;
mod chave;
pub(crate) mod conjuntos;
mod cota;
mod provedor;

#[cfg(test)]
mod tests;

/// Os nove quadros do conjunto, na ordem do atlas 3×3 (contrato da fase 2a).
pub(crate) const QUADROS: [&str; 9] = [
    "livre-1",
    "livre-2",
    "piscar",
    "trabalhando-1",
    "trabalhando-2",
    "precisa-resposta",
    "erro",
    "dormindo-1",
    "dormindo-2",
];

/// Tamanhos físicos gerados, como no catálogo.
pub(crate) const TAMANHOS: [u32; 2] = [64, 256];

/// Versão dos prompts incorporados; vai ao manifesto e aos logs, nunca o conteúdo.
const PROMPTS_VERSAO: &str = "2b.1";

static ESTILO_SISTEMA: &str =
    include_str!("../../../../apps/desktop/src/renderer/lib/mexeriquinhas/prompts/estilo-sistema.md");
static ATLAS_PROMPT: &str = include_str!("../../../../apps/desktop/src/renderer/lib/mexeriquinhas/prompts/atlas.md");
static VARIANTES: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../apps/desktop/src/renderer/lib/mexeriquinhas/prompts/variantes.json"))
        .expect("variantes do gerador incorporadas inválidas")
});
/// Referência própria fixa de geração; nenhuma entrada do usuário escolhe imagem.
static REFERENCIA: &[u8] =
    include_bytes!("../../../../apps/desktop/src/renderer/lib/mexeriquinhas/assets/base/referencia.png");

/// Erro com código estável para o renderer traduzir; nunca carrega corpo do provedor.
pub(crate) struct Falha(pub &'static str);
impl std::fmt::Display for Falha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::fmt::Debug for Falha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Falha {}
pub(crate) fn falha(codigo: &'static str) -> anyhow::Error {
    Falha(codigo).into()
}
pub(crate) fn codigo(e: &anyhow::Error) -> &'static str {
    e.downcast_ref::<Falha>().map_or("GERACAO_FALHOU", |f| f.0)
}

/// Estado do job exposto ao renderer; o conjunto só aparece em `ready`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Generating,
    Ready,
    Error,
    Cancelled,
}
impl Status {
    fn id(self) -> &'static str {
        match self {
            Self::Generating => "generating",
            Self::Ready => "ready",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }
}

pub(crate) struct Job {
    pub status: Status,
    pub codigo: Option<&'static str>,
    /// Nome de exibição da proposta; conjunto só ganha manifesto ao salvar.
    pub nome: String,
    pub criado_em: i64,
    /// Cancelamento cooperativo: o job confere entre as etapas pagas.
    pub cancelar: Arc<std::sync::atomic::AtomicBool>,
}

/// Um job ativo por Hub, como no protótipo; a reserva precede qualquer espera externa.
#[derive(Default)]
pub struct State {
    pub(crate) jobs: Mutex<Jobs>,
}
#[derive(Default)]
pub(crate) struct Jobs {
    pub ativo: bool,
    pub mapa: HashMap<uuid::Uuid, Job>,
}

/// Tempo de vida de um job encerrado na memória, para leitura tardia do status.
pub(crate) const JOB_TTL_MS: i64 = 30 * 60 * 1000;

/// assetId válido de conjunto gerado: uuid v4 canônico. Nome nunca entra em caminho.
pub(crate) fn id_gerado(id: &str) -> bool {
    super::asset_id_valido(id)
}

/// Entrada do grupo `mexeriquinha*` no despacho do fork. Só o app deste computador.
pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::fork::engine_setup::require_local(caller)?;
    api::call(hub, method, body).await
}

/// Tarefa do job: identidade → atlas → quadros. Erro e cancelamento são estados do job.
async fn executar_job(
    hub: Arc<Hub>,
    id: uuid::Uuid,
    briefing: briefing::Briefing,
    provedor: Arc<dyn provedor::Provedor>,
) {
    let pasta = conjuntos::pasta_job(&hub.store, &id);
    let resultado = executar(&hub, &id, &briefing, provedor.as_ref(), &pasta).await;
    {
        let mut jobs = hub.gerador.jobs.locked();
        if let Some(job) = jobs.mapa.get_mut(&id) {
            match resultado {
                Ok(()) => {
                    job.status = Status::Ready;
                    job.codigo = None;
                }
                Err(ref e) => {
                    if job.status != Status::Cancelled {
                        job.status = Status::Error;
                        job.codigo = Some(codigo(e));
                    }
                }
            }
        }
        jobs.ativo = false;
    }
    let status = if resultado.is_ok() { "ready" } else { "error" };
    if resultado.is_err() {
        // Falha não publica conjunto: a pasta de trabalho é descartada.
        let _ = std::fs::remove_dir_all(&pasta);
    }
    tracing::info!(job = %id, status, prompts = PROMPTS_VERSAO, "job do gerador encerrado");
}

fn cancelado(hub: &Arc<Hub>, id: &uuid::Uuid) -> bool {
    let jobs = hub.gerador.jobs.locked();
    match jobs.mapa.get(id) {
        None => true,
        Some(job) => job.status == Status::Cancelled || job.cancelar.load(std::sync::atomic::Ordering::Relaxed),
    }
}

async fn executar(
    hub: &Arc<Hub>,
    id: &uuid::Uuid,
    briefing: &briefing::Briefing,
    provedor: &dyn provedor::Provedor,
    pasta: &std::path::Path,
) -> Result<()> {
    std::fs::create_dir_all(pasta)?;
    conjuntos::privado(pasta)?;
    // Etapa 1: identidade nova a partir da referência fixa; a imagem do dono não sai daqui.
    let identidade = provedor
        .editar(REFERENCIA, &format!("{ESTILO_SISTEMA}\nBriefing visual JSON:\n{}", briefing.json()), "1024x1024")
        .await?;
    if cancelado(hub, id) {
        bail!("JOB_CANCELADO");
    }
    // Etapa 2: atlas 3×3 da identidade recém-gerada; nada é compartilhado entre jobs.
    let atlas_png = provedor.editar(&identidade, &format!("{ESTILO_SISTEMA}\n{ATLAS_PROMPT}"), "1536x1536").await?;
    if cancelado(hub, id) {
        bail!("JOB_CANCELADO");
    }
    let destino = pasta.to_path_buf();
    let medidas = tokio::task::spawn_blocking(move || atlas::processar(&atlas_png, &destino)).await??;
    tracing::info!(job = %id, bytes = medidas.bytes, "atlas do gerador processado");
    Ok(())
}
