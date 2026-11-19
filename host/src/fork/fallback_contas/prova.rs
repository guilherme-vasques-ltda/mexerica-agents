//! F6 fatia B, regra (k): prova antes de enviar contexto à conta reserva.
//!
//! Estar conectada, ser do mesmo motor e constar da lista não bastam. A prova (falha fechada) confere: (1) a conta
//! e a identidade que o próprio CLI confirma no perfil isolado dela; (2) que a conta pode receber os dados daquele
//! Space (consta da ordem do Space E um bot do Space a usa); (3) a autoridade vigente do dono sobre a conta e o bot
//! (registro com escopo do dono, conta ligada, bot vivo). Sem prova de qualquer item, nada é enviado. É refeita a
//! cada troca, de novo antes do envio e uma última vez **dentro da transação do permit** (`admissao::dispatch`):
//! revogar entre a prova e o envio cancela a troca e nada é enviado.
use crate::LockExt;
use crate::fork::engine_accounts::{AuthorizedScope, Registry};
use crate::hub::Hub;
use crate::store::BotConfig;
use rusqlite::Connection;
use std::sync::Arc;

/// Por que a prova falhou. Lista fechada: o texto ao dono sai daqui, nunca da mensagem de um erro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Falha {
    /// A conta saiu do cadastro, foi desconectada ou deixou de ser assinatura comprovada.
    Revogada,
    /// O CLI não confirmou a identidade da conta.
    SemIdentidade,
    /// A conta não consta mais da ordem do Space.
    SemAcessoAoSpace,
    /// O bot ou a posse do dono mudou.
    SemAutoridade,
}

impl Falha {
    pub(crate) fn codigo(self) -> &'static str {
        match self {
            Self::Revogada => "conta_revogada",
            Self::SemIdentidade => "sem_identidade",
            Self::SemAcessoAoSpace => "sem_acesso_ao_space",
            Self::SemAutoridade => "sem_autoridade",
        }
    }

    pub(crate) fn motivo(self) -> &'static str {
        match self {
            Self::Revogada => "a conta não está mais conectada como assinatura",
            Self::SemIdentidade => "o login da conta não foi confirmado",
            Self::SemAcessoAoSpace => "sem prova de acesso ao Space",
            Self::SemAutoridade => "sem prova de autoridade sobre a conta e o bot",
        }
    }
}

/// Itens (2) e (3), só leitura, numa `Connection` (a do permit, para valer no mesmo instante do despacho). A conta
/// precisa constar da ordem do Space E ter um vínculo de verdade com ele (um bot do Space a usa): constar da lista
/// sozinho não prova nada, senão uma conta de outro Space posta na ordem passaria.
pub(crate) fn conferir_vinculos_on(c: &Connection, cfg: &BotConfig, conta: &str) -> Result<(), Falha> {
    let registry = Registry::load_db(c).map_err(|_| Falha::SemAutoridade)?;
    let account = registry.account(conta).map_err(|_| Falha::Revogada)?;
    if !account.subscription_verified() || account.managed.as_ref().is_some_and(|m| !m.enabled) {
        return Err(Falha::Revogada);
    }
    if account.scope != AuthorizedScope::Owner
        || crate::store::Store::bot_on(c, &cfg.id).ok().flatten().is_none_or(|b| b.deleted)
    {
        return Err(Falha::SemAutoridade);
    }
    let listada = super::reserva_do_space(c, conta, &cfg.space_id).unwrap_or(false);
    let vinculada = crate::fork::engine_accounts::managed::linked_to_space(c, conta, &cfg.space_id).unwrap_or(false);
    if listada && vinculada { Ok(()) } else { Err(Falha::SemAcessoAoSpace) }
}

/// Os mesmos itens, lendo o banco do Hub.
pub(crate) fn conferir_vinculos(hub: &Hub, cfg: &BotConfig, conta: &str) -> Result<(), Falha> {
    conferir_vinculos_on(&hub.store.db.locked(), cfg, conta)
}

/// A prova inteira: os vínculos e, por último, o CLI confirmando a identidade. Cada chamada refaz tudo.
pub(crate) async fn provar(hub: &Arc<Hub>, cfg: &BotConfig, conta: &str) -> Result<(), Falha> {
    conferir_vinculos(hub, cfg, conta)?;
    let registry = Registry::load(&hub.store).map_err(|_| Falha::SemAutoridade)?;
    crate::fork::engine_setup::confirm_identity(&registry, conta).await.map_err(|_| Falha::SemIdentidade)?;
    // A identidade levou tempo: os vínculos podem ter sido revogados enquanto o CLI respondia.
    conferir_vinculos(hub, cfg, conta)
}
