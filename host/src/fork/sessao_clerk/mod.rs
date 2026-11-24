//! Sessão Clerk no acesso remoto de um Hub com vínculo: token de sessão v2 verificado contra a
//! JWKS fixada, prova de posse da chave do aparelho em cada pedido (amarrada ao handshake do
//! canal), `userId` do aparelho igual ao `sub` e papel tirado só do token. O canal fecha no `exp`
//! (ou no fim da validade da JWKS), a menos que o aparelho reautentique antes.
//!
//! O papel mais recente de cada `sub` fica no banco (`fork_sessao_papel`), com o `iat` do token
//! que o trouxe: sobrevive a reinício. Uma troca de papel sobe o `workspaceEpoch` no mesmo commit,
//! e os canais e pedidos admitidos na geração anterior são encerrados por ele.
//!
//! Cada pedido admitido leva uma [`Guarda`] imutável na autoridade original do transporte. Ela é
//! conferida na própria transação de cada efeito e antes de cada saída (`Autoridade::conferir`, e
//! [`conferir_no_efeito`] nas gravações que não passam por ela): prazo da admissão, epoch gravado,
//! papel registrado, vínculo e `userId` ligado à concessão atual do aparelho. Uma renovação da
//! sessão não prolonga pedido já admitido.
//!
//! Hub sem vínculo (`personal:unbound`): nada muda para os aparelhos pareados. Negar o remoto sem
//! vínculo é um interruptor desligado ([`NEGAR_REMOTO_SEM_VINCULO`]).
//!
//! Identidade nunca vem do corpo do pedido: `userId`, `orgId` e `role` no corpo são ignorados.
pub(crate) mod jwks;
pub(crate) mod matriz;
pub(crate) mod posse;
#[cfg(test)]
pub(crate) mod prova;
pub(crate) mod token;
pub(crate) mod vinculo;

