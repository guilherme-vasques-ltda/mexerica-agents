//! Marketplace 1a: recibo de cada ação sobre uma conta (conectar, renomear, desconectar). Só identificador opaco,
//! serviço, Space, resultado e horário; nunca nome dado pela pessoa, token, cabeçalho, URL ou conteúdo.
#[cfg(test)]
use crate::{LockExt, store::Store};
use anyhow::Result;
use rusqlite::{Connection, params};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Acao {
    Conectar,
    Renomear,
    Desconectar,
    /// O app Google do Space foi cadastrado ou trocado (o recibo não guarda ID nem chave).
    GoogleAppSalvar,
    GoogleAppRemover,
    AppOauthSalvar,
    AppOauthRemover,
}

impl Acao {
    fn as_str(self) -> &'static str {
        match self {
            Self::Conectar => "conectar",
            Self::Renomear => "renomear",
            Self::Desconectar => "desconectar",
            Self::GoogleAppSalvar => "google_app_salvar",
            Self::GoogleAppRemover => "google_app_remover",
            Self::AppOauthSalvar => "app_oauth_salvar",
            Self::AppOauthRemover => "app_oauth_remover",
        }
    }
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Recibo {
    pub acao: String,
    /// Identificador opaco da conta (o do vínculo OAuth do Hub), não o `id` nem o nome.
    pub conta: String,
    pub servico: String,
    pub space: String,
    pub resultado: String,
}

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS mexerica_conta_recibos(
            id INTEGER PRIMARY KEY AUTOINCREMENT, em INTEGER NOT NULL, acao TEXT NOT NULL, conta TEXT NOT NULL,
            servico TEXT NOT NULL, space TEXT NOT NULL, resultado TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS mexerica_conta_recibos_conta ON mexerica_conta_recibos(conta);",
    )?;
    Ok(())
}

/// Grava o recibo na transação recebida (a mesma que sela o cofre da conta).
pub(crate) fn registrar_em(
    db: &Connection,
    acao: Acao,
    conta: &str,
    servico: Option<&str>,
    space: &str,
    resultado: &str,
) -> Result<()> {
    db.execute(
        "INSERT INTO mexerica_conta_recibos(em,acao,conta,servico,space,resultado) VALUES(?1,?2,?3,?4,?5,?6)",
        params![crate::store::now_ms(), acao.as_str(), conta, servico.unwrap_or(""), space, resultado],
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn da_conta(store: &Store, conta: &str) -> Result<Vec<Recibo>> {
    let db = store.db.locked();
    let mut st =
        db.prepare("SELECT acao,conta,servico,space,resultado FROM mexerica_conta_recibos WHERE conta=?1 ORDER BY id")?;
    let rows = st
        .query_map([conta], |r| {
            Ok(Recibo { acao: r.get(0)?, conta: r.get(1)?, servico: r.get(2)?, space: r.get(3)?, resultado: r.get(4)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
