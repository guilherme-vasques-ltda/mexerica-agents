//! O perfil isolado de cada plano e o ambiente do processo do OpenCode.
//!
//! O OpenCode do plano roda com a casa e as pastas de configuração, dados, estado e cache dentro de uma pasta só do
//! plano: não enxerga o login, as chaves nem a configuração do OpenCode do dono. A chave segue por variável de
//! ambiente, só para este processo, e nunca é escrita em arquivo.
use super::{Plano, plano_do_modelo, revisao, slot_chave};
use crate::{hub::Hub, store::BotConfig, store::Store};
use anyhow::{Result, anyhow, ensure};
use std::{path::PathBuf, sync::Arc};
use zeroize::Zeroizing;

/// O que o Hub entrega ao processo do OpenCode de um bot ligado a um plano.
pub(crate) struct Ambiente {
    pub(crate) env: Vec<(String, String)>,
    /// Entra no carimbo da sessão: trocar ou remover a chave reinicia o processo.
    pub(crate) carimbo: String,
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
/// Fecha por padrão: plano sem chave, bot fora do Space pessoal ou executor isolado é erro, nunca o ambiente do dono.
pub(crate) async fn ambiente_do_bot(hub: &Arc<Hub>, cfg: &BotConfig) -> Result<Option<Ambiente>> {
    if cfg.backend != "opencode" {
        return Ok(None);
    }
    let Some(plano) = cfg.model.as_deref().and_then(plano_do_modelo) else { return Ok(None) };
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
    crate::market::vault::unlock(hub.clone()).await?;
    let chave = crate::market::vault::read(&hub.store, &slot_chave(plano))?
        .map(Zeroizing::new)
        .ok_or_else(|| anyhow!("plano_sem_chave: cadastre a chave do plano em Conexões de IA."))?;
    let home = casa(&hub.store, plano)?;
    let pasta = |relativo: &str| home.join(relativo).to_string_lossy().into_owned();
    let env = vec![
        ("HOME".into(), home.to_string_lossy().into_owned()),
        ("USERPROFILE".into(), home.to_string_lossy().into_owned()),
        ("XDG_CONFIG_HOME".into(), pasta(".config")),
        ("XDG_DATA_HOME".into(), pasta(".local/share")),
        ("XDG_STATE_HOME".into(), pasta(".local/state")),
        ("XDG_CACHE_HOME".into(), pasta(".cache")),
        (plano.variavel.into(), chave.to_string()),
    ];
    Ok(Some(Ambiente { env, carimbo: format!("plano:{}:{}", plano.id, revisao(&hub.store, plano)?) }))
}