use crate::LockExt;
use crate::api::devices::{Caller, RejectCode};
use crate::hub::Hub;
use crate::store::{Device, now_ms};
use anyhow::{Context as _, Result, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use token::Afirmacoes;
use tokio::sync::watch;
use vinculo::{Papel, Vinculo};

/// Pedido do canal que apresenta (ou renova) o token de sessão.
pub(crate) const METODO: &str = "sessionAuthenticate";
/// Campo do corpo com o token, em `sessionAuthenticate` e no `pair` de um Hub com vínculo.
pub(crate) const CAMPO_TOKEN: &str = "sessionToken";
/// Interruptor de produto: negar acesso remoto num Hub sem vínculo. Desligado; liga só por decisão
/// futura (a Fase 1 não pode depender da nuvem).
pub(crate) const NEGAR_REMOTO_SEM_VINCULO: bool = false;
/// Interruptor de produto: vínculo `team`. Desligado até existir conversa por membro; com ele
/// desligado, um vínculo `team` nega todo o acesso remoto e não pode ser gravado. O pessoal segue.
pub(crate) const VINCULO_TIME_LIBERADO: bool = false;
/// Num Hub com vínculo, o canal sem sessão fecha depois disto.
pub(crate) const ESPERA_SESSAO_MS: i64 = 30_000;

/// JWKS da sessão, em RAM.
#[derive(Default)]
pub(crate) struct Estado {
    pub(crate) jwks: jwks::Jwks,
    /// Provas: o caminho `team` com o interruptor ligado, só neste Hub.
    #[cfg(test)]
    time_para_prova: std::sync::atomic::AtomicBool,
}

impl Estado {
    #[cfg_attr(not(test), expect(clippy::unused_self, reason = "o estado só muda o interruptor nas provas"))]
    pub(crate) fn time_liberado(&self) -> bool {
        #[cfg(test)]
        if self.time_para_prova.load(std::sync::atomic::Ordering::SeqCst) {
            return true;
        }
        VINCULO_TIME_LIBERADO
    }

    #[cfg(test)]
    pub(crate) fn liberar_time_para_prova(&self) {
        self.time_para_prova.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Erros estáveis (contrato, seção 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Recusa {
    AuthInvalid,
    DeviceUnpaired,
    WorkspaceForbidden,
    RoleInsufficient,
}

impl Recusa {
    pub(crate) fn status(self) -> u16 {
        match self {
            Self::AuthInvalid | Self::DeviceUnpaired => 401,
            Self::WorkspaceForbidden | Self::RoleInsufficient => 403,
        }
    }

    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::AuthInvalid => "auth_invalid",
            Self::DeviceUnpaired => "device_unpaired",
            Self::WorkspaceForbidden => "workspace_forbidden",
            Self::RoleInsufficient => "role_insufficient",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::AuthInvalid => "Sign in again to use this computer.",
            Self::DeviceUnpaired => "This device isn't paired for this account. Pair it again.",
            Self::WorkspaceForbidden => "This account has no access to this workspace.",
            Self::RoleInsufficient => "Your role in this workspace doesn't allow this.",
        }
    }
}

/// O que o acesso remoto exige agora.
pub(crate) enum Exigencia {
    /// Sem vínculo: o comportamento de sempre.
    Livre,
    Sessao(Vinculo),
    Negar(Recusa),
}

pub(crate) fn exigencia(hub: &Hub) -> Exigencia {
    exigencia_com(vinculo::ler(hub), NEGAR_REMOTO_SEM_VINCULO, hub.sessao_clerk.time_liberado())
}

fn exigencia_com(estado: vinculo::Estado, negar_sem_vinculo: bool, time_liberado: bool) -> Exigencia {
    match estado {
        vinculo::Estado::SemVinculo if negar_sem_vinculo => Exigencia::Negar(Recusa::WorkspaceForbidden),
        vinculo::Estado::SemVinculo => Exigencia::Livre,
        vinculo::Estado::Vinculado(v) if v.kind() == "team" && !time_liberado => {
            Exigencia::Negar(Recusa::WorkspaceForbidden)
        }
        vinculo::Estado::Vinculado(v) => Exigencia::Sessao(v),
        vinculo::Estado::Ilegivel => Exigencia::Negar(Recusa::AuthInvalid),
    }
}

/// Vínculo ainda o mesmo workspace e permitido (o `team` só com o interruptor ligado).
fn vinculo_igual(hub: &Hub, db: &Connection, workspace_id: &str) -> bool {
    matches!(vinculo::ler_on(db), vinculo::Estado::Vinculado(v)
        if v.workspace_id() == workspace_id && (v.kind() != "team" || hub.sessao_clerk.time_liberado()))
}

/// Até quando uma sessão vale: o `exp` civil e o mesmo prazo no relógio monotônico (o menor entre
/// o `exp` convertido na verificação e a validade da JWKS). Vence no primeiro dos dois, então um
/// recuo do relógio civil não a estende. O monotônico é o do runtime, o mesmo em que a barreira do
/// canal dorme até o fim da sessão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limite {
    exp_ms: i64,
    ate: tokio::time::Instant,
}

impl Limite {
    /// Daqui a `ms` nos dois relógios.
    pub(crate) fn em(ms: i64) -> Self {
        let resta = Duration::from_millis(u64::try_from(ms).unwrap_or(0));
        Self { exp_ms: now_ms().saturating_add(ms), ate: tokio::time::Instant::now() + resta }
    }

    pub(crate) fn vencido(&self) -> bool {
        now_ms() >= self.exp_ms || tokio::time::Instant::now() >= self.ate
    }

    pub(crate) fn resta(&self) -> Duration {
        let civil = Duration::from_millis(u64::try_from(self.exp_ms.saturating_sub(now_ms())).unwrap_or(0));
        civil.min(self.ate.saturating_duration_since(tokio::time::Instant::now()))
    }

    /// Para o cliente (`expiresAt`, ms).
    pub(crate) fn expira_em_ms(&self) -> i64 {
        now_ms().saturating_add(i64::try_from(self.resta().as_millis()).unwrap_or(i64::MAX))
    }
}

/// Sessão verificada de um canal.
#[derive(Clone, Debug)]
pub(crate) struct Sessao {
    sub: String,
    papel: Papel,
    pub(crate) limite: Limite,
}

impl Sessao {
    pub(crate) fn sub(&self) -> &str {
        &self.sub
    }

    pub(crate) fn papel(&self) -> Papel {
        self.papel
    }
}

/// Token verificado. O papel pode ser a recusa (`workspace_forbidden`) de um `sub` verificado.
struct Verificado {
    afirmacoes: Afirmacoes,
    papel: Result<Papel, Recusa>,
    limite: Limite,
}

/// Token e prazo, sem registrar nada. O relógio é lido depois da busca da JWKS. Passada metade da
/// validade da JWKS, a renovação corre numa tarefa à parte.
async fn verificar(hub: &Arc<Hub>, v: &Vinculo, b: &Value) -> Result<Verificado, Recusa> {
    let token = b[CAMPO_TOKEN].as_str().ok_or(Recusa::AuthInvalid)?;
    let partes = token::separar(token)?;
    let jwks::Resolvida { chave, ate: jwks_ate, renovar } =
        hub.sessao_clerk.jwks.chave(&v.jwks, &partes.kid, Instant::now()).await?;
    if renovar {
        let (hub, url) = (hub.clone(), v.jwks.clone());
        tokio::spawn(async move { hub.sessao_clerk.jwks.renovar(&url, Instant::now()).await });
    }
    let agora = now_ms();
    let afirmacoes = token::conferir(&partes, &chave, v, agora.div_euclid(1000))?;
    let exp_ms = afirmacoes.exp.saturating_mul(1000);
    let resta = Duration::from_millis(u64::try_from(exp_ms.saturating_sub(agora)).unwrap_or(0));
    let ate = tokio::time::Instant::from_std(jwks_ate).min(tokio::time::Instant::now() + resta);
    let limite = Limite { exp_ms, ate };
    if limite.vencido() {
        return Err(Recusa::AuthInvalid);
    }
    Ok(Verificado { papel: v.papel(&afirmacoes), afirmacoes, limite })
}

/// Prova de posse ligada ao canal: o `th` do handshake e os nonces já usados nele.
pub(crate) struct Posse<'a> {
    pub(crate) th: &'a [u8; 32],
    pub(crate) nonces: &'a mut posse::Nonces,
}

