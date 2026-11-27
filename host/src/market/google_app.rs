//! O "app Google da pessoa" (MEX-1447): o ID de cliente e a chave secreta do app OAuth que a PRÓPRIA pessoa criou no
//! Google Cloud, guardados no cofre, um por Space. Nada disso vem do código ou do chat.
//!
//! Regras:
//! - a chave secreta entra uma vez (pelo cartão seguro do app) e **nunca mais sai**: nem a resposta da API, nem recibo,
//!   nem log, nem espelho a mostram; a leitura devolve só o ID;
//! - o registro é selado com o slot do Space como dado associado (`vault::seal`): copiado para o slot de outro Space,
//!   não abre. O app de um Space não serve a outro;
//! - trocar ou remover o app encerra o login das contas Google do Space (elas foram emitidas por outro cliente);
//! - o recibo da troca nasce na mesma transação que sela o registro e só diz "salvou" ou "removeu".
use super::{
    contas,
    contas_recibos::{self, Acao},
    oauth, update_connectors_com, vault,
};
use crate::LockExt;
use crate::fork::conectores::catalogo_servicos::{self as catalogo, Auth, Servico, TipoConexao};
use crate::fork::conectores::oauth_registration;
use crate::store::Store;
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroizing;

const SUFIXO_ID: &str = ".apps.googleusercontent.com";
const ID_MAX: usize = 200;
const CHAVE_MIN: usize = 8;
const CHAVE_MAX: usize = 200;
/// Identificador opaco dos recibos do app Google (nunca o ID de cliente, e muito menos a chave).
const RECIBO_CONTA: &str = "app_google";

/// O que o cofre guarda por Space. O `Debug` não imprime a chave: nem um `{:?}` esquecido a vaza.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CredenciaisApp {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for CredenciaisApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthApp").finish_non_exhaustive()
    }
}

/// Cofre trancado vira o código estável `cofre_trancado` (o app o traduz); qualquer outro erro segue como veio.
fn do_cofre<T>(r: Result<T>) -> Result<T> {
    r.map_err(|e| if vault::indisponivel(&e) { anyhow!("{COFRE_TRANCADO}") } else { e })
}

pub(crate) const COFRE_TRANCADO: &str = "cofre_trancado: destrave o cofre do Hub para ver e salvar o app Google.";

fn slot(space: &str) -> String {
    format!("google_app.{space}")
}

fn erro_id() -> anyhow::Error {
    anyhow!("app_google_invalido: o ID do cliente termina em .apps.googleusercontent.com; copie-o da tela do Google.")
}

/// ID e chave como o Google os entrega: sem espaço nem quebra de linha (colar com sobra é o erro comum, e a sobra
/// aparada não vai para o cofre). Recusa sem repetir o valor na mensagem.
fn validar(client_id: &str, client_secret: &str) -> Result<CredenciaisApp> {
    let (id, chave) = (client_id.trim(), client_secret.trim());
    let limpo = |s: &str| !s.chars().any(|c| c.is_control() || c.is_whitespace());
    ensure!(id.len() > SUFIXO_ID.len() && id.len() <= ID_MAX && id.ends_with(SUFIXO_ID) && limpo(id), erro_id());
    ensure!(
        (CHAVE_MIN..=CHAVE_MAX).contains(&chave.len()) && limpo(chave),
        "app_google_invalido: a chave secreta parece incompleta; copie-a de novo da tela do Google."
    );
    Ok(CredenciaisApp { client_id: id.to_owned(), client_secret: chave.to_owned() })
}

/// O app do Space, aberto do cofre. `None` = ainda não cadastrado. Cofre trancado é erro (nunca "não cadastrado").
pub(crate) fn ler(store: &Store, space: &str) -> Result<Option<CredenciaisApp>> {
    ler_slot(store, &slot(space))
}

fn ler_slot(store: &Store, slot: &str) -> Result<Option<CredenciaisApp>> {
    // Sem a chave aberta não dá para saber se há app: "não cadastrado" seria mentira, então é cofre trancado.
    ensure!(store.secret_key.locked().is_some(), "{COFRE_TRANCADO}");
    // Leitura ESTRITA (`kv_read` + `open`): registro em texto claro, sem selo ou selado para outro slot nunca abre, e nada
    // é "migrado". O slot nasce com esta função; não há legado a aceitar.
    let Some(cru) = store.kv_read(slot)? else { return Ok(None) };
    let bruto =
        Zeroizing::new(vault::open(store, slot, &cru).map_err(|_| {
            anyhow!("app_google_ilegivel: o registro do app Google é inválido; cadastre o app de novo.")
        })?);
    Ok(Some(
        serde_json::from_str(&bruto)
            .map_err(|_| anyhow!("app_google_ilegivel: o registro do app Google é inválido."))?,
    ))
}

/// O que a tela pode ver: se há app e qual o ID. **Nunca a chave.** Cofre trancado: `None` (a tela pede para destrancar).
pub(crate) fn publico(store: &Store, space: &str) -> Option<Value> {
    ler(store, space).ok().map(publicar)
}

fn publicar(app: Option<CredenciaisApp>) -> Value {
    match app {
        Some(app) => json!({"configurado": true, "clientId": app.client_id}),
        None => json!({"configurado": false, "clientId": null}),
    }
}

/// A conta usa o app Google? (ficha com `Auth::AppProprio` de provedor Google.) Olha só a ficha, não o endereço.
fn conta_google(c: &super::Connector) -> bool {
    let (Some(servico), Some(tipo)) = (c.servico.as_deref(), c.tipo.as_deref()) else { return false };
    catalogo::servico(servico)
        .and_then(|f| f.tipo(tipo))
        .is_some_and(|t| matches!(t.auth, Auth::AppProprio { provedor: "google", .. }))
}

