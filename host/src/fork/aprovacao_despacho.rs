//! Porteiro no despacho do Hub: toda chamada que o motor faz ao Hub pela ponte de ferramentas passa por cartão de
//! aprovação no ponto em que o Hub a recebe, quando o motor do turno não tem cartão provado (hoje Kilo e Cline).
//!
//! Vale por negação, em três níveis: só o que está nas listas de [`leitura`] e [`excecao`] dispensa o cartão; uma
//! ferramenta nova, um método novo ou um transporte novo da ponte pedem cartão (ou são recusados, se a chamada não traz
//! execução para o cartão) até alguém classificá-los. O motor do turno é o que o Hub gravou no vínculo da sessão quando
//! o ator a ativou, nunca o cadastro do bot; sem esse registro, pede cartão. Não depende de configuração do motor.
use crate::{LockExt, agent::bot::Cmd, hub::Hub};
use anyhow::{Result, bail, ensure};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::oneshot;

/// Quanto a chamada espera a resposta no cartão. As pontes MCP esperam mais que isso (`mcp.rs`).
pub(crate) const ESPERA: Duration = Duration::from_secs(100);
/// O cartão mostra os argumentos por inteiro ou a chamada é recusada: prévia truncada não prova o efeito.
const DETALHE_MAX: usize = 1200;

/// A ferramenta do Hub que o cartão autoriza, com os argumentos reais, para as regras da pessoa avaliarem.
pub(crate) struct FerramentaDoHub {
    pub server: String,
    pub name: String,
    pub arguments: Value,
}

/// Motores cujo cartão de aprovação o Hub não consegue provar: o Hub não confia no que o motor decide por conta própria.
pub(crate) fn sem_cartao_provado(backend: &str) -> bool {
    matches!(backend, "kilo" | "cline")
}

/// Motores que o Hub marca como «age no computador sem pedir aprovação» (`hello.backends`): ele não prova o cartão para
/// o que o motor faz sozinho (editar arquivo, rodar comando). A tela avisa antes de a pessoa escolher o motor; não há
/// bloqueio. Hoje são os mesmos que não têm cartão provado.
pub(crate) fn age_sem_pedir(backend: &str) -> bool {
    sem_cartao_provado(backend)
}

/// Conector externo entrega ao motor credencial, destino ou capacidade de execução que o Hub não consegue prender a um
/// cartão por pedido: motor sem cartão provado não recebe nenhum conector, nem pelo caminho legado nem pelo curado. Os
/// servidores do próprio Hub seguem (e passam pelo porteiro).
pub(crate) fn sem_conectores(backend: &str, oferta: &mut crate::fork::spaces::tool_servers::Servers<'_>) {
    if sem_cartao_provado(backend) {
        oferta.connectors = &[];
        oferta.curated = false;
    }
}

/// O nome do servidor MCP do Hub que atende o método de chamada; um método desconhecido usa o próprio nome.
fn servidor(metodo: &str) -> &str {
    match metodo {
        "chatCall" => "chat",
        "memoryCall" => "memory",
        "manageCall" => "manage",
        "teamCall" => "team",
        "taskCall" => "tasks",
        "routineCall" => "routines",
        "computersCall" => "computers",
        "connectorCall" => "connectors",
        outro => outro,
    }
}

/// Leitura: devolve dados ao motor e não grava nada. Memória, histórico e metadados de login voltam ao motor dentro da
/// autorização que ele já tem; nenhuma destas leituras envia nada a terceiros.
fn leitura(metodo: &str, nome: &str) -> bool {
    matches!(
        (metodo, nome),
        (
            "memoryCall",
            "search_memory" | "read_memory" | "search_history" | "open_history" | "browser_status" | "__tools"
        ) | ("teamCall", "list_bots" | "my_team_chain")
            | ("manageCall", "list_team")
            | ("routineCall", "list_routines")
            | ("taskCall", "task_read")
            | ("connectorCall", "list_connectors" | "list_logins")
            | ("computersCall", "list_computers" | "computer_job_status")
            | ("computerTools", _)
    )
}

