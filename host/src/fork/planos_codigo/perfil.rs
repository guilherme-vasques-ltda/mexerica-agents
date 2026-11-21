//! O perfil isolado de cada plano e o ambiente do processo do OpenCode.
//!
//! O OpenCode do plano roda com a casa e as pastas de configuração, dados, estado e cache dentro de uma pasta só do
//! plano: não enxerga o login, as chaves nem a configuração do OpenCode do dono, nem a configuração do projeto. A
//! chave de verdade não entra no processo: no lugar dela o OpenCode recebe um código de sessão e o endereço do repasse
//! local do Hub (`repasse.rs`), que troca o código pela chave ao falar com o fornecedor.
use super::{Plano, plano_do_modelo, repasse::Concessao, repasse_do_hub, revisao, slot_chave};
use crate::{hub::Hub, store::BotConfig, store::Store};
use anyhow::{Result, anyhow, bail, ensure};
use std::{path::PathBuf, sync::Arc};
use zeroize::Zeroizing;

/// O que o Hub entrega ao processo do OpenCode de um bot ligado a um plano.
pub(crate) struct Ambiente {
    pub(crate) env: Vec<(String, String)>,
    /// Entra no carimbo da sessão: trocar ou remover a chave reinicia o processo.
    pub(crate) carimbo: String,
    /// A sessão do repasse: guardada junto do processo, que a perde (e o código morre) quando é encerrado ou trocado.
    pub(crate) concessao: Concessao,
}

fn raiz(store: &Store, plano: &Plano) -> PathBuf {
    store.engine_setup_root.join("planos").join(plano.id)
}

/// Cria (ou confere) a casa do plano, uma pasta por nível, sem link simbólico e só do dono.
fn casa(store: &Store, plano: &Plano) -> Result<PathBuf> {
    let mut caminho = store.engine_setup_root.clone();
    for parte in ["planos", plano.id, "home"] {
        caminho.push(parte);
        crate::fork::engine_accounts::managed::private_dir(&caminho)?;
    }
    let home = caminho.clone();
    for parte in [".config", ".local", ".local/share", ".local/state", ".cache"] {
        crate::fork::engine_accounts::managed::private_dir(&home.join(parte))?;
    }
    Ok(home)
}

/// Apaga o perfil do plano. Falhar não impede a remoção da chave: o perfil não guarda chave.
pub(super) fn apagar(store: &Store, plano: &Plano) {
    let raiz = raiz(store, plano);
    if std::fs::symlink_metadata(&raiz).is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        && let Err(erro) = std::fs::remove_dir_all(&raiz)
    {
        tracing::warn!(plano = plano.id, %erro, "não deu para apagar o perfil do plano");
    }
}

/// O ambiente do OpenCode para este bot, ou `None` quando o modelo não é de um plano (o caminho de sempre).
/// Fecha por padrão: plano desligado ou sem chave, comando próprio, bot fora do Space pessoal ou executor isolado é
/// erro, nunca o ambiente do dono.
pub(crate) async fn ambiente_do_bot(hub: &Arc<Hub>, cfg: &BotConfig) -> Result<Option<Ambiente>> {
    if cfg.backend != "opencode" {
        return Ok(None);
    }
    let Some(plano) = cfg.model.as_deref().and_then(plano_do_modelo) else { return Ok(None) };
    ensure!(
        cfg.command.as_deref().is_none_or(|comando| comando.trim().is_empty()),
        "plano_comando_proprio: um bot com plano de código só roda o OpenCode oficial, sem comando próprio."
    );
    let pessoal = cfg.space_id == crate::fork::spaces::PERSONAL
        && cfg.context.is_none()
        && hub.store.bot(&cfg.id)?.is_some_and(|row| {
            !row.deleted && row.config.space_id == crate::fork::spaces::PERSONAL && row.config.context.is_none()
        });
    ensure!(pessoal, "plano_so_pessoal: a chave do plano só serve aos bots do Space pessoal.");
    ensure!(
        !crate::fork::motor_exec::configured(),
        "plano_indisponivel: os planos de código ainda não rodam no executor isolado."
    );
    ensure!(!cfg!(windows), "plano_indisponivel: os planos de código só funcionam em Linux e macOS nesta versão.");
    crate::market::vault::unlock(hub.clone()).await?;
    let repasse = repasse_do_hub(hub).await?;
    // A geração e a revisão são lidas ANTES da chave, e a sessão só é concedida se a geração não mudou: uma troca ou
    // remoção no meio (que sobe a geração e recusa a sessão durante a gravação) recusa a concessão, nunca deixa viva
    // uma chave velha.
    let mut recusa = None;
    for _ in 0..3 {
        let geracao = repasse.geracao(plano);
        let revisao = revisao(&hub.store, plano)?;
        let slot = slot_chave(plano);
        let selada = hub
            .store
            .kv_read(&slot)?
            .ok_or_else(|| anyhow!("plano_sem_chave: cadastre a chave do plano em Conexões de IA."))?;
        let chave = Zeroizing::new(crate::market::vault::open_ciphertext(&hub.store, &slot, &selada)?);
        let concessao = match repasse.conceder(plano, chave, geracao) {
            Ok(concessao) => concessao,
            Err(erro) => {
                tracing::debug!(plano = plano.id, motivo = %erro, "o repasse recusou a sessão do plano");
                recusa = Some(erro.to_string());
                continue;
            }
        };
        let home = casa(&hub.store, plano)?;
        let pasta = |relativo: &str| home.join(relativo).to_string_lossy().into_owned();
        let configuracao =
            serde_json::json!({"provider": {plano.provedor: {"options": {"baseURL": concessao.base()}}}});
        let env = vec![
            ("HOME".into(), home.to_string_lossy().into_owned()),
            ("USERPROFILE".into(), home.to_string_lossy().into_owned()),
            ("XDG_CONFIG_HOME".into(), pasta(".config")),
            ("XDG_DATA_HOME".into(), pasta(".local/share")),
            ("XDG_STATE_HOME".into(), pasta(".local/state")),
            ("XDG_CACHE_HOME".into(), pasta(".cache")),
            (plano.variavel.into(), concessao.codigo().to_owned()),
            ("OPENCODE_CONFIG_CONTENT".into(), configuracao.to_string()),
            // A configuração e os plugins do projeto (conteúdo do repositório, não do dono) não valem no plano.
            ("OPENCODE_DISABLE_PROJECT_CONFIG".into(), "1".into()),
        ];
        return Ok(Some(Ambiente { env, carimbo: format!("plano:{}:{revisao}", plano.id), concessao }));
    }
    bail!("plano_em_troca: não deu para abrir a sessão do plano ({}); tente de novo.", recusa.unwrap_or_default())
}

/// Aplica o filtro do motor (que tira casa e pastas `XDG_*` herdadas) e só depois entrega o ambiente do plano: o
/// perfil e o código de sessão do plano são sempre os últimos a valer, nunca os do dono.
pub(crate) fn aplicar_ambiente(env: &mut Vec<(String, String)>, backend: &str, system: &str, plano: Option<&Ambiente>) {
    crate::agent::harness::apply(env, backend, system);
    let Some(plano) = plano else { return };
    env.retain(|(nome, _)| !plano.env.iter().any(|(do_plano, _)| do_plano == nome));
    env.extend(plano.env.iter().cloned());
}