/// Grava (ou troca) o app e encerra o login das contas Google do Space, tudo numa transação com o recibo.
pub(crate) fn salvar(store: &Store, space: &str, client_id: &str, client_secret: &str) -> Result<Value> {
    store.require_space(space)?;
    let app = validar(client_id, client_secret)?;
    let slot = slot(space);
    let selado = selar(store, &slot, &app)?;
    escrever(store, space, &slot, Some(&selado), (Acao::GoogleAppSalvar, RECIBO_CONTA, "google"), conta_google)?;
    Ok(json!({"configurado": true, "clientId": app.client_id}))
}

/// Remove o app do Space e encerra o login das contas Google dele.
pub(crate) fn remover(store: &Store, space: &str) -> Result<Value> {
    store.require_space(space)?;
    escrever(store, space, &slot(space), None, (Acao::GoogleAppRemover, RECIBO_CONTA, "google"), conta_google)?;
    Ok(json!({"configurado": false, "clientId": null}))
}

fn escrever(
    store: &Store,
    space: &str,
    slot: &str,
    novo: Option<&str>,
    recibo: (Acao, &str, &str),
    pertence: impl Fn(&super::Connector) -> bool,
) -> Result<()> {
    do_cofre(update_connectors_com(
        store,
        |all| {
            for c in all.iter_mut().filter(|c| c.space == space && pertence(c)) {
                oauth_registration::apagar(c)?;
            }
            Ok(())
        },
        |tx, ()| {
            match novo {
                Some(selado) => vault::write_sealed(tx, slot, selado)?,
                None => {
                    tx.execute("DELETE FROM kv WHERE k=?1", [slot])?;
                }
            }
            let (acao, conta, servico) = recibo;
            contas_recibos::registrar_em(tx, acao, conta, Some(servico), space, "ok")
        },
    ))
}

fn selar(store: &Store, slot: &str, app: &CredenciaisApp) -> Result<String> {
    let bruto = Zeroizing::new(serde_json::to_string(app)?);
    do_cofre(vault::seal(store, slot, &bruto))
}

/// Para o login: copia o ID e a chave do Space para a conta (no cofre de conectores), sem redirecionamentos nem tokens.
/// Sem app cadastrado, recusa com o código estável que o app lê para abrir o guia.
pub(crate) fn para_a_conta(store: &Store, space: &str) -> Result<oauth::OAuth> {
    let app = ler(store, space)?.ok_or_else(|| anyhow!("{}", contas::APP_PROPRIO_AUSENTE))?;
    Ok(oauth::OAuth { client_id: app.client_id, client_secret: Some(app.client_secret), ..oauth::OAuth::default() })
}

pub(crate) struct AppDaFicha {
    servico: &'static str,
    tipo: &'static str,
    emissor: &'static str,
}

pub(crate) const APP_DA_FICHA_AUSENTE: &str = "app_da_ficha_ausente: cadastre o app OAuth desta ficha primeiro.";

impl AppDaFicha {
    pub(crate) fn novo(ficha: &Servico, tipo: &TipoConexao) -> Option<Self> {
        let Auth::AppProprio { emissor, configuracao: Some(_), .. } = tipo.auth else { return None };
        Some(Self { servico: ficha.id, tipo: tipo.id, emissor })
    }

    pub(crate) fn resolver(servico: &str, tipo: &str) -> Result<Self> {
        #[cfg(test)]
        {
            let ficha = catalogo::fixture_app_proprio();
            if ficha.id == servico && let Some(app) = ficha.tipo(tipo).and_then(|tipo| Self::novo(ficha, tipo)) {
                return Ok(app);
            }
        }
        catalogo::servico(servico)
            .and_then(|ficha| ficha.tipo(tipo).and_then(|tipo| Self::novo(ficha, tipo)))
            .ok_or_else(|| anyhow!("Este jeito de conectar não usa um app OAuth próprio."))
    }

    fn slot(&self, space: &str) -> String {
        format!("app_oauth.{}", json!([space, self.servico, self.tipo, self.emissor]))
    }

    pub(crate) fn ler(&self, store: &Store, space: &str) -> Result<Option<CredenciaisApp>> {
        ler_slot(store, &self.slot(space)).map_err(|_| anyhow!("Não foi possível ler o app OAuth no cofre."))
    }

    pub(crate) fn publico(&self, store: &Store, space: &str) -> Option<Value> {
        self.ler(store, space).ok().map(|app| json!({"configurado": app.is_some()}))
    }

    pub(crate) fn escrever(&self, store: &Store, space: &str, dados: Option<(&str, &str)>) -> Result<Value> {
        store.require_space(space)?;
        let slot = self.slot(space);
        let app = dados
            .map(|(id, chave)| {
                let validar = |s: &str| {
                    !s.is_empty() && s.len() <= 1024 && !s.chars().any(|c| c.is_control() || c.is_whitespace())
                };
                let (id, chave) = (id.trim(), chave.trim());
                ensure!(
                    validar(id) && validar(chave),
                    "app_oauth_invalido: preencha o ID e a chave do cliente sem espaços."
                );
                Ok(CredenciaisApp { client_id: id.to_owned(), client_secret: chave.to_owned() })
            })
            .transpose()?;
        let selado = app.as_ref().map(|app| selar(store, &slot, app)).transpose()?;
        let acao = if app.is_some() { Acao::AppOauthSalvar } else { Acao::AppOauthRemover };
        escrever(store, space, &slot, selado.as_deref(), (acao, self.tipo, self.servico), |c| {
            c.servico.as_deref() == Some(self.servico) && c.tipo.as_deref() == Some(self.tipo)
        })?;
        Ok(json!({"configurado": app.is_some()}))
    }
}