impl Posse<'_> {
    fn conferir(&mut self, hub: &Hub, dk: &str, metodo: &str, pop: &Value) -> Result<(), Recusa> {
        self.nonces.conferir(&hub.host_id, self.th, dk, metodo, pop, now_ms())
    }
}

/// `sessionAuthenticate`: prova de posse e token; depois, numa Tx só, reconfere o prazo, o vínculo,
/// o epoch do canal, a concessão original do aparelho e `sub` igual ao `userId` dele, e só então
/// registra o papel do token (com troca, o epoch sobe no mesmo commit). Renovar não ressuscita uma
/// sessão já vencida nem troca o `sub` ou o papel da sessão aberta: recusa sem mudar a sessão.
pub(crate) async fn autenticar(
    hub: &Arc<Hub>,
    v: &Vinculo,
    canal: &Canal<'_>,
    mut posse: Posse<'_>,
    atual: Option<&Sessao>,
    pedido: &Value,
) -> Result<Sessao, Recusa> {
    posse.conferir(hub, canal.chave, METODO, &pedido["pop"])?;
    let Verificado { afirmacoes, papel, limite } = verificar(hub, v, &pedido["b"]).await?;
    registrar_na_prova(hub, canal, &v.workspace_id(), &afirmacoes, papel.ok(), limite)?;
    let papel = papel?;
    if let Some(atual) = atual
        && (atual.limite.vencido() || atual.sub != afirmacoes.sub || atual.papel != papel)
    {
        return Err(Recusa::AuthInvalid);
    }
    Ok(Sessao { sub: afirmacoes.sub, papel, limite })
}

