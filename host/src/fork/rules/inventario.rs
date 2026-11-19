//! I-132 declarado: nenhum item instala guarda ou demonstra interceptação.
use super::floor::Categoria;

pub const RUNTIME_ATIVO: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Estado {
    PlanejadoSemProva,
    ForaDaGarantia,
    AutoridadeHumanaPreservada,
}

pub struct Superficie {
    pub id: Aplicador,
    pub ponto: &'static str,
    pub motores: &'static str,
    pub estado: Estado,
    pub limite: &'static str,
}

// Cada variante tem uma descrição obrigatória. Retirar um aplicador usado no mapa
// quebra a compilação. Isto não descobre executores omitidos por conta própria.
macro_rules! superficies {
    ($( $id:ident => ($ponto:literal, $motores:literal, $estado:ident, $limite:literal) ),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Aplicador { $( $id, )+ }
        pub const SUPERFICIES: &[Superficie] = &[
            $(Superficie { id: Aplicador::$id, ponto: $ponto, motores: $motores,
                estado: Estado::$estado, limite: $limite },)+
        ];
    };
}

superficies! {
    McpDespacho => ("host/src/mcp.rs: Server, tools/call, call, POST autenticado", "Claude e Codex", PlanejadoSemProva,
        "Nove servidores internos; resolver identidade autenticada antes do executor, sem confiar no corpo."),
    Chat => ("host/src/api/bots.rs -> chat/outbox.rs e chat/profile.rs, antes do envio/mutação", "Claude e Codex", PlanejadoSemProva,
        "Servidor interno chat; ação derivada não comprova pedido humano."),
    Equipe => ("host/src/api/bots.rs -> chat/team.rs, call e delegação, antes da entrega", "Claude e Codex", PlanejadoSemProva,
        "Servidor interno team; outro bot e descendentes preservam origem derivada."),
    Gestao => ("host/src/api/bots.rs -> chat/manage.rs: call, criação/alteração e commit", "Claude e Codex", PlanejadoSemProva,
        "Servidor interno manage; conservar autoridade de gestor e revalidar antes da escrita transacional."),
    Memoria => ("host/src/mcp.rs -> api/bots.rs -> chat/memory/mod.rs -> chat/memory/tools.rs", "Claude e Codex", PlanejadoSemProva,
        "Servidor interno memory; memória não é categoria de chão. Nome externo igual não recebe exceção interna."),
    Rotinas => ("host/src/api/bots.rs -> routines/mod.rs: call, antes de salvar/ativar/remover/enqueue/credencial", "Claude e Codex", PlanejadoSemProva,
        "Servidor interno routines; manter user_turn, origem e pausa na admissão; pendências aguardam e reservas/recuperação podem continuar."),
    Webhook => ("host/src/api/http.rs -> routines/mod.rs: receive, antes de enqueue", "origem externa", PlanejadoSemProva,
        "Webhook não é dono; novas admissões recusadas durante pausa não ficam guardadas."),
    PausaGlobal => ("host/src/api/bots.rs: routinesPause/setRoutinesPaused -> routines/pause.rs sob team_lock", "Local/Control humano", AutoridadeHumanaPreservada,
        "Consulta e alteração humana durável; sem ferramenta MCP de bot nem cartão genérico novo. Retomada recalcula agenda existente."),
    Conectores => ("host/src/api/marketplace.rs -> market/requests.rs: request e conclusão, antes de conexão/login", "Claude e Codex", PlanejadoSemProva,
        "Servidor interno connectors; pedido de conexão/login não é executor genérico MCP; preservar cofre e conta."),
    ComputadorLocal => ("host/src/api/host.rs -> screen/computer.rs: call -> screen/cua.rs, antes de escrita no driver", "Claude e Codex", PlanejadoSemProva,
        "Servidor interno computer; clique opaco não comprova consequência. Guarda futura exige ação/alvo vinculáveis."),
    PreencherLogin => ("host/src/screen/computer.rs: type_login, antes de enviar credencial ao driver", "Claude e Codex", PlanejadoSemProva,
        "Preservar cerimônia local; login não pede biometria por padrão. Touch ID escolhido no item continua exigido."),
    ComputadoresDono => ("host/src/mcp.rs: computersCall -> computers/tools.rs: call, validação do alvo, criação de job e futura entrega", "Claude e Codex", PlanejadoSemProva,
        "Servidor interno computers; D83 parcial. Criar job não prova execução; helper, Touch ID e cartão de rotina não entregues."),
    ProcessoStdio => ("host/src/agent/bot/session.rs -> guard_hub/mcp/mod.rs/contexto.rs -> stdio.rs: serve, antes de Command::spawn", "Claude e Codex", PlanejadoSemProva,
        "Proxy do Host mediado; inicialização também pode ter efeito. Não garantir efeitos autônomos internos do processo."),
    RequisicaoStdio => ("host/src/guard_hub/mcp/stdio.rs: request, antes de escrever frame ao processo", "Claude e Codex", PlanejadoSemProva,
        "Vincular bot/conector/lane/run e ação/alvo. entrada.rs observa retorno depois do envio, não guarda pré-efeito."),
    Http => ("host/src/market/mod.rs -> mcp/remote.rs: forward, antes de client.request/send", "Claude e Codex", PlanejadoSemProva,
        "Proxy do Host mediado; contexto remoto precisa ser ligado. Método desconhecido, repetição 401/404 e redireção exigem nova revalidação."),
    AbrirSse => ("host/src/mcp/remote.rs: open_legacy, antes do GET de abertura", "Claude e Codex", PlanejadoSemProva,
        "Fallback SSE é caminho mediado; abertura também precisa de avaliação, identidade e cancelamento."),
    PostSse => ("host/src/mcp/remote.rs: post_legacy, antes do POST", "Claude e Codex", PlanejadoSemProva,
        "Fallback não contorna política; nunca repetir efeito incerto por inferência."),
    AcpClaude => ("host/src/agent/bot/updates.rs: pedido -> cartão -> resposta, antes de acp.respond", "Claude", PlanejadoSemProva,
        "Só pedidos efetivos; notificações podem chegar após efeito. Capturar antes do cartão e revalidar antes de consumir pendência/responder."),
    AcpCodex => ("host/src/agent/bot/updates.rs: pedido -> cartão -> resposta, antes de acp.respond", "Codex", PlanejadoSemProva,
        "D197 futuro: livre em Liberar/Acesso total; chão só no Hub. PR2A não muda configuração nem nível efetivo."),
    FerramentaInterna => ("host/src/agent/bot/updates.rs: resposta, antes de reply.send e novamente antes do efeito interno", "Claude e Codex", PlanejadoSemProva,
        "Cartão não transforma Recusar em Permitir; revalidar mesma ação, alvo, autoridade, sessão e revisão depois de await."),
    Cofre => ("host/src/api/mod.rs: despacho, antes de desbloqueio do cofre", "autoridade específica", PlanejadoSemProva,
        "Manter cerimônias e autoridade próprias; nenhuma autorização ampla do bot."),
    ChromeDono => ("contrato D68: Chrome nativo do Mac; D83: computadores do dono", "confirmação local", PlanejadoSemProva,
        "Em Liberar, enviar/pagar/apagar perguntam. Login sem biometria por padrão; Touch ID ligado no item exige também em Liberar. Só inventário."),
    NativoClaude => ("efeitos nativos Claude sem pedido ou proxy do Host", "Claude", ForaDaGarantia,
        "Não há ponto de interceptação comprovado; não chamar todo efeito fora do Hub de Codex."),
    NativoCodex => ("efeitos nativos Codex sem pedido ou proxy do Host", "Codex", ForaDaGarantia,
        "D197: não prometer chão fora do Hub nem bloquear Codex genericamente por ausência de mediação."),
}