/// Exceções que gravam, mas com efeito e alcance conhecidos e já cercados pelo próprio Hub:
/// - `send_message`: grava uma mensagem do bot na conversa da sessão de origem, para a pessoa dona dela;
/// - `react`: grava uma reação na conversa de origem (em canal, na mensagem escolhida);
/// - `ask_owner`: abre uma pergunta pendente para a pessoa dona, na conversa de origem;
/// - `mostrar_componente`: publica um cartão de escolha na conversa capturada;
/// - `report_routine_result`: registra a declaração do bot sobre o resultado da própria execução da rotina;
/// - `computerCall` (qualquer ferramenta): controle de tela do próprio Hub, que exige a concessão de tela do computador
///   antes de qualquer leitura ou ação; não gera um cartão a cada chamada.
fn excecao(metodo: &str, nome: &str) -> bool {
    matches!(
        (metodo, nome),
        ("chatCall", "send_message" | "react" | "ask_owner" | "mostrar_componente")
            | ("routineCall", "report_routine_result")
            | ("computerCall", _)
    )
}

/// Exige o cartão de uma chamada do motor ao Hub. `sessao` é o vínculo da credencial da ponte (a identidade do
/// chamador); `execucao` é a execução capturada pela API para esta chamada, se o método captura uma. `Ok` só com a
/// resposta «permitir» da pessoa, ainda no mesmo turno; sem resposta no prazo, recusa, turno trocado, execução ausente ou
/// de outro vínculo, é erro e nada acontece.
pub(crate) async fn exigir(
    hub: &Hub,
    bot: &str,
    sessao: &str,
    metodo: &str,
    nome: &str,
    argumentos: &Value,
    execucao: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<()> {
    if leitura(metodo, nome) || excecao(metodo, nome) {
        return Ok(());
    }
    let motor = crate::fork::sessoes::bindings::backend(&hub.store.db.locked(), sessao)?;
    if motor.as_deref().is_some_and(|motor| !sem_cartao_provado(motor)) {
        return Ok(());
    }
    let nome = if nome.is_empty() { metodo } else { nome };
    ensure!(
        execucao.is_some_and(|execucao| execucao.acp_binding() == Some(sessao)),
        "chamada vencida: a execução não é a da sessão desta ponte"
    );
    pedir(hub, bot, servidor(metodo), nome, argumentos, execucao).await
}

/// O cartão em si: título com o servidor e a ferramenta, detalhe com os argumentos por inteiro.
async fn pedir(
    hub: &Hub,
    bot: &str,
    servidor: &str,
    nome: &str,
    argumentos: &Value,
    execucao: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<()> {
    let Some(execucao) = execucao else { bail!("chamada vencida: execução ausente") };
    let detalhe = argumentos.to_string();
    ensure!(
        detalhe.encode_utf16().count() <= DETALHE_MAX,
        "os argumentos são grandes demais para o cartão mostrar por inteiro; nada foi feito"
    );
    crate::fork::sessoes::bindings::recheck_turn(hub, execucao)?;
    let (reply, resposta) = oneshot::channel();
    hub.send_cmd(
        bot,
        Cmd::Approve {
            title: format!("Permitir {servidor}: {nome}"),
            detail: detalhe,
            execution: Some(execucao.clone()),
            identity: Some(FerramentaDoHub {
                server: servidor.to_owned(),
                name: nome.to_owned(),
                arguments: argumentos.clone(),
            }),
            reply,
        },
    )?;
    let resposta = tokio::time::timeout(ESPERA, resposta).await;
    crate::fork::sessoes::bindings::recheck_turn(hub, execucao)?;
    match resposta {
        Ok(Ok(true)) => Ok(()),
        Ok(_) => bail!("o dono não liberou esta ação; nada foi feito"),
        Err(_) => bail!("o dono não respondeu ao cartão a tempo; nada foi feito"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nomes(lista: &Value) -> Vec<String> {
        lista.as_array().unwrap().iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect()
    }

    /// Cada nome das duas listas existe de verdade no servidor dele: renomear ou tirar uma ferramenta sem atualizar a
    /// lista quebra aqui, e uma ferramenta nova não classificada pede cartão.
    #[test]
    fn as_listas_so_tem_ferramentas_que_existem() {
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
                    lista.extend(
                        ["read_memory", "search_history", "open_history", "browser_status", "__tools"]
                            .map(str::to_owned),
                    );
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
        for (metodo, nome, lista) in [
            ("memoryCall", "search_memory", "leitura"),
            ("memoryCall", "read_memory", "leitura"),
            ("memoryCall", "search_history", "leitura"),
            ("memoryCall", "open_history", "leitura"),
            ("memoryCall", "browser_status", "leitura"),
            ("memoryCall", "__tools", "leitura"),
            ("teamCall", "list_bots", "leitura"),
            ("teamCall", "my_team_chain", "leitura"),
            ("manageCall", "list_team", "leitura"),
            ("routineCall", "list_routines", "leitura"),
            ("taskCall", "task_read", "leitura"),
            ("connectorCall", "list_connectors", "leitura"),
            ("connectorCall", "list_logins", "leitura"),
            ("computersCall", "list_computers", "leitura"),
            ("computersCall", "computer_job_status", "leitura"),
            ("routineCall", "report_routine_result", "excecao"),
            ("chatCall", "send_message", "excecao"),
            ("chatCall", "react", "excecao"),
            ("chatCall", "ask_owner", "excecao"),
            ("chatCall", "mostrar_componente", "excecao"),
        ] {
            assert_eq!(leitura(metodo, nome), lista == "leitura", "{metodo}/{nome}");
            assert_eq!(excecao(metodo, nome), lista == "excecao", "{metodo}/{nome}");
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
            ("computersCall", "read_file"),
            ("chatCall", "send_file"),
            ("chatCall", "set_instructions"),
            ("memoryCall", "ferramenta_nova_que_ninguem_classificou"),
            ("metodoNovoDaPonte", "qualquer"),
            ("donnaGuardRuntime", ""),
            ("donnaConectorFrame", ""),
        ] {
            assert!(!leitura(metodo, nome) && !excecao(metodo, nome), "{metodo}/{nome} precisa de cartão");
        }
        assert!(!leitura("memoryCall", "list_bots"), "o nome vale só no servidor dele");
    }

    #[test]
    fn so_kilo_e_cline_ficam_sem_cartao_provado_e_so_eles_avisam() {
        for motor in ["kilo", "cline"] {
            assert!(sem_cartao_provado(motor) && age_sem_pedir(motor));
        }
        for motor in ["claude", "codex", "goose"] {
            assert!(!sem_cartao_provado(motor) && !age_sem_pedir(motor));
        }
    }

    #[test]
    fn motor_sem_cartao_provado_nao_recebe_nenhum_conector() {
        let ids = ["conector".to_owned()];
        for (motor, esperado) in [("kilo", false), ("cline", false), ("claude", true), ("codex", true)] {
            let mut oferta =
                crate::fork::spaces::tool_servers::Servers { builtin: vec!["memory"], curated: true, connectors: &ids };
            sem_conectores(motor, &mut oferta);
            assert_eq!(oferta.curated, esperado, "{motor}");
            assert_eq!(!oferta.connectors.is_empty(), esperado, "{motor}");
            assert_eq!(oferta.builtin, ["memory"], "{motor}: os servidores do Hub seguem");
        }
    }

    #[test]
    fn metodo_desconhecido_leva_o_proprio_nome_no_cartao() {
        assert_eq!(servidor("routineCall"), "routines");
        assert_eq!(servidor("metodoNovoDaPonte"), "metodoNovoDaPonte");
    }
}
