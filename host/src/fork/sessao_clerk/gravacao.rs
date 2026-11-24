//! Gravação do vínculo pessoal. Só pelo subcomando `workspace vincular` do binário do Hub, rodado
//! pelo usuário dono da pasta de dados e com o Hub parado (`host.lock`). Nenhum RPC grava o vínculo:
//! bearer, Web local, renderer e aparelhos não chegam aqui. O app chama o subcomando só pelo
//! processo principal. Fica atrás de [`super::GRAVACAO_VINCULO_LIBERADA`], desligado.
//!
//! A âncora de confiança (emissor, JWKS e `azp`) vem da build de cada ambiente
//! (`MEXERICA_CLERK_ISSUER`, `MEXERICA_CLERK_JWKS_URL` e `MEXERICA_CLERK_AZP`, separados por vírgula),
//! nunca do pedido. A edição Open e uma build sem os três não gravam. Trocar a âncora exige uma build
//! nova e regravar o vínculo. O pedido traz só o `sessionToken` do dono (pela entrada padrão, nunca
//! por argumento) e o compare-and-swap. O dono é o `sub` do token verificado contra a âncora,
//! emitido há no máximo 60 s. Com terminal, a pessoa confirma o workspace antes da gravação; sem
//! terminal, a confirmação é a do processo principal do app (`--confirmado`).
//!
//! Na mesma Tx: confere o compare-and-swap sobre a versão do vínculo inteiro e o epoch, grava o
//! vínculo, apaga as ligações `userId` dos aparelhos e os papéis registrados e sobe o
//! `workspaceEpoch`. A troca de dono é recusada. O token nunca vai a log, saída nem erro.
use super::{Recusa, jwks::Jwks, token, vinculo};
use crate::LockExt;
use crate::store::{Store, now_ms};
use anyhow::Result;
use rusqlite::TransactionBehavior;
use serde_json::{Value, json};

/// Idade máxima do token que prova o dono (segundos desde o `iat`).
const RECENTE_S: i64 = 60;
/// Valor de `--esperado-versao` quando não há vínculo gravado.
const NENHUM: &str = "nenhum";

/// Recusa com código estável; só o código sai.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Falha(pub(crate) &'static str);

impl std::fmt::Display for Falha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Falha {}

fn recusa(r: Recusa) -> anyhow::Error {
    Falha(r.code()).into()
}

/// Âncora de confiança: o emissor Clerk, a JWKS dele e os `azp` aceitos.
pub(crate) struct Ancora {
    pub(crate) emissor: String,
    pub(crate) jwks: String,
    pub(crate) azp: Vec<String>,
}

impl Ancora {
    /// A da build. Nenhuma na edição Open nem sem os três valores.
    pub(crate) fn da_build() -> Option<Self> {
        if crate::environment::Environment::is_open() {
            return None;
        }
        let (Some(emissor), Some(jwks), Some(azp)) = (
            option_env!("MEXERICA_CLERK_ISSUER"),
            option_env!("MEXERICA_CLERK_JWKS_URL"),
            option_env!("MEXERICA_CLERK_AZP"),
        ) else {
            return None;
        };
        let azp = azp.split(',').map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned).collect();
        Some(Self { emissor: emissor.to_owned(), jwks: jwks.to_owned(), azp })
    }

    /// O vínculo pessoal de `dono` com esta âncora, no formato gravado e validado como o lido.
    fn vinculo(&self, dono: &str) -> Option<(String, vinculo::Vinculo)> {
        let raw = json!({
            "schema": 1,
            "kind": "personal",
            "ownerUserId": dono,
            "issuer": self.emissor,
            "jwksUrl": self.jwks,
            "authorizedParties": self.azp,
        })
        .to_string();
        vinculo::interpretar(&raw).map(|v| (raw, v))
    }
}

/// O dono provado, pronto para a Tx: o registro a gravar e o prazo do token.
pub(crate) struct Provado {
    raw: String,
    vinculo: vinculo::Vinculo,
    exp: i64,
}

impl Provado {
    pub(crate) fn workspace_id(&self) -> String {
        self.vinculo.workspace_id()
    }
}