pub const CATEGORIAS: &[Categoria] = &[
    Categoria::Dinheiro,
    Categoria::Destruicao,
    Categoria::Elevacao,
    Categoria::Segredos,
    Categoria::PastaHubMotores,
    Categoria::ApiHubMotores,
    Categoria::ConfiguracaoHubMotores,
    Categoria::ExecucaoFutura,
    Categoria::ConfirmacaoLocal,
    Categoria::Desconhecida,
];

/// Mapa categoria -> pontos propostos. Match exaustivo exige tratar categoria nova.
/// Os proxies permanecem dentro da mediação do Host, inclusive HTTP/SSE/processo.
pub fn aplicadores(categoria: Categoria) -> Vec<Aplicador> {
    use Aplicador::{
        AbrirSse, AcpClaude, AcpCodex, Chat, ChromeDono, Cofre, ComputadorLocal, ComputadoresDono, Conectores, Equipe,
        FerramentaInterna, Gestao, Http, McpDespacho, Memoria, PostSse, PreencherLogin, ProcessoStdio, RequisicaoStdio,
        Rotinas, Webhook,
    };
    let mut pontos = vec![
        McpDespacho,
        ProcessoStdio,
        RequisicaoStdio,
        Http,
        AbrirSse,
        PostSse,
        AcpClaude,
        AcpCodex,
        FerramentaInterna,
    ];
    let especificos: &[Aplicador] = match categoria {
        Categoria::Dinheiro => &[ComputadorLocal, ComputadoresDono, ChromeDono],
        Categoria::Destruicao => &[Rotinas, ComputadorLocal, ComputadoresDono, ChromeDono],
        Categoria::Elevacao | Categoria::ExecucaoFutura => &[ComputadorLocal, ComputadoresDono],
        Categoria::Segredos => &[Cofre, Conectores, PreencherLogin, ComputadorLocal, ComputadoresDono, Rotinas],
        Categoria::PastaHubMotores => &[Gestao, ComputadorLocal, ComputadoresDono],
        Categoria::ApiHubMotores => {
            &[Chat, Equipe, Gestao, Memoria, Rotinas, Conectores, ComputadorLocal, ComputadoresDono]
        }
        Categoria::ConfiguracaoHubMotores => &[Gestao, Rotinas, ComputadorLocal, ComputadoresDono],
        Categoria::ConfirmacaoLocal => &[ChromeDono, ComputadorLocal, ComputadoresDono, PreencherLogin],
        Categoria::Desconhecida => &[
            Chat,
            Equipe,
            Gestao,
            Memoria,
            Rotinas,
            Webhook,
            Conectores,
            ComputadorLocal,
            ComputadoresDono,
            PreencherLogin,
            Cofre,
            ChromeDono,
        ],
    };
    pontos.extend_from_slice(especificos);
    pontos
}

/// Completude estrutural, não scanner de código nem teste de integração.
pub fn verificar(categorias: &[Categoria], superficies: &[Superficie]) -> Result<(), &'static str> {
    if categorias != CATEGORIAS {
        return Err("categorias ausentes, duplicadas ou fora do inventário declarado");
    }
    if superficies.len() != SUPERFICIES.len()
        || SUPERFICIES.iter().any(|esperada| superficies.iter().filter(|s| s.id == esperada.id).count() != 1)
    {
        return Err("superfície ausente ou duplicada");
    }
    for categoria in categorias {
        let pontos = aplicadores(*categoria);
        if pontos.is_empty() || pontos.iter().any(|id| !superficies.iter().any(|s| s.id == *id)) {
            return Err("categoria sem aplicador declarado");
        }
    }
    Ok(())
}
