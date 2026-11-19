//! Autoridade original de transporte. Não aprova efeitos nem prova presença humana.
//! Extraída do código próprio `Chão2B`; independente de política, editor e Skills.
//! Ordem única: Cloud (dono) → DB → sincronização. Callbacks só usam a Connection.
pub(crate) mod concessao;
pub(crate) mod epoca_dono;
pub(crate) mod personal_channel;
pub(crate) mod personal_link;
pub mod sincronizacao;

use crate::{
    LockExt,
    api::devices::{Caller, RejectCode},
    fork::engine_setup::access::Authority,
    hub::Hub,
    store::{Device, DeviceSource, Scope, now_ms},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, TransactionBehavior};
use std::sync::{Arc, atomic::Ordering};

/// Erro de guarda, distinto de uma falha do callback do consumidor.
#[derive(Debug)]
pub(crate) struct AutorizacaoEncerrada;
impl std::fmt::Display for AutorizacaoEncerrada {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not_allowed: autorização original encerrada")
    }
}
impl std::error::Error for AutorizacaoEncerrada {}

type LinhaDispositivo = (String, String, String, Option<String>, String, Option<i64>, i64, Option<i64>);

/// RAM privada, não serializável. Só a autenticação confiável produz Dispositivo.
#[derive(Clone, Debug)]
pub struct Autoridade {
    original: Original,
    captura: Arc<()>,
}
#[derive(Clone, Debug)]
enum Original {
    Local,
    Dispositivo {
        chave: String,
        origem: DeviceSource,
        concessao: Option<String>,
        escopos: Vec<Scope>,
        prazo: Option<i64>,
        dono: Option<String>,
        geracao_dono: u64,
        epoca_sincronizacao: u64,
        geracao: i64,
    },
    /// D430 PR-A: procedimento administrativo inicial, intenção fechada. Nunca vira Caller.
    Inicial(Arc<personal_link::CapturaInicial>),
    Canal(Arc<personal_channel::CapturaCanal>),
}

/// Captura Device + concessão na MESMA Connection, antes de qualquer fila/await.
/// A chave já vem do transporte criptográfico, não de argumentos de uma ação.
pub(crate) fn autenticar(hub: &Hub, chave: &str) -> std::result::Result<(Device, Caller), RejectCode> {
    hub.cloud
        .with_transport_owner(|dono, geracao_dono| {
            let mut db = hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let sincronizacao = hub.cloud_synced.congelar();
            let resultado = (|| {
                let dispositivo = dispositivo(&tx, chave).map_err(|_| RejectCode::Unauthorized)?;
                if dispositivo.source == DeviceSource::Account
                    && (!hub.cloud_synced.load(Ordering::Acquire)
                        || sincronizacao.epoca() == u64::MAX
                        || dono.is_none()
                        || dispositivo.lease_until.is_none_or(|prazo| prazo <= now_ms()))
                {
                    return Err(RejectCode::LeaseExpired);
                }
                let original = Autoridade {
                    captura: Arc::new(()),
                    original: Original::Dispositivo {
                        chave: chave.to_owned(),
                        origem: dispositivo.source,
                        concessao: dispositivo.grant_id.clone(),
                        escopos: dispositivo.scopes.clone(),
                        prazo: dispositivo.lease_until,
                        dono: if dispositivo.source == DeviceSource::Account { dono.map(str::to_owned) } else { None },
                        geracao_dono,
                        epoca_sincronizacao: sincronizacao.epoca(),
                        geracao: concessao::geracao(&tx, chave).map_err(|_| RejectCode::Unauthorized)?,
                    },
                };
                let caller = Caller::Device {
                    key: chave.to_owned(),
                    scopes: dispositivo.scopes.clone(),
                    autoridade: Some(Arc::new(original)),
                };
                Ok((dispositivo, caller))
            })();
            tx.commit()?;
            Ok(resultado)
        })
        .map_err(|_| RejectCode::Unauthorized)?
}

/// Leitura estrita: corrupção de origem/escopos não vira outra identidade.
fn dispositivo(db: &Connection, chave: &str) -> Result<Device> {
    // HOST-R1-02 / C-1: origem pessoal (inclusive Local pareado depois) não usa canal/subscription genéricos até o C4;
    // a mesma recusa de `devices::authorize`, aqui na leitura que autenticar/reconferir/conferir usam. Erro de leitura fecha.
    ensure!(
        crate::store::personal_bootstrap::personal_origin_on(db, chave)?.is_none(),
        "not_allowed: origem pessoal sem uso genérico"
    );
    let (nome, plataforma, origem, concessao, escopos, prazo, criado, visto): LinhaDispositivo = db.query_row(
        "SELECT name,platform,source,grant_id,scopes,lease_until,created_at,last_seen_at FROM devices WHERE key=?1",
        [chave],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )?;
    Ok(Device {
        key: chave.to_owned(),
        name: nome,
        platform: plataforma,
        source: serde_json::from_value(origem.into())?,
        grant_id: concessao,
        scopes: serde_json::from_str(&escopos)?,
        lease_until: prazo,
        created_at: criado,
        last_seen_at: visto,
    })
}

