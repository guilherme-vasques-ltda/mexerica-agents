//! Verificação da release anterior à publicação. Não modifica slots.
use super::{
    armazenamento::Diretorio,
    manifesto,
    politica::Politica,
    tuf::{self, Cadeia},
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, fs::File};

pub struct Documentos<'a> {
    pub timestamp: &'a [u8],
    pub snapshot: &'a [u8],
    pub targets: &'a [u8],
    pub delegados: &'a BTreeMap<String, Vec<u8>>,
    pub manifesto: &'a [u8],
    pub assinatura: &'a [u8],
}

pub struct Release {
    pub manifesto: manifesto::Manifesto,
    pub prazo_limite: i64,
}

/// O chamador deve qualificar o relógio e manter a trava até concluir a transação.
/// O histórico autenticado é persistido antes do parse semântico do manifesto.
#[expect(clippy::too_many_arguments, reason = "todos os insumos vêm de estado root distinto e são conferidos juntos")]
pub fn verificar(
    dir: &Diretorio,
    publicacao: &Diretorio,
    trava: &File,
    politica: &Politica,
    documentos: &Documentos<'_>,
    agora: i64,
    marca: u64,
    piso: u64,
) -> Result<Release> {
    dir.validar_trava(trava)?;
    dir.exigir_sem_bloqueio()?;
    publicacao.exigir_sem_emergencia()?;
    let historico = dir.carregar_historico_tuf(trava)?;
    let raiz = dir.raiz_tuf(trava, agora)?;
    let cadeia = Cadeia {
        timestamp: documentos.timestamp,
        snapshot: documentos.snapshot,
        targets: documentos.targets,
        delegados: documentos.delegados,
        anteriores: &historico.versoes,
    };
    let resultado = tuf::verificar_cadeia(&raiz, &cadeia, agora, "manifesto.json")?;
    tuf::verificar_alvo(documentos.manifesto, &resultado.alvo)?;
    dir.registrar_historico_tuf(trava, raiz.version, &resultado)?;
    let manifesto = manifesto::verificar(documentos.manifesto, documentos.assinatura, &politica.chave()?)?;
    let wrapper = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    let prazo_limite = manifesto::validar(&manifesto, &politica.manifesto(&wrapper), agora, marca, piso)?;
    Ok(Release { manifesto, prazo_limite })
}

/// Resultado autenticado da consulta de rede. Ainda não publica arquivos.
pub struct ReleaseBaixada {
    pub release: Release,
    pub manifesto: Vec<u8>,
    pub assinatura: Vec<u8>,
}

/// Limite de versões de raiz aceitas por execução (cadeia longa exige ação administrativa).
const MAX_ROTACOES_RAIZ: usize = 32;

async fn documento(
    transporte: &super::rede::Transporte,
    recurso: super::rede::Recurso,
    limite: u64,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    transporte.baixar(recurso, &mut bytes, limite).await?;
    Ok(bytes)
}

/// Consulta somente a origem fixada na política; o snapshot é autenticado
/// antes de determinar nomes de papéis delegados. Nenhum nome vem do Hub.
pub async fn baixar_verificada(
    dir: &Diretorio,
    publicacao: &Diretorio,
    trava: &File,
    politica: &Politica,
    agora: i64,
    marca: u64,
    piso: u64,
) -> Result<ReleaseBaixada> {
    use super::rede::{Recurso, Transporte};
    let transporte = Transporte::novo(&politica.endpoint, politica.tamanho_maximo, politica.tempo_rede_segundos)?;
    let limite = politica.tamanho_maximo.min(1024 * 1024);
    dir.validar_trava(trava)?;
    dir.exigir_sem_bloqueio()?;
    publicacao.exigir_sem_emergencia()?;
    // TUF 5.3: antes do timestamp, segue a cadeia de rotação da raiz publicada
    // na mesma origem, uma versão por vez, até a próxima não existir.
    let mut rotacoes = 0usize;
    loop {
        let proxima = dir.carregar_historico_tuf(trava)?.raiz.checked_add(1).context("versão de raiz esgotada")?;
        let mut bytes = Vec::new();
        if transporte.baixar_se_existir(Recurso::Raiz(proxima), &mut bytes, limite).await?.is_none() {
            break;
        }
        rotacoes += 1;
        ensure!(rotacoes <= MAX_ROTACOES_RAIZ, "rotações de raiz demais numa só atualização");
        dir.atualizar_raiz_tuf(trava, &bytes)?;
    }
    let historico = dir.carregar_historico_tuf(trava)?;
    let raiz = dir.raiz_tuf(trava, agora)?;
    let timestamp = documento(&transporte, Recurso::Timestamp, limite).await?;
    let snapshot = documento(&transporte, Recurso::Snapshot, limite).await?;
    let nomes = tuf::listar_delegados(&raiz, &timestamp, &snapshot, &historico.versoes, agora)?;
    let targets = documento(&transporte, Recurso::Targets, limite).await?;
    let mut delegados = BTreeMap::new();
    for (nome, tamanho) in nomes {
        let bytes = documento(&transporte, Recurso::Delegado(nome.clone()), tamanho.min(limite)).await?;
        delegados.insert(nome, bytes);
    }
    let manifesto = documento(&transporte, Recurso::Manifesto, limite.min(manifesto::MAX_MANIFESTO as u64)).await?;
    let assinatura = documento(&transporte, Recurso::Assinatura, limite.min(64)).await?;
    let documentos = Documentos {
        timestamp: &timestamp,
        snapshot: &snapshot,
        targets: &targets,
        delegados: &delegados,
        manifesto: &manifesto,
        assinatura: &assinatura,
    };
    // A rede pode demorar até o prazo da política por arquivo. Não valide a
    // janela ou expiração usando somente o horário anterior aos downloads.
    let agora_final = dir.hora_conferida(trava)?;
    let release = verificar(dir, publicacao, trava, politica, &documentos, agora_final, marca, piso)?;
    Ok(ReleaseBaixada { release, manifesto, assinatura })
}