/// O canal que apresenta a sessão: a chave do aparelho, a autoridade original do transporte e o
/// `workspaceEpoch` de quando abriu.
pub(crate) struct Canal<'a> {
    pub(crate) chave: &'a str,
    pub(crate) original: &'a crate::fork::autoridade::Autoridade,
    pub(crate) epoch: u64,
}

/// A Tx da prova: reconfere tudo o que a admitiu e grava o papel no mesmo commit.
fn registrar_na_prova(
    hub: &Hub,
    canal: &Canal<'_>,
    workspace_id: &str,
    afirmacoes: &Afirmacoes,
    papel: Option<Papel>,
    limite: Limite,
) -> Result<(), Recusa> {
    let feito = hub.cloud.with_transport_owner(|dono, geracao_dono| {
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sincronizacao = hub.cloud_synced.congelar();
        let sincronizado = hub.cloud_synced.load(std::sync::atomic::Ordering::Acquire);
        if canal.original.conferir(&tx, dono, geracao_dono, sincronizacao.epoca(), sincronizado).is_err()
            || usuario_do_aparelho_on(&tx, canal.chave)?.as_deref() != Some(afirmacoes.sub.as_str())
        {
            return Ok(Err(Recusa::DeviceUnpaired));
        }
        if limite.vencido()
            || !vinculo_igual(hub, &tx, workspace_id)
            || crate::fork::workspace::epoch_on(&tx)? != canal.epoch
        {
            return Ok(Err(Recusa::AuthInvalid));
        }
        let mudou = match registrar_papel_on(&tx, &afirmacoes.sub, afirmacoes.iat, papel) {
            Ok(mudou) => mudou,
            Err(recusa) => return Ok(Err(recusa)),
        };
        confirmar_papel(hub, tx, mudou, papel)?;
        Ok(Ok(()))
    });
    feito.unwrap_or_else(|erro| {
        tracing::warn!(error = %format!("{erro:#}"), "sessão: registro do papel na Tx da prova falhou");
        Err(Recusa::AuthInvalid)
    })
}

/// O que o `pair` verificou, para a Tx da gravação conferir de novo.
pub(crate) struct Pareamento {
    sub: String,
    iat: i64,
    papel: Papel,
    limite: Limite,
    workspace_id: String,
}

/// `pair` num Hub com vínculo: prova de posse e token de quem pareia. Nada é registrado aqui.
pub(crate) async fn autenticar_pareamento(
    hub: &Arc<Hub>,
    v: &Vinculo,
    dk: &str,
    mut posse: Posse<'_>,
    pedido: &Value,
) -> Result<Pareamento, Recusa> {
    posse.conferir(hub, dk, "pair", &pedido["pop"])?;
    let Verificado { afirmacoes, papel, limite } = verificar(hub, v, &pedido["b"]).await?;
    Ok(Pareamento { sub: afirmacoes.sub, iat: afirmacoes.iat, papel: papel?, limite, workspace_id: v.workspace_id() })
}

/// Todo pedido remoto num Hub com vínculo, menos `sessionAuthenticate`: sessão viva, prova de
/// posse do pedido, `userId` do aparelho igual ao `sub`, papel ainda o registrado e método
/// permitido para ele na [`matriz`].
pub(crate) fn conferir_pedido(
    hub: &Hub,
    dk: &str,
    mut posse: Posse<'_>,
    sessao: Option<&Sessao>,
    metodo: &str,
    pop: &Value,
) -> Result<(), Recusa> {
    let sessao = sessao.filter(|s| !s.limite.vencido()).ok_or(Recusa::AuthInvalid)?;
    posse.conferir(hub, dk, metodo, pop)?;
    if usuario_do_aparelho(hub, dk).as_deref() != Some(sessao.sub()) {
        return Err(Recusa::DeviceUnpaired);
    }
    let registrado = papel_on(&hub.store.db.locked(), &sessao.sub).ok().flatten().and_then(|(_, p)| p);
    if registrado != Some(sessao.papel) {
        return Err(Recusa::AuthInvalid);
    }
    if !matriz::permitido(sessao.papel, metodo) {
        return Err(Recusa::RoleInsufficient);
    }
    Ok(())
}