impl Autoridade {
    /// Fixture de testes: o próprio computador do Hub.
    #[cfg(test)]
    pub(crate) fn local() -> Self {
        Self { original: Original::Local, captura: Arc::new(()) }
    }

    /// Recupera a autoridade ORIGINAL do transporte; jamais captura outra concessão.
    /// Local vem somente do adaptador loopback autenticado. `DeviceSource::Local` segue Device.
    pub(crate) fn capturar(hub: &Hub, caller: &Caller) -> Result<Self> {
        let original = match caller {
            Caller::Local => Self { original: Original::Local, captura: Arc::new(()) },
            Caller::Device { key, scopes, autoridade: Some(original) } => {
                ensure!(scopes.contains(&Scope::Control), "not_allowed: aparelho sem controle");
                original.conferir_identidade(key, scopes)?;
                original.as_ref().clone()
            }
            _ => anyhow::bail!("not_allowed: autoridade original de controle ausente"),
        };
        original.with_db(hub, None, |_| Ok(()))?;
        Ok(original)
    }

    /// D395: preferência do PRÓPRIO app (o idioma), inclusive de aparelho sem Control. Usa a MESMA
    /// autoridade original do transporte, sem promover a Local, sem capturar concessão nova e sem
    /// Authority paralela; confere na Tx da escrita, que recebe a geração original da concessão
    /// (`None` para o computador do Hub) para gravar junto.
    pub(crate) fn proprio_with_db<T>(
        hub: &Hub,
        caller: &Caller,
        escrever: impl FnOnce(&Connection, Option<i64>) -> Result<T>,
    ) -> Result<T> {
        match caller {
            Caller::Local => {
                Self { original: Original::Local, captura: Arc::new(()) }.with_db(hub, None, |tx| escrever(tx, None))
            }
            Caller::Device { key, scopes, autoridade: Some(original) } => {
                original.conferir_identidade(key, scopes)?;
                let Original::Dispositivo { geracao, .. } = &original.original else {
                    anyhow::bail!("not_allowed: identidade de transporte divergente")
                };
                let geracao = *geracao;
                crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
                original.with_db(hub, None, |tx| escrever(tx, Some(geracao)))
            }
            _ => anyhow::bail!("not_allowed: autoridade original ausente"),
        }
    }

    /// A geração atual da concessão de um aparelho (`None` se não há registro).
    pub(crate) fn geracao_atual(db: &Connection, chave: &str) -> Option<i64> {
        concessao::geracao(db, chave).ok()
    }

    fn conferir_identidade(&self, key: &str, scopes: &[Scope]) -> Result<()> {
        ensure!(
            matches!(&self.original, Original::Dispositivo { chave, escopos, .. } if chave == key && escopos == scopes),
            "not_allowed: identidade de transporte divergente"
        );
        Ok(())
    }

    /// Não abre Store/Cloud: só consulta a Connection já protegida e seu dono.
    pub(crate) fn conferir(
        &self,
        db: &Connection,
        dono: Option<&str>,
        geracao_dono_atual: u64,
        epoca_sincronizacao_atual: u64,
        sincronizado: bool,
    ) -> Result<()> {
        if let Original::Canal(canal) = &self.original {
            return canal.conferir(db);
        }
        if let Original::Inicial(inicial) = &self.original {
            return inicial.conferir(db);
        }
        if let Original::Dispositivo {
            chave,
            origem,
            concessao,
            escopos,
            prazo,
            dono: original,
            geracao_dono,
            epoca_sincronizacao,
            geracao,
        } = &self.original
        {
            let atual = dispositivo(db, chave)?;
            ensure!(
                atual.source == *origem
                    && atual.grant_id == *concessao
                    && atual.scopes == *escopos
                    && concessao::geracao(db, chave)? == *geracao,
                "not_allowed: a concessão original mudou; refaça a solicitação"
            );
            if *origem == DeviceSource::Account {
                ensure!(
                    original.is_some()
                        && original.as_deref() == dono
                        && *geracao_dono == geracao_dono_atual
                        && sincronizado
                        && *epoca_sincronizacao == epoca_sincronizacao_atual
                        && epoca_sincronizacao_atual < u64::MAX
                        && prazo.is_some_and(|ate| ate > now_ms())
                        && atual.lease_until.zip(*prazo).is_some_and(|(atual, original)| atual >= original),
                    "not_allowed: dono, época de sincronização ou prazo original encerrado"
                );
            } else {
                ensure!(atual.lease_until == *prazo, "not_allowed: prazo original alterado");
            }
        }
        Ok(())
    }

