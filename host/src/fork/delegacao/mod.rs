//! D371 (T1): regras de delegação escritas pelo dono, gerais e por área do quadro de Equipes, só no
//! Space do bot. Ler e gravar só pela RPC do dono (`api`); o bot recebe o texto apenas no início de cada
//! turno (`turno`), nunca em Canais. Orientação: não libera modelo, conta nem permissão.
pub(crate) mod api;
/// T2a (DON-912): contrato curto de tarefa em ask_bot/message_bot.
pub(crate) mod contrato;
pub(crate) mod pausa;
#[cfg(test)]
mod tests;

use crate::LockExt;
use crate::store::{BotConfig, Store};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension};

pub(crate) const LIMITE: usize = 4_000;
pub(crate) const TITULO: &str = "Regras do dono para delegar";
pub(crate) const AVISO: &str =
    "As regras orientam a escolha. Elas não liberam modelo fora da lista aprovada nem permissão acima da sua.";

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_delegation_versions(
           space_id TEXT PRIMARY KEY, version INTEGER NOT NULL CHECK(version>=0));
         CREATE TABLE IF NOT EXISTS donna_delegation_rules(
           space_id TEXT NOT NULL, team_id TEXT NOT NULL, body TEXT NOT NULL CHECK(length(body) BETWEEN 1 AND 4000),
           PRIMARY KEY(space_id,team_id));
         CREATE TABLE IF NOT EXISTS donna_delegation_receipts(
           space_id TEXT NOT NULL, client_request_id TEXT NOT NULL, digest TEXT NOT NULL,
           version INTEGER NOT NULL, PRIMARY KEY(space_id,client_request_id));",
    )?;
    Ok(())
}

/// Aparado nas pontas, quebras de linha `\n`; outros caracteres de controle ou invisíveis (a mesma
/// regra da função do bot) são recusados. Até 4.000 caracteres.
pub(crate) fn limpa(texto: &str) -> Result<String> {
    let texto = texto.replace("\r\n", "\n");
    let texto = texto.trim();
    if texto.chars().any(|c| c != '\n' && crate::fork::funcao::invisivel(c)) {
        bail!("delegation_invalid: o texto tem um caractere invisível ou de controle. Apague-o e tente de novo.");
    }
    if texto.chars().count() > LIMITE {
        bail!("delegation_limit: as regras podem ter até {LIMITE} caracteres.");
    }
    Ok(texto.to_owned())
}

pub(crate) fn versao(c: &Connection, space: &str) -> Result<i64> {
    Ok(c.query_row("SELECT version FROM donna_delegation_versions WHERE space_id=?1", [space], |r| r.get(0))
        .optional()?
        .unwrap_or(0))
}

/// `""` é a regra geral. Áreas apagadas do quadro são ignoradas na leitura (a linha fica até a
/// próxima gravação daquela área, que não existe mais).
pub(crate) fn regra(c: &Connection, space: &str, team: &str) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT body FROM donna_delegation_rules WHERE space_id=?1 AND team_id=?2",
        rusqlite::params![space, team],
        |r| r.get(0),
    )
    .optional()?)
}

/// Bots sem identidade alheia e fora de Canal recebem as regras do próprio Space.
fn recebe(cfg: &BotConfig) -> bool {
    !cfg.space_id.is_empty() && !cfg.is_group() && !cfg.context.as_ref().is_some_and(|c| c.foreign_identity)
}

/// O bloco das instruções e a versão em vigor; `None` sem regra aplicável.
pub(crate) fn bloco_na(c: &Connection, cfg: &BotConfig) -> Result<Option<(String, i64)>> {
    if !recebe(cfg) {
        return Ok(None);
    }
    crate::fork::spaces::require_space(c, &cfg.space_id)?;
    let space = &cfg.space_id;
    let mut partes = Vec::new();
    for (area, nome) in crate::fork::times::listagem::areas_do_bot(c, space, &cfg.id)? {
        if let Some(texto) = regra(c, space, &area)? {
            partes.push(format!("Regras da equipe {}:\n{}", escapar(&nome), escapar(&texto)));
        }
    }
    if let Some(texto) = regra(c, space, "")? {
        partes.push(format!("Regras gerais:\n{}", escapar(&texto)));
    }
    if partes.is_empty() {
        return Ok(None);
    }
    let versao = versao(c, space)?;
    Ok(Some((
        format!(
            "<{TAG}>\n{TITULO} (v{versao}). Primeiro as da sua equipe, da mais específica para a mais geral, depois as gerais; se divergirem, vale a mais específica.\n\n{}\n\n{AVISO}\n</{TAG}>",
            partes.join("\n\n")
        ),
        versao,
    )))
}

const TAG: &str = "regras_de_delegacao";

/// O texto do dono (e o nome da área) nunca abre nem fecha marcação: o bloco, os rótulos e o aviso são só do Hub.
fn escapar(texto: &str) -> String {
    texto.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Prefixo do prompt de cada turno (ator); vazio sem regras ou em turno de Canal. A mudança vale no turno seguinte.
pub(crate) fn turno(store: &Store, cfg: &BotConfig, canal: bool) -> String {
    if canal {
        return String::new();
    }
    bloco(store, cfg).map(|(bloco, _)| format!("{bloco}\n\n")).unwrap_or_default()
}

/// Falha de leitura não quebra o turno: as regras só orientam, e nada é injetado.
pub(crate) fn bloco(store: &Store, cfg: &BotConfig) -> Option<(String, i64)> {
    bloco_na(&store.db.locked(), cfg).unwrap_or_else(|error| {
        tracing::warn!(%error, "regras de delegação indisponíveis neste turno");
        None
    })
}

/// Recibo do turno (#401): o aviso do remetente de `ask_bot`/`message_bot` guarda a versão das regras em
/// vigor para ele, que o app mostra como "Regras de delegação v N". Sem regras, nada é gravado.
/// Pedido feito num turno de Canal não marca: esse turno não recebeu as regras.
pub(crate) fn marcar_aviso(
    store: &Store,
    source: &BotConfig,
    lane: &crate::store::Lane,
    aviso: &mut serde_json::Value,
) {
    let canal = lane.chat != source.id && store.bot(&lane.chat).ok().flatten().is_some_and(|b| b.config.is_group());
    if canal {
        return;
    }
    if let Some((_, versao)) = bloco(store, source) {
        aviso["delegationRulesVersion"] = versao.into();
    }
}
