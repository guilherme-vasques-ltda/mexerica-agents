//! Instruções do dono para como os bots trabalham: um campo «Para todos os meus bots» e outro «Neste Space», somados
//! às instruções de cada bot. Ler e gravar só pela RPC do dono neste computador (`call`); o bot recebe o texto apenas
//! no início de cada turno (`turno`), nunca em Canais. É orientação: não libera permissão, ferramenta, modelo nem conta.
use crate::{
    LockExt,
    api::devices::{Caller, Forbidden},
    hub::Hub,
    store::{BotConfig, Store},
};
use anyhow::{Result, anyhow, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

pub(crate) const METHODS: &[&str] = &["instrucaoGeral", "instrucaoGeralSalvar"];
/// Caracteres por campo: o bastante para «dez linhas afiadas», curto para não pesar em todo turno.
pub(crate) const LIMITE: usize = 2_000;
const TAG: &str = "instrucoes_do_dono";
const TODOS: &str = "";
pub(crate) const AVISO: &str = "As instruções orientam o jeito de trabalhar. Elas não liberam permissão, ferramenta, modelo nem conta, e não mudam as regras fixas do sistema.";

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_instrucao_geral(
        space_id TEXT PRIMARY KEY, texto TEXT NOT NULL, revisao INTEGER NOT NULL, atualizado_em INTEGER NOT NULL);",
    )?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Leitura {
    #[serde(default)]
    space_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Escopo {
    Geral,
    Space,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Gravacao {
    escopo: Escopo,
    #[serde(default)]
    space_id: Option<String>,
    texto: String,
    expected_revision: i64,
}

fn lido(c: &Connection, space: &str) -> Result<(String, i64)> {
    Ok(c.query_row("SELECT texto,revisao FROM donna_instrucao_geral WHERE space_id=?1", [space], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .optional()?
    .unwrap_or_default())
}

/// Aparado nas pontas, quebras de linha `\n`; outros caracteres de controle ou invisíveis são recusados.
fn limpa(texto: &str) -> Result<String> {
    let texto = texto.replace("\r\n", "\n");
    let texto = texto.trim();
    if texto.chars().any(|c| c != '\n' && crate::fork::funcao::invisivel(c)) {
        bail!("invalid_field: o texto tem um caractere invisível ou de controle. Apague-o e tente de novo.");
    }
    ensure!(texto.chars().count() <= LIMITE, "limit_exceeded: o campo pode ter até {LIMITE} caracteres.");
    Ok(texto.to_owned())
}

fn estado(c: &Connection, space: &str) -> Result<Value> {
    crate::fork::spaces::require_space(c, space)?;
    let (geral, revisao_geral) = lido(c, TODOS)?;
    let (texto, revisao) = lido(c, space)?;
    Ok(json!({
        "limite": LIMITE,
        "geral": {"texto": geral, "revision": revisao_geral},
        "space": {"spaceId": space, "texto": texto, "revision": revisao},
    }))
}

pub(crate) fn call(hub: &Hub, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    if !matches!(caller, Caller::Local) {
        return Err(Forbidden.into());
    }
    let invalido = |_| anyhow!("invalid_field: pedido de instruções inválido");
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    if method == "instrucaoGeral" {
        let leitura: Leitura = serde_json::from_value(body).map_err(invalido)?;
        return estado(&tx, leitura.space_id.as_deref().unwrap_or(crate::fork::spaces::PERSONAL));
    }
    let pedido: Gravacao = serde_json::from_value(body).map_err(invalido)?;
    let space = pedido.space_id.as_deref().unwrap_or(crate::fork::spaces::PERSONAL);
    let alvo = match pedido.escopo {
        Escopo::Geral => {
            ensure!(pedido.space_id.is_none(), "invalid_field: spaceId só vale para o escopo space");
            TODOS
        }
        Escopo::Space => {
            ensure!(pedido.space_id.is_some(), "invalid_field: spaceId obrigatório");
            space
        }
    };
    crate::fork::spaces::require_space(&tx, space)?;
    let texto = limpa(&pedido.texto)?;
    if lido(&tx, alvo)?.1 != pedido.expected_revision {
        return Err(crate::remote::cloud::Conflict("instrucao_geral_revision_conflict").into());
    }
    tx.execute(
        "INSERT INTO donna_instrucao_geral(space_id,texto,revisao,atualizado_em) VALUES(?1,?2,?3,?4)
         ON CONFLICT(space_id) DO UPDATE SET texto=excluded.texto, revisao=excluded.revisao, atualizado_em=excluded.atualizado_em",
        params![alvo, texto, crate::store::next_rev(&tx)?, crate::store::now_ms()],
    )?;
    let resultado = estado(&tx, space)?;
    tx.commit()?;
    Ok(resultado)
}

/// O texto do dono nunca abre nem fecha marcação: o bloco, os rótulos e o aviso são só do Hub.
fn escapar(texto: &str) -> String {
    texto.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// O bloco do turno; `None` sem texto em nenhum dos dois campos. O mais específico vem primeiro e vale em conflito.
pub(crate) fn bloco(c: &Connection, cfg: &BotConfig) -> Result<Option<String>> {
    let mut partes = Vec::new();
    for (rotulo, space) in [("Neste Space", cfg.space_id.as_str()), ("Para todos os meus bots", TODOS)] {
        let (texto, _) = lido(c, space)?;
        if !texto.is_empty() {
            partes.push(format!("{rotulo}:\n{}", escapar(&texto)));
        }
    }
    if partes.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "<{TAG}>\nInstruções do dono sobre como trabalhar. Valem nesta ordem, da mais específica para a mais geral: as instruções próprias deste bot (no seu perfil), depois «Neste Space», depois «Para todos os meus bots»; se divergirem, vale a mais específica.\n\n{}\n\n{AVISO}\n</{TAG}>",
        partes.join("\n\n")
    )))
}

/// Prefixo do prompt de cada turno (ator), lido do banco a cada turno: mudar um campo vale na mensagem seguinte, sem
/// reiniciar bot nem conversa. Vazio sem texto, em turno de Canal e para bot com identidade alheia.
pub(crate) fn turno(store: &Store, cfg: &BotConfig, canal: bool) -> String {
    if canal || !crate::fork::delegacao::recebe(cfg) {
        return String::new();
    }
    match bloco(&store.db.locked(), cfg) {
        Ok(bloco) => bloco.map(|b| format!("{b}\n\n")).unwrap_or_default(),
        Err(error) => {
            tracing::warn!(%error, "instruções do dono indisponíveis neste turno");
            String::new()
        }
    }
}