/// Até quando vale a guarda de um pedido.
#[derive(Debug)]
pub(crate) enum Prazo {
    /// Pedido: o prazo da sessão na admissão; renovar depois não o estende.
    Fixo(Limite),
    /// Assinatura: o prazo atual do canal (a renovação não troca `sub` nem papel).
    Canal(watch::Receiver<Option<Limite>>),
}

/// Guarda imutável da admissão de um pedido num Hub com vínculo, presa à geração que o admitiu: o
/// `workspaceEpoch` gravado, que sobe a cada troca de papel.
#[derive(Debug)]
pub(crate) struct Guarda {
    hub: Weak<Hub>,
    chave: String,
    sub: String,
    papel: Papel,
    epoch: u64,
    workspace_id: String,
    prazo: Prazo,
}

impl Guarda {
    pub(crate) fn nova(hub: &Arc<Hub>, chave: &str, sessao: &Sessao, v: &Vinculo, epoch: u64, prazo: Prazo) -> Self {
        Self {
            hub: Arc::downgrade(hub),
            chave: chave.to_owned(),
            sub: sessao.sub.clone(),
            papel: sessao.papel,
            epoch,
            workspace_id: v.workspace_id(),
            prazo,
        }
    }

    pub(crate) fn sub(&self) -> &str {
        &self.sub
    }

    pub(crate) fn papel(&self) -> Papel {
        self.papel
    }

    /// Só a `Connection` já protegida (a Tx do efeito); nenhum lock do banco.
    pub(crate) fn conferir(&self, db: &Connection) -> Result<()> {
        let hub = self.hub.upgrade().context("not_allowed: sessão encerrada")?;
        let viva = match &self.prazo {
            Prazo::Fixo(limite) => !limite.vencido(),
            Prazo::Canal(limite) => limite.borrow().as_ref().is_some_and(|l| !l.vencido()),
        };
        ensure!(viva, "not_allowed: sessão vencida");
        ensure!(crate::fork::workspace::epoch_on(db)? == self.epoch, "not_allowed: workspace mudou");
        ensure!(papel_on(db, &self.sub)?.and_then(|(_, p)| p) == Some(self.papel), "not_allowed: papel mudou");
        ensure!(vinculo_igual(&hub, db, &self.workspace_id), "not_allowed: vínculo mudou");
        ensure!(
            usuario_do_aparelho_on(db, &self.chave)?.as_deref() == Some(self.sub.as_str()),
            "not_allowed: aparelho desligado do usuário"
        );
        Ok(())
    }
}

/// Gravação que abre a Tx direto no banco, sem `Autoridade::with_db`: confere, na própria Tx, a
/// guarda da sessão do pedido em curso, se houver.
pub(crate) fn conferir_no_efeito(db: &Connection) -> Result<()> {
    if let Some(crate::fork::donna_fixa::pedido::Origem::Dono(_, original)) =
        crate::fork::donna_fixa::pedido::origem_efetiva()
        && let Some(guarda) = original.sessao()
    {
        guarda.conferir(db)?;
    }
    Ok(())
}

/// Erro de pedido no canal: o formato atual, com o código estável em `code`.
pub(crate) fn erro_canal(id: u64, recusa: Recusa) -> Value {
    tracing::warn!(reason = recusa.code(), origin = "aparelho", "remote session refused");
    json!({"id": id, "err": {"status": recusa.status(), "message": recusa.message(), "code": recusa.code()}})
}