/// O que o chamador viu antes de pedir a gravação: a versão do vínculo (nenhuma sem vínculo) e o
/// epoch.
pub(crate) struct Esperado {
    pub(crate) versao: Option<String>,
    pub(crate) epoch: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Gravado {
    pub(crate) workspace_id: String,
    pub(crate) versao: String,
    pub(crate) epoch: u64,
}

/// Verifica o token do dono contra a âncora: JWKS da âncora por busca avulsa (o mesmo cliente
/// filtrado), assinatura, `iss`, `azp`, `sid`, prazos e `iat` recente.
pub(crate) async fn provar_dono(ancora: &Ancora, jwks: &Jwks, token: &str) -> Result<Provado> {
    let (_, provisorio) = ancora.vinculo("dono").ok_or(Falha("workspace_anchor_invalid"))?;
    let partes = token::separar(token).map_err(recusa)?;
    let chave = jwks.chave_avulsa(&provisorio.jwks, &partes.kid).await.map_err(recusa)?;
    let agora = now_ms().div_euclid(1000);
    let afirmacoes = token::conferir(&partes, &chave, &provisorio, agora).map_err(recusa)?;
    if agora.saturating_sub(afirmacoes.iat) > RECENTE_S {
        return Err(recusa(Recusa::AuthInvalid));
    }
    let (raw, vinculo) = ancora.vinculo(&afirmacoes.sub).ok_or_else(|| recusa(Recusa::AuthInvalid))?;
    vinculo::conferir_gravacao(&vinculo, super::VINCULO_TIME_LIBERADO).map_err(recusa)?;
    Ok(Provado { raw, vinculo, exp: afirmacoes.exp })
}

/// Grava o vínculo provado. Quem chama segura o `host.lock`.
pub(crate) fn gravar(store: &Store, provado: &Provado, esperado: &Esperado) -> Result<Gravado> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let atual = match vinculo::ler_on(&tx) {
        vinculo::Estado::SemVinculo => None,
        vinculo::Estado::Vinculado(atual) => Some(atual),
        vinculo::Estado::Ilegivel => return Err(Falha("workspace_bind_conflict").into()),
    };
    if atual.as_ref().map(|v| v.versao.as_str()) != esperado.versao.as_deref()
        || crate::fork::workspace::epoch_on(&tx)? != esperado.epoch
    {
        return Err(Falha("workspace_bind_conflict").into());
    }
    if atual.is_some_and(|v| v.workspace_id() != provado.workspace_id()) {
        return Err(Falha("workspace_swap_unsupported").into());
    }
    if now_ms().div_euclid(1000) >= provado.exp {
        return Err(recusa(Recusa::AuthInvalid));
    }
    tx.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        rusqlite::params![vinculo::CHAVE, provado.raw],
    )?;
    tx.execute("DELETE FROM fork_sessao_aparelho_usuario", [])?;
    tx.execute("DELETE FROM fork_sessao_papel", [])?;
    let epoch = crate::fork::workspace::bump_on(&tx)?;
    tx.commit()?;
    tracing::info!(kind = "personal", epoch, "workspace binding written");
    Ok(Gravado { workspace_id: provado.workspace_id(), versao: provado.vinculo.versao.clone(), epoch })
}

/// Estado para o compare-and-swap: workspace, versão e epoch atuais.
pub(crate) fn estado(store: &Store) -> Result<Value> {
    let db = store.db.locked();
    let epoch = crate::fork::workspace::epoch_on(&db)?;
    Ok(match vinculo::ler_on(&db) {
        vinculo::Estado::SemVinculo => json!({"state": "unbound", "workspaceVersion": NENHUM, "workspaceEpoch": epoch}),
        vinculo::Estado::Vinculado(v) => json!({"state": "bound", "workspaceId": v.workspace_id(),
            "workspaceVersion": v.versao, "workspaceEpoch": epoch}),
        vinculo::Estado::Ilegivel => json!({"state": "unreadable", "workspaceEpoch": epoch}),
    })
}

#[derive(clap::Subcommand)]
pub(crate) enum Acao {
    /// Mostra o vínculo gravado (workspace, versão e epoch) para o compare-and-swap.
    Estado,
    /// Grava o vínculo pessoal com a prova Clerk do dono, lida da entrada padrão. Exige o Hub parado.
    Vincular {
        /// Versão mostrada por `estado`, ou `nenhum` sem vínculo.
        #[arg(long)]
        esperado_versao: String,
        /// Epoch mostrado por `estado`.
        #[arg(long)]
        esperado_epoch: u64,
        /// Sem terminal: a confirmação humana já foi feita pelo processo principal do app.
        #[arg(long)]
        confirmado: bool,
    },
}

pub(crate) async fn executar(acao: Acao) -> Result<()> {
    use std::io::{BufRead as _, IsTerminal as _, Write as _};
    let Acao::Vincular { esperado_versao, esperado_epoch, confirmado } = acao else {
        let (store, _, _) = crate::open_store()?;
        println!("{}", estado(&store)?);
        return Ok(());
    };
    if !super::GRAVACAO_VINCULO_LIBERADA {
        return Err(Falha("workspace_bind_disabled").into());
    }
    let terminal = std::io::stdin().is_terminal();
    if !terminal && !confirmado {
        return Err(Falha("workspace_bind_unconfirmed").into());
    }
    let ancora = Ancora::da_build().ok_or(Falha("workspace_anchor_missing"))?;
    let esperado = Esperado { versao: (esperado_versao != NENHUM).then_some(esperado_versao), epoch: esperado_epoch };
    let _trava = tokio::task::spawn_blocking(crate::service::lock_host).await??;
    let (store, _, _) = crate::open_store()?;
    let mut token = String::new();
    if terminal {
        eprint!("sessionToken do dono: ");
        let _ = std::io::stderr().flush();
        #[cfg(unix)]
        let _eco = crate::SemEco::ligar()?;
        std::io::stdin().lock().read_line(&mut token)?;
        eprintln!();
    } else {
        std::io::stdin().lock().read_line(&mut token)?;
    }
    let provado = provar_dono(&ancora, &Jwks::default(), token.trim()).await?;
    drop(token);
    if terminal {
        eprint!("Vincular este Hub ao workspace {}? Digite VINCULAR para confirmar: ", provado.workspace_id());
        let _ = std::io::stderr().flush();
        let mut resposta = String::new();
        std::io::stdin().lock().read_line(&mut resposta)?;
        if resposta.trim() != "VINCULAR" {
            return Err(Falha("workspace_bind_unconfirmed").into());
        }
    }
    let g = gravar(&store, &provado, &esperado)?;
    println!("{}", json!({"workspaceId": g.workspace_id, "workspaceVersion": g.versao, "workspaceEpoch": g.epoch}));
    Ok(())
}
