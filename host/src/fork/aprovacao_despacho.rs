//! Porteiro no despacho do Hub: toda ferramenta do Hub que muda alguma coisa passa por cartão de aprovação no ponto em
//! que o Hub recebe a chamada, quando o motor não tem cartão provado (hoje Kilo e Cline).
//!
//! Vale por negação: só o que está na lista de leitura abaixo dispensa o cartão, então uma ferramenta nova, ou de nome
//! que a lista não conhece, pede cartão até alguém classificá-la. Não depende de configuração do motor: o motor pode
//! liberar o que quiser do lado dele, o Hub recusa a chamada que não foi aprovada. Fica de fora a ferramenta de tela do
//! computador (`computerCall`), que já exige o cartão de acesso à tela do próprio Hub antes de qualquer ação.
use crate::{agent::bot::Cmd, hub::Hub};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::oneshot;

/// Quanto a chamada espera a resposta no cartão. As pontes MCP esperam mais que isso (`mcp.rs`).
pub(crate) const ESPERA: Duration = Duration::from_secs(100);
/// O cartão mostra só o começo dos argumentos.
const DETALHE_MAX: usize = 1200;

/// Motores cujo cartão de aprovação o Hub não consegue provar: o Hub não confia no que o motor decide por conta própria.
pub(crate) fn sem_cartao_provado(backend: &str) -> bool {
    matches!(backend, "kilo" | "cline")
}

/// O método de chamada que este porteiro cobre.
pub(crate) fn cobre(metodo: &str) -> bool {
    servidor(metodo).is_some()
}

/// Motores que o Hub marca como «age no computador sem pedir aprovação» (`hello.backends`): ele não consegue impor nem
/// provar o cartão para o que o motor faz sozinho (editar arquivo, rodar comando). A tela avisa antes de a pessoa
/// escolher o motor; não há bloqueio.
pub(crate) fn age_sem_pedir(backend: &str) -> bool {
    backend == "kilo"
}

/// O servidor MCP do Hub que atende o método de chamada; `None` para o que este porteiro não cobre.
fn servidor(metodo: &str) -> Option<&'static str> {
    Some(match metodo {
        "chatCall" => "chat",
        "memoryCall" => "memory",
        "manageCall" => "manage",
        "teamCall" => "team",
        "taskCall" => "tasks",
        "routineCall" => "routines",
        "computersCall" => "computers",
        "connectorCall" => "connectors",
        _ => return None,
    })
}

/// As únicas ferramentas do Hub que dispensam o cartão: leitura, e o que só fala com o próprio dono (a mensagem, a
/// reação, a pergunta e o cartão de escolha da conversa) ou registra o resultado da própria execução da rotina.
pub(crate) fn so_leitura(metodo: &str, nome: &str) -> bool {
    matches!(
        (metodo, nome),
        ("memoryCall", "search_memory" | "read_memory" | "search_history" | "open_history" | "browser_status")
            | ("teamCall", "list_bots" | "my_team_chain")
            | ("manageCall", "list_team")
            | ("routineCall", "list_routines" | "report_routine_result")
            | ("taskCall", "task_read")
            | ("connectorCall", "list_connectors" | "list_logins")
            | ("computersCall", "list_computers" | "computer_job_status")
            | ("chatCall", "send_message" | "react" | "ask_owner" | "mostrar_componente")
    )
}

/// O motor que roda o turno do bot agora: o da conversa em andamento, que pode ser outro que o do cadastro.
pub(crate) fn motor_do_turno(hub: &Hub, bot: &str) -> Result<String> {
    let guardado = hub.store.bot(bot)?.filter(|row| !row.deleted).context("unknown bot")?.config;
    let runtime = hub.runtime(bot);
    let conversa = runtime.lane.as_ref().map_or(bot, |lane| lane.chat.as_str());
    Ok(crate::fork::spaces::execution_config(&hub.store, &guardado, conversa)?.backend)
}

/// Exige o cartão de uma chamada de ferramenta do Hub. `Ok` só com a resposta «permitir» do dono, ainda com a execução
/// viva; sem resposta no prazo, recusa, ou execução vencida, é erro e nada acontece.
pub(crate) async fn exigir(
    hub: &Hub,
    bot: &str,
    metodo: &str,
    nome: &str,
    argumentos: &Value,
    execucao: &crate::fork::sessoes::ExecutionRef,
) -> Result<()> {
    let Some(servidor) = servidor(metodo) else { return Ok(()) };
    if so_leitura(metodo, nome) || !sem_cartao_provado(&motor_do_turno(hub, bot)?) {
        return Ok(());
    }
    pedir(hub, bot, &format!("{servidor}: {nome}"), argumentos, Some(execucao)).await
}