/// `reject` terminal da sessão vencida (ou da JWKS vencida): `code` legado e o código estável.
pub(crate) fn reject_sessao() -> Value {
    json!({
        "t": "reject",
        "code": RejectCode::Unauthorized,
        "message": Recusa::AuthInvalid.message(),
        "stableCode": Recusa::AuthInvalid.code(),
    })
}

/// Concessão que o canal não corta no fim da sessão (mídia de tela WebRTC, credencial de voz do
/// fornecedor): num Hub com vínculo, negada a aparelhos até haver corte próprio no prazo da
/// sessão. Hub sem vínculo e o próprio computador seguem iguais.
/// Toda recusa desta porta fica registrada com o método e o motivo (vínculo presente ou recusado).
pub(crate) fn negar_sem_corte(hub: &Hub, caller: &Caller, metodo: &str) -> Result<()> {
    if !matches!(caller, Caller::Device { .. }) {
        return Ok(());
    }
    let motivo = match exigencia(hub) {
        Exigencia::Livre => return Ok(()),
        Exigencia::Sessao(_) => "no_session_cut",
        Exigencia::Negar(recusa) => recusa.code(),
    };
    tracing::warn!(reason = motivo, origin = "aparelho", method = metodo, "remote request refused");
    Err(crate::api::devices::Forbidden.into())
}

/// Método ainda sem cerca ([`matriz::sem_cerca`]): num Hub com vínculo, negado a aparelhos também
/// no despacho. Hub sem vínculo e o próprio computador seguem iguais.
pub(crate) fn negar_sem_cerca(hub: &Hub, caller: &Caller, metodo: &str) -> Result<()> {
    if matriz::sem_cerca(metodo) {
        negar_sem_corte(hub, caller, metodo)?;
    }
    Ok(())
}

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS fork_sessao_aparelho_usuario(
            key TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            geracao INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS fork_sessao_papel(
            sub TEXT PRIMARY KEY,
            iat INTEGER NOT NULL,
            papel TEXT);",
    )?;
    Ok(())
}

/// O papel mais recente registrado de `sub`: (`iat`, papel; `None`: sem acesso ao workspace).
fn papel_on(db: &Connection, sub: &str) -> Result<Option<(i64, Option<Papel>)>> {
    let lido: Option<(i64, Option<String>)> = db
        .query_row("SELECT iat, papel FROM fork_sessao_papel WHERE sub=?1", [sub], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((iat, papel)) = lido else { return Ok(None) };
    let papel = papel.map(|p| Papel::de_nome(&p).context("integridade: papel ilegível")).transpose()?;
    Ok(Some((iat, papel)))
}

/// Registra, na Tx do chamador, o papel de um token admitido. `true`: o papel mudou e o epoch tem
/// de subir no mesmo commit. Um `iat` igual ou anterior ao registrado, com outro papel, é
/// recusado: sem prova de ordem, o papel não muda.
fn registrar_papel_on(c: &Connection, sub: &str, iat: i64, papel: Option<Papel>) -> Result<bool, Recusa> {
    let nome = papel.map(Papel::nome);
    let mudou = match papel_on(c, sub).map_err(|_| Recusa::AuthInvalid)? {
        None => {
            c.execute("INSERT INTO fork_sessao_papel(sub, iat, papel) VALUES(?1, ?2, ?3)", params![sub, iat, nome])
                .map_err(|_| Recusa::AuthInvalid)?;
            false
        }
        Some((visto, anterior)) if anterior == papel => {
            c.execute("UPDATE fork_sessao_papel SET iat = ?2 WHERE sub = ?1", params![sub, iat.max(visto)])
                .map_err(|_| Recusa::AuthInvalid)?;
            false
        }
        Some((visto, _)) if iat <= visto => return Err(Recusa::AuthInvalid),
        Some(_) => {
            c.execute("UPDATE fork_sessao_papel SET iat = ?2, papel = ?3 WHERE sub = ?1", params![sub, iat, nome])
                .map_err(|_| Recusa::AuthInvalid)?;
            true
        }
    };
    Ok(mudou)
}

/// Commit da Tx que registrou o papel: com troca, o epoch sobe e é publicado no mesmo commit.
fn confirmar_papel(hub: &Hub, tx: rusqlite::Transaction<'_>, mudou: bool, papel: Option<Papel>) -> Result<()> {
    if !mudou {
        tx.commit()?;
        return Ok(());
    }
    let motivo = match papel {
        Some(_) => crate::fork::workspace::Motivo::RoleChanged,
        None => crate::fork::workspace::Motivo::AcessoRetirado,
    };
    crate::fork::workspace::subir_na_tx(hub, tx, motivo)?;
    Ok(())
}

/// Provas: registra um papel direto, sem canal.
#[cfg(test)]
fn registrar_papel(hub: &Hub, sub: &str, iat: i64, papel: Option<Papel>) -> Result<(), Recusa> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|_| Recusa::AuthInvalid)?;
    let mudou = registrar_papel_on(&tx, sub, iat, papel)?;
    confirmar_papel(hub, tx, mudou, papel).map_err(|_| Recusa::AuthInvalid)
}

