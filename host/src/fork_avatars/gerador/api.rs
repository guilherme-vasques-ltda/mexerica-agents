//! Intenções do renderer. O renderer envia briefing e IDs; o Hub valida, resolve a
//! credencial do cofre, executa o job e serve os quadros. Nenhum segredo volta.
use super::{JOB_TTL_MS, Status, atlas, briefing, chave, conjuntos, cota, falha, id_gerado, provedor};
use crate::LockExt;
use crate::hub::Hub;
use crate::store::now_ms;
use anyhow::Result;
use base64::Engine as _;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn str_arg<'a>(b: &'a Value, k: &str) -> Result<&'a str> {
    b[k].as_str().ok_or_else(|| falha("PEDIDO_INVALIDO"))
}

/// Sem chave configurada (ou cofre fechado) o fluxo não roda: estado falso, geração recusada.
pub(crate) fn estado(store: &crate::store::Store) -> Value {
    // `cofre` olha a chave do cofre direto: um slot vazio lido travado não diferencia
    // "sem chave" de "cofre fechado"; o app precisa saber qual mensagem mostrar.
    let cofre = store.secret_key.locked().is_some();
    let processador = atlas::disponivel();
    let gerador = processador && cofre && chave::configurada(store).unwrap_or(false);
    json!({"gerador": gerador, "cofre": cofre, "processador": processador})
}

fn job_dto(id: &uuid::Uuid, job: &super::Job) -> Value {
    let mut v = json!({"id": id.to_string(), "status": job.status.id()});
    if let Some(c) = job.codigo {
        v["codigo"] = c.into();
    }
    if job.status == Status::Ready {
        v["conjunto"] = json!({"id": id.to_string(), "nome": job.nome});
    }
    v
}

pub async fn call(hub: &Arc<Hub>, method: &str, b: Value) -> Result<Value> {
    match method {
        "mexeriquinhaGeradorEstado" => Ok(estado(&hub.store)),
        // `chave` vazia = apagar. O valor sai do pedido para o cofre sem retorno.
        "mexeriquinhaGeradorChave" => {
            let chave = str_arg(&b, "chave")?;
            if chave.len() > 256 || chave.chars().any(char::is_control) {
                return Err(falha("PEDIDO_INVALIDO"));
            }
            chave::gravar(&hub.store, chave)?;
            Ok(estado(&hub.store))
        }
        "mexeriquinhaGerar" => gerar(hub, &b),
        "mexeriquinhaGeradorJob" => {
            let id = uuid::Uuid::parse_str(str_arg(&b, "id")?).map_err(|_| falha("NAO_ENCONTRADO"))?;
            let mut jobs = hub.gerador.jobs.locked();
            jobs.mapa.retain(|_, j| now_ms() - j.criado_em < JOB_TTL_MS || j.status == Status::Generating);
            let job = jobs.mapa.get(&id).ok_or_else(|| falha("NAO_ENCONTRADO"))?;
            Ok(job_dto(&id, job))
        }
        "mexeriquinhaGeradorCancelar" => {
            let id = uuid::Uuid::parse_str(str_arg(&b, "id")?).map_err(|_| falha("NAO_ENCONTRADO"))?;
            let mut jobs = hub.gerador.jobs.locked();
            let job = jobs.mapa.get_mut(&id).ok_or_else(|| falha("NAO_ENCONTRADO"))?;
            if job.status == Status::Generating {
                job.status = Status::Cancelled;
                job.cancelar.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            Ok(json!({}))
        }
        "mexeriquinhaConjuntoSalvar" => {
            let id = uuid::Uuid::parse_str(str_arg(&b, "id")?).map_err(|_| falha("NAO_ENCONTRADO"))?;
            let (nome, pronto) = {
                let jobs = hub.gerador.jobs.locked();
                let job = jobs.mapa.get(&id).ok_or_else(|| falha("CONJUNTO_INDISPONIVEL"))?;
                (job.nome.clone(), job.status == Status::Ready)
            };
            if !pronto {
                return Err(falha("CONJUNTO_INDISPONIVEL"));
            }
            let fallback = b["fallbackConjuntoId"].as_str().unwrap_or("base").to_owned();
            let conjunto = tokio::task::spawn_blocking({
                let hub = hub.clone();
                move || conjuntos::salvar(&hub.store, &id, &nome, &fallback)
            })
            .await??;
            Ok(json!({"conjunto": conjunto}))
        }
        "mexeriquinhaConjuntos" => Ok(json!({"conjuntos": conjuntos::listar(&hub.store)?})),
        "mexeriquinhaQuadro" => {
            let asset_id = str_arg(&b, "assetId")?;
            let tamanho = b["tamanho"].as_u64().and_then(|t| u32::try_from(t).ok()).unwrap_or_default();
            let quadro = str_arg(&b, "quadro")?;
            let data = tokio::task::spawn_blocking({
                let hub = hub.clone();
                let asset_id = asset_id.to_owned();
                let quadro = quadro.to_owned();
                move || conjuntos::quadro(&hub.store, &asset_id, tamanho, &quadro)
            })
            .await??;
            Ok(json!({"data": base64::engine::general_purpose::STANDARD.encode(data), "mime": "image/webp"}))
        }
        "mexeriquinhaConjuntoApagar" => {
            let asset_id = str_arg(&b, "assetId")?;
            if !id_gerado(asset_id) {
                return Err(falha("NAO_ENCONTRADO"));
            }
            tokio::task::spawn_blocking({
                let hub = hub.clone();
                let asset_id = asset_id.to_owned();
                move || conjuntos::apagar(&hub.store, &asset_id)
            })
            .await??;
            Ok(json!({}))
        }
        _ => Err(falha("PEDIDO_INVALIDO")),
    }
}

/// Gate de geração: SEM PROCESSADOR nem chave não há nenhum caminho pago —
/// recusa antes da reserva de cota e de qualquer chamada ao provedor.
pub(crate) fn gate_geracao(store: &crate::store::Store) -> Result<()> {
    if !atlas::disponivel() {
        return Err(falha("GERADOR_SEM_PROCESSADOR"));
    }
    chave::resolver(store).map(|_| ())
}

fn gerar(hub: &Arc<Hub>, b: &Value) -> Result<Value> {
    let briefing = briefing::validar(b)?;
    gate_geracao(&hub.store)?;
    // Sem credencial não há caminho de geração; a chave sai do cofre direto p/ o adaptador.
    let key = chave::resolver(&hub.store)?;
    let id = uuid::Uuid::new_v4();
    {
        let mut jobs = hub.gerador.jobs.locked();
        jobs.mapa.retain(|_, j| now_ms() - j.criado_em < JOB_TTL_MS || j.status == Status::Generating);
        if jobs.ativo {
            return Err(falha("GERADOR_OCUPADO"));
        }
        // Reserva atômica antes de qualquer espera: cota persistente consumida em falha.
        cota::reservar(&hub.store)?;
        jobs.ativo = true;
        jobs.mapa.insert(
            id,
            super::Job {
                status: Status::Generating,
                codigo: None,
                nome: briefing.nome.clone(),
                criado_em: now_ms(),
                cancelar: Arc::new(AtomicBool::new(false)),
            },
        );
    }
    let provedor: Arc<dyn provedor::Provedor> = Arc::new(provedor::OpenAi::new(key));
    tokio::spawn(super::executar_job(hub.clone(), id, briefing, provedor));
    Ok(json!({"id": id.to_string()}))
}