    /// Classifica a origem imutável do transporte, nunca a indicação enviada pelo cliente.
    pub(crate) fn outro_aparelho(&self) -> bool {
        matches!(&self.original, Original::Dispositivo { origem: DeviceSource::Account, escopos, .. }
            if escopos.contains(&Scope::Control))
    }

    pub(crate) fn dono(&self) -> Option<&str> {
        match &self.original {
            Original::Dispositivo { dono, .. } => dono.as_deref(),
            Original::Local | Original::Inicial(_) | Original::Canal(_) => None,
        }
    }

    fn conferir_contas(
        &self,
        contas: Option<&Authority>,
        db: &Connection,
        dono: Option<&str>,
        sincronizado: bool,
    ) -> Result<()> {
        if let Some(contas) = contas {
            let mesmo_sujeito = match (&self.original, contas) {
                (Original::Local, Authority::Local) => true,
                (
                    Original::Dispositivo { chave, origem: DeviceSource::Account, .. },
                    Authority::Owner { key, transporte, .. },
                ) => chave == key && Arc::ptr_eq(&self.captura, &transporte.captura),
                _ => false,
            };
            ensure!(mesmo_sujeito, "not_allowed: autoridade de contas pertence a outro sujeito ou domínio");
            contas.conferir_saida(db, dono, sincronizado)?;
        }
        Ok(())
    }

    fn conferir_guardas(
        &self,
        db: &Connection,
        dono: Option<&str>,
        geracao_dono: u64,
        epoca_sincronizacao: u64,
        sincronizado: bool,
        contas: Option<&Authority>,
    ) -> Result<()> {
        self.conferir(db, dono, geracao_dono, epoca_sincronizacao, sincronizado)
            .and_then(|()| self.conferir_contas(contas, db, dono, sincronizado))
            .map_err(|_| AutorizacaoEncerrada.into())
    }

    /// Valida antes do efeito e do commit na mesma `TxImmediate`/Connection.
    /// O callback é síncrono, sem await/rede/Store/Cloud ou saída irreversível.
    pub(crate) fn with_db<T>(
        &self,
        hub: &Hub,
        contas: Option<&Authority>,
        escrever: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        hub.cloud.with_transport_owner(|dono, geracao_dono| {
            let mut db = hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let sincronizacao = hub.cloud_synced.congelar();
            let conferir = || {
                let sincronizado = hub.cloud_synced.load(Ordering::Acquire);
                self.conferir_guardas(&tx, dono, geracao_dono, sincronizacao.epoca(), sincronizado, contas)
            };
            conferir()?;
            let resposta = escrever(&tx)?;
            conferir()?;
            tx.commit()?;
            Ok(resposta)
        })
    }

    /// Commit durável e última admissão sob as mesmas guardas originais; o enqueue é síncrono e não toca DB.
    pub(crate) fn commit_and_admit<T, R>(
        &self,
        hub: &Hub,
        contas: &Authority,
        write: impl FnOnce(&Connection) -> Result<T>,
        admit: impl FnOnce(T) -> R,
    ) -> Result<R> {
        hub.cloud.with_transport_owner(|owner, generation| {
            let mut db = hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let sync = hub.cloud_synced.congelar();
            self.conferir_guardas(
                &tx,
                owner,
                generation,
                sync.epoca(),
                hub.cloud_synced.load(Ordering::Acquire),
                Some(contas),
            )?;
            let value = write(&tx)?;
            self.conferir_guardas(
                &tx,
                owner,
                generation,
                sync.epoca(),
                hub.cloud_synced.load(Ordering::Acquire),
                Some(contas),
            )?;
            tx.commit()?;
            Ok(admit(value))
        })
    }

    /// Resposta e vaga reservadas ANTES. Só o último enqueue síncrono dentro.
    /// Nenhum commit, pós-checagem ou outro passo falível depois da admissão.
    pub(crate) fn com_saida<T>(&self, hub: &Hub, contas: Option<&Authority>, enviar: impl FnOnce() -> T) -> Result<T> {
        com_saida_validada(hub, OrigemSaida::Controle(self, contas), |_, _, _, _, _| Ok(()), enviar)
    }
}