/// O `userId` que pareou a chave: só aparelho Local pareado por código num Hub com vínculo, ainda
/// na geração de concessão gravada no pareamento (qualquer mudança do aparelho a encerra).
fn usuario_do_aparelho_on(db: &Connection, dk: &str) -> Result<Option<String>> {
    let Some(geracao) = crate::fork::autoridade::Autoridade::geracao_atual(db, dk) else { return Ok(None) };
    Ok(db
        .query_row(
            "SELECT l.user_id FROM fork_sessao_aparelho_usuario l JOIN devices d ON d.key = l.key
              WHERE l.key = ?1 AND d.source = 'local' AND l.geracao = ?2",
            params![dk, geracao],
            |r| r.get(0),
        )
        .optional()?)
}

/// Erro de leitura conta como sem `userId`.
fn usuario_do_aparelho(hub: &Hub, dk: &str) -> Option<String> {
    usuario_do_aparelho_on(&hub.store.db.locked(), dk).ok().flatten()
}

/// Desfaz a ligação da chave com o `userId` (revogação, repareamento sem vínculo).
pub(crate) fn desligar_on(c: &Connection, dk: &str) -> Result<()> {
    c.execute("DELETE FROM fork_sessao_aparelho_usuario WHERE key=?1", [dk])?;
    Ok(())
}

/// Pareamento por código num Hub com vínculo. Na mesma Tx: relê o vínculo e o prazo do token,
/// registra o papel (com troca, o epoch sobe no mesmo commit), libera o bloqueio, grava o aparelho
/// e liga o `userId` à geração nova da concessão. O erro de fora é do banco; o de dentro, a recusa.
pub(crate) fn readmitir_vinculado(hub: &Hub, device: &Device, par: &Pareamento) -> Result<Result<(), Recusa>> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !vinculo_igual(hub, &tx, &par.workspace_id) || par.limite.vencido() {
        return Ok(Err(Recusa::AuthInvalid));
    }
    let mudou = match registrar_papel_on(&tx, &par.sub, par.iat, Some(par.papel)) {
        Ok(mudou) => mudou,
        Err(recusa) => return Ok(Err(recusa)),
    };
    crate::fork::workspace::liberar_pareamento_on(&tx, &device.key)?;
    crate::store::put_device_on(&tx, device)?;
    let geracao = crate::fork::autoridade::Autoridade::geracao_atual(&tx, &device.key)
        .context("integridade: geração do aparelho ausente")?;
    tx.execute(
        "INSERT INTO fork_sessao_aparelho_usuario(key, user_id, geracao) VALUES(?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET user_id = excluded.user_id, geracao = excluded.geracao",
        params![device.key, par.sub, geracao],
    )?;
    confirmar_papel(hub, tx, mudou, Some(par.papel))?;
    Ok(Ok(()))
}

#[cfg(test)]
pub(crate) mod tests;