/// O cartão em si: título com o servidor e a ferramenta, detalhe com o começo dos argumentos.
pub(crate) async fn pedir(
    hub: &Hub,
    bot: &str,
    titulo: &str,
    argumentos: &Value,
    execucao: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<()> {
    let Some(execucao) = execucao else { bail!("chamada vencida: execução ausente") };
    crate::fork::sessoes::bindings::recheck(hub, execucao)?;
    let titulo = format!("Permitir {titulo}");
    let detalhe: String = argumentos.to_string().chars().take(DETALHE_MAX).collect();
    let (reply, resposta) = oneshot::channel();
    hub.send_cmd(bot, Cmd::Approve { title: titulo, detail: detalhe, execution: Some(execucao.clone()), reply })?;
    let resposta = tokio::time::timeout(ESPERA, resposta).await;
    crate::fork::sessoes::bindings::recheck(hub, execucao)?;
    match resposta {
        Ok(Ok(true)) => Ok(()),
        Ok(_) => bail!("o dono não liberou esta ação; nada foi feito"),
        Err(_) => bail!("o dono não respondeu ao cartão a tempo; nada foi feito"),
    }
}

/// Ferramenta de conector (servidor MCP de fora) numa chamada `tools/call` que o Hub encaminha: cartão uma vez por
/// pedido (`beginRequest`), antes de qualquer encaminhamento, quando o motor não tem cartão provado. `false` recusa.
pub(crate) async fn conector_liberado(
    hub: &Hub,
    bot: &str,
    conector: &str,
    corpo: &Value,
    execucao: Option<&crate::fork::sessoes::ExecutionRef>,
) -> bool {
    if corpo["beginRequest"] != true {
        return true;
    }
    match motor_do_turno(hub, bot) {
        Ok(motor) if !sem_cartao_provado(&motor) => return true,
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "porteiro: motor do turno ilegível; chamada de conector recusada");
            return false;
        }
    }
    let nome = corpo["params"]["name"].as_str().unwrap_or("?");
    let resposta =
        pedir(hub, bot, &format!("conector {conector}: {nome}"), &corpo["params"]["arguments"], execucao).await;
    resposta.is_ok()
}

/// Caminho curado, que não espera resposta: motor sem cartão provado não chama ferramenta de conector por ele.
pub(crate) fn conector_curado_liberado(hub: &Hub, bot: &str) -> bool {
    motor_do_turno(hub, bot).is_ok_and(|motor| !sem_cartao_provado(&motor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nomes(lista: &Value) -> Vec<String> {
        lista.as_array().unwrap().iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect()
    }

    /// Cada nome da lista de leitura existe de verdade no servidor dele: renomear ou tirar uma ferramenta sem atualizar a
    /// lista quebra aqui, e uma ferramenta nova não classificada pede cartão.
    #[test]
    fn a_lista_de_leitura_so_tem_ferramentas_que_existem() {
        let existentes = |metodo: &str| -> Vec<String> {
            let mut lista = match metodo {
                "memoryCall" => nomes(&crate::chat::memory::tool_list()),
                "teamCall" => nomes(&crate::chat::team::tools()),
                "manageCall" => nomes(&crate::chat::manage::tools()),
                "routineCall" => nomes(&crate::routines::tools_for(None)),
                "taskCall" => nomes(&crate::fork::tarefas::tools()),
                "connectorCall" => nomes(&crate::market::requests::tools()),
                "computersCall" => nomes(&crate::computers::tools::tools()),
                _ => nomes(&crate::chat::outbox::tools()),
            };
            match metodo {
                "memoryCall" => {
                    lista
                        .extend(["read_memory", "search_history", "open_history", "browser_status"].map(str::to_owned));
                }
                "routineCall" => lista.push(crate::fork::rotina_resultado::TOOL.to_owned()),
                "chatCall" => lista.extend([
                    crate::chat::reactions::TOOL.to_owned(),
                    crate::fork::owner_question::TOOL.to_owned(),
                    crate::fork::genui::flow::TOOL.to_owned(),
                ]),
                _ => {}
            }
            lista
        };
        for (metodo, nome) in [
            ("memoryCall", "search_memory"),
            ("memoryCall", "read_memory"),
            ("memoryCall", "search_history"),
            ("memoryCall", "open_history"),
            ("memoryCall", "browser_status"),
            ("teamCall", "list_bots"),
            ("teamCall", "my_team_chain"),
            ("manageCall", "list_team"),
            ("routineCall", "list_routines"),
            ("routineCall", "report_routine_result"),
            ("taskCall", "task_read"),
            ("connectorCall", "list_connectors"),
            ("connectorCall", "list_logins"),
            ("computersCall", "list_computers"),
            ("computersCall", "computer_job_status"),
            ("chatCall", "send_message"),
            ("chatCall", "react"),
            ("chatCall", "ask_owner"),
            ("chatCall", "mostrar_componente"),
        ] {
            assert!(so_leitura(metodo, nome), "{metodo}/{nome} deveria dispensar o cartão");
            assert!(existentes(metodo).iter().any(|n| n == nome), "{metodo}/{nome} não existe mais no servidor");
        }
    }

    #[test]
    fn o_que_muda_algo_ou_nao_e_conhecido_pede_cartao() {
        for (metodo, nome) in [
            ("memoryCall", "remember"),
            ("memoryCall", "edit_memory"),
            ("manageCall", "create_bot"),
            ("manageCall", "update_bot"),
            ("teamCall", "ask_bot"),
            ("teamCall", "message_bot"),
            ("routineCall", "save_routine"),
            ("routineCall", "delete_routine"),
            ("taskCall", "task_update"),
            ("connectorCall", "request_secret"),
            ("computersCall", "run_command"),
            ("chatCall", "send_file"),
            ("chatCall", "set_instructions"),
            ("memoryCall", "ferramenta_nova_que_ninguem_classificou"),
        ] {
            assert!(!so_leitura(metodo, nome), "{metodo}/{nome} precisa de cartão");
        }
        assert!(!so_leitura("memoryCall", "list_bots"), "o nome vale só no servidor dele");
    }

    #[test]
    fn so_kilo_e_cline_ficam_sem_cartao_provado() {
        assert!(sem_cartao_provado("kilo") && sem_cartao_provado("cline"));
        assert!(!sem_cartao_provado("claude") && !sem_cartao_provado("codex"));
    }

    #[test]
    fn a_tela_do_computador_fica_fora_deste_porteiro() {
        assert!(servidor("computerCall").is_none() && servidor("computersCall").is_some());
    }
}