/// Origem capturada pelo produtor correspondente; Ponte nunca vira Authority/Control.
#[derive(Clone, Copy)]
pub(crate) enum OrigemSaida<'a> {
    Controle(&'a Autoridade, Option<&'a Authority>),
    Ponte(&'a crate::fork::ponte_credencial::Origem),
}

/// Corpo canônico único de saída: pré-checagem usa SOMENTE a `Connection` recebida;
/// enviar é um callback síncrono distinto, sem `Connection`. Sem commit após admissão.
pub(crate) fn com_saida_validada<T>(
    hub: &Hub,
    origem: OrigemSaida<'_>,
    conferir: impl FnOnce(&Connection, Option<&str>, u64, u64, bool) -> Result<()>,
    enviar: impl FnOnce() -> T,
) -> Result<T> {
    hub.cloud.with_transport_owner(|dono, geracao_dono| {
        let executar = || {
            let mut db = hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let sincronizacao = hub.cloud_synced.congelar();
            let sincronizado = hub.cloud_synced.load(Ordering::Acquire);
            match origem {
                OrigemSaida::Controle(original, contas) => {
                    original.conferir_guardas(&tx, dono, geracao_dono, sincronizacao.epoca(), sincronizado, contas)?;
                }
                OrigemSaida::Ponte(original) => original.conferir_db(&tx)?,
            }
            conferir(&tx, dono, geracao_dono, sincronizacao.epoca(), sincronizado)?;
            Ok(enviar())
        };
        match origem {
            OrigemSaida::Ponte(original) => original.com_original(hub, executar),
            OrigemSaida::Controle(..) => executar(),
        }
    })
}

/// Renova apenas a autenticação de uma NOVA requisição no mesmo canal.
/// Valida a mesma geração/grant/escopos/dono na Connection da captura. Pedidos
/// já enfileirados conservam Arc/prazo/época originais e nunca recebem esta renovação.
pub(crate) fn reconferir(hub: &Hub, caller: &Caller) -> std::result::Result<(Device, Caller), RejectCode> {
    let Caller::Device { key, scopes, autoridade: Some(original) } = caller else {
        return Err(RejectCode::Unauthorized);
    };
    hub.cloud
        .with_transport_owner(|dono, geracao_dono| {
            let mut db = hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let sincronizacao = hub.cloud_synced.congelar();
            let resultado = (|| -> std::result::Result<(Device, Caller), RejectCode> {
                original.conferir_identidade(key, scopes).map_err(|_| RejectCode::Unauthorized)?;
                let dispositivo = dispositivo(&tx, key).map_err(|_| RejectCode::Revoked)?;
                if dispositivo.source == DeviceSource::Account
                    && (!hub.cloud_synced.load(Ordering::Acquire)
                        || sincronizacao.epoca() == u64::MAX
                        || dono.is_none()
                        || dispositivo.lease_until.is_none_or(|prazo| prazo <= now_ms()))
                {
                    return Err(RejectCode::LeaseExpired);
                }
                let mut renovada = original.as_ref().clone();
                renovada.captura = Arc::new(());
                if let Original::Dispositivo { prazo, epoca_sincronizacao, .. } = &mut renovada.original {
                    *prazo = dispositivo.lease_until;
                    *epoca_sincronizacao = sincronizacao.epoca();
                }
                renovada
                    .conferir(&tx, dono, geracao_dono, sincronizacao.epoca(), hub.cloud_synced.load(Ordering::Acquire))
                    .map_err(|_| RejectCode::Revoked)?;
                let caller =
                    Caller::Device { key: key.clone(), scopes: scopes.clone(), autoridade: Some(Arc::new(renovada)) };
                Ok((dispositivo, caller))
            })();
            tx.commit()?;
            Ok(resultado)
        })
        .map_err(|_| RejectCode::Unauthorized)?
}

/// Revalidação de Caller real no dispatch, inclusive antes da captura do consumidor.
pub(crate) fn conferir_caller(hub: &Hub, caller: &Caller) -> Result<()> {
    if let Caller::Device { key, scopes, autoridade: Some(original) } = caller {
        original.conferir_identidade(key, scopes)?;
        original.with_db(hub, None, |_| Ok(()))?;
    }
    Ok(())
}

pub(crate) fn com_saida_caller<T>(hub: &Hub, caller: &Caller, enviar: impl FnOnce() -> T) -> Result<T> {
    let Caller::Device { key, scopes, autoridade: Some(original) } = caller else {
        anyhow::bail!("not_allowed: autoridade original de saída ausente");
    };
    original.conferir_identidade(key, scopes)?;
    original.com_saida(hub, None, enviar)
}

#[cfg(test)]
mod provas;
