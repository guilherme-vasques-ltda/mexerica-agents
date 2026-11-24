//! Vínculo do Hub com um workspace. Só [`super::gravacao`] grava (atrás de um interruptor
//! desligado). Sem vínculo gravado, o Hub segue `personal:unbound`. Vínculo ilegível nega o acesso
//! remoto.
use super::Recusa;
use super::token::Afirmacoes;
use crate::LockExt;
use crate::hub::Hub;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

/// Linha `kv` do vínculo.
pub(crate) const CHAVE: &str = "fork.workspace.vinculo.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tipo {
    Pessoal {
        dono: String,
    },
    /// `criador` é `owner` enquanto seguir admin da org.
    Time {
        org: String,
        criador: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Vinculo {
    pub(crate) tipo: Tipo,
    /// `iss` exato da instância Clerk.
    pub(crate) emissor: String,
    /// JWKS fixada, no mesmo host do emissor.
    pub(crate) jwks: reqwest::Url,
    /// Valores aceitos de `azp`, por igualdade exata.
    pub(crate) azp: Vec<String>,
}

pub(crate) enum Estado {
    SemVinculo,
    Vinculado(Vinculo),
    Ilegivel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Papel {
    Dono,
    Admin,
    Membro,
}

impl Papel {
    pub(crate) fn nome(self) -> &'static str {
        match self {
            Self::Dono => "owner",
            Self::Admin => "admin",
            Self::Membro => "member",
        }
    }

    pub(crate) fn de_nome(nome: &str) -> Option<Self> {
        [Self::Dono, Self::Admin, Self::Membro].into_iter().find(|p| p.nome() == nome)
    }
}

impl Vinculo {
    pub(crate) fn kind(&self) -> &'static str {
        match self.tipo {
            Tipo::Pessoal { .. } => "personal",
            Tipo::Time { .. } => "team",
        }
    }

    pub(crate) fn workspace_id(&self) -> String {
        match &self.tipo {
            Tipo::Pessoal { dono } => format!("personal:{dono}"),
            Tipo::Time { org, .. } => format!("org:{org}"),
        }
    }

    /// Papel no Hub, só a partir das afirmações verificadas do token.
    pub(crate) fn papel(&self, afirmacoes: &Afirmacoes) -> Result<Papel, Recusa> {
        match &self.tipo {
            Tipo::Pessoal { dono } if afirmacoes.sub == *dono => Ok(Papel::Dono),
            Tipo::Pessoal { .. } => Err(Recusa::WorkspaceForbidden),
            Tipo::Time { org, criador } => {
                let Some(o) = afirmacoes.org.as_ref().filter(|o| o.id == *org) else {
                    return Err(Recusa::WorkspaceForbidden);
                };
                match o.rol.as_str() {
                    "admin" if afirmacoes.sub == *criador => Ok(Papel::Dono),
                    "admin" => Ok(Papel::Admin),
                    "member" => Ok(Papel::Membro),
                    _ => Err(Recusa::WorkspaceForbidden),
                }
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Gravado {
    schema: u32,
    kind: String,
    owner_user_id: Option<String>,
    org_id: Option<String>,
    creator_user_id: Option<String>,
    issuer: String,
    jwks_url: String,
    authorized_parties: Vec<String>,
}

fn texto(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty() && s.len() <= 256)
}

/// `https`, com host, sem credencial, consulta nem fragmento.
fn https(raw: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(raw).ok()?;
    let limpa = url.scheme() == "https"
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    limpa.then_some(url)
}

/// Porta da gravação do vínculo (a etapa que grava passa por aqui): o `team` é recusado enquanto
/// [`super::VINCULO_TIME_LIBERADO`] estiver desligado.
pub(crate) fn conferir_gravacao(v: &Vinculo, time_liberado: bool) -> Result<(), Recusa> {
    match v.tipo {
        Tipo::Time { .. } if !time_liberado => Err(Recusa::WorkspaceForbidden),
        _ => Ok(()),
    }
}

pub(crate) fn interpretar(raw: &str) -> Option<Vinculo> {
    let g: Gravado = serde_json::from_str(raw).ok()?;
    if g.schema != 1 {
        return None;
    }
    // Campo presente tem de ser válido: vazio ou longo demais não conta como ausente.
    let valido = |c: &Option<String>| c.as_deref().is_none_or(|s| texto(Some(s.to_owned())).is_some());
    if ![&g.owner_user_id, &g.org_id, &g.creator_user_id].into_iter().all(valido) {
        return None;
    }
    let tipo = match (g.kind.as_str(), texto(g.owner_user_id), texto(g.org_id), texto(g.creator_user_id)) {
        ("personal", Some(dono), None, None) => Tipo::Pessoal { dono },
        ("team", None, Some(org), Some(criador)) => Tipo::Time { org, criador },
        _ => return None,
    };
    let emissor = https(&g.issuer)?;
    let jwks = https(&g.jwks_url)?;
    if jwks.host_str() != emissor.host_str() || jwks.port_or_known_default() != emissor.port_or_known_default() {
        return None;
    }
    if g.authorized_parties.is_empty() || g.authorized_parties.iter().any(|a| a.is_empty() || a.len() > 512) {
        return None;
    }
    Some(Vinculo { tipo, emissor: g.issuer, jwks, azp: g.authorized_parties })
}

pub(crate) fn ler(hub: &Hub) -> Estado {
    ler_on(&hub.store.db.locked())
}

/// Na `Connection` já protegida (a Tx do efeito).
pub(crate) fn ler_on(db: &Connection) -> Estado {
    let lido: rusqlite::Result<Option<String>> =
        db.query_row("SELECT v FROM kv WHERE k=?1", [CHAVE], |r| r.get(0)).optional();
    match lido {
        Ok(None) => Estado::SemVinculo,
        Ok(Some(raw)) => interpretar(&raw).map_or(Estado::Ilegivel, Estado::Vinculado),
        Err(_) => Estado::Ilegivel,
    }
}

/// Fixture de prova: grava o vínculo direto, sem a prova do dono de [`super::gravacao`].
#[cfg(test)]
pub(crate) fn gravar_para_prova(hub: &Hub, raw: &str) {
    hub.store
        .db
        .locked()
        .execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            rusqlite::params![CHAVE, raw],
        )
        .expect("vínculo de prova");
}
