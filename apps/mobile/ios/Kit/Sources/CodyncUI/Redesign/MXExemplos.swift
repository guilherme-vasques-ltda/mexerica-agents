// DADOS DE EXEMPLO — NÃO SÃO DO HUB.
//
// Copiados do desenho aprovado (`brand/apps/ios-v1-design/telas/Conversas.dc.html`, repositório `mexerica`)
// só para a casca do redesenho rodar sem login. Nada aqui chama o Hub nem imita a API dele: a ligação com
// os dados reais é a etapa 2. Não use estes valores fora da prévia de Debug.
import CodyncKit
import Foundation

public enum MXExemplo {
    public static let berga = MXBergaResumo(
        id: "berga", nome: "Berga", space: "Pessoal", rosto: .conjunto("berga"), estado: .livre,
        estadoTexto: "Livre", mensagem: "Bom dia! Byte e Prosa esperam você."
    )

    public static let space = "Pessoal"

    public static let canais: [MXCanalResumo] = [
        MXCanalResumo(nome: "lançamento", naoLidas: 3, sessoesRodando: 2, sessoesEsperando: 1),
        MXCanalResumo(nome: "financeiro", naoLidas: 0, sessoesRodando: 1, sessoesEsperando: 0),
        MXCanalResumo(nome: "vendas", naoLidas: 0, sessoesRodando: 0, sessoesEsperando: 0),
    ]

    public static let agentes: [MXAgenteResumo] = [
        MXAgenteResumo(id: "byte", nome: "Byte", funcao: "Dev", rosto: .conjunto("developer"), estado: .precisaResposta,
                       hora: "09:12", previa: "Precisa da sua aprovação", tom: .alerta,
                       sessoesRodando: 1, sessoesEsperando: 1, naoLidas: 1),
        MXAgenteResumo(id: "prosa", nome: "Prosa", funcao: "Roteiros", rosto: .conjunto("assistente"), estado: .precisaResposta,
                       hora: "08:47", previa: "Tem uma pergunta para você", tom: .alerta,
                       sessoesRodando: 0, sessoesEsperando: 1, naoLidas: 1),
        MXAgenteResumo(id: "crivo", nome: "Crivo", funcao: "Pesquisa", rosto: .conjunto("pesquisa"), estado: .trabalhando,
                       hora: "09:05", previa: "Escrevendo…", tom: .ativo,
                       sessoesRodando: 1, sessoesEsperando: 0, naoLidas: 0),
        MXAgenteResumo(id: "trato", nome: "Trato", funcao: "Vendas", rosto: .conjunto("vendas"), estado: .livre,
                       hora: "Ontem", previa: "Mandei a proposta para o fornecedor.", tom: .normal,
                       sessoesRodando: 0, sessoesEsperando: 0, naoLidas: 0),
        MXAgenteResumo(id: "saldo", nome: "Saldo", funcao: "Financeiro", rosto: .conjunto("financeiro"), estado: .dormindo,
                       hora: "8 nov", previa: "Você: Quero uma rotina que confira os repasses", tom: .normal,
                       sessoesRodando: 0, sessoesEsperando: 0, naoLidas: 0),
        MXAgenteResumo(id: "vitrine", nome: "Vitrine", funcao: "E-commerce", rosto: .conjunto("marketing"), estado: .dormindo,
                       hora: "28 out", previa: "As novidades da semana na Amazon chegaram.", tom: .normal,
                       sessoesRodando: 0, sessoesEsperando: 0, naoLidas: 0),
    ]

    /// Contadores laranja da barra de abas, como no desenho.
    public static let pendencias: [MXAba: Int] = [.missionBoard: 2, .conversas: 2, .equipes: 1, .rotinas: 2]
    /// Só para conferência na tela (Debug, `-MexericaRedesignContadorAlto`): contadores de 1 a 3 algarismos.
    public static let pendenciasAltas: [MXAba: Int] = [.missionBoard: 12, .conversas: 150, .equipes: 9, .rotinas: 1]
}

// DADOS DE EXEMPLO DA CONVERSA — NÃO SÃO DO HUB.
// Copiados de `telas/Chat.dc.html` (05, 05a, 05b) e `telas/ChatAprovar.dc.html` (06). Só a `MXFonteExemplo` usa.
public extension MXExemplo {
    /// Fila de 05: duas mensagens. As cenas 05a e 05b têm três (a terceira não aparece no desenho; texto nosso).
    static let fila: [MXItemDaFila] = [
        MXItemDaFila(id: "fila-1", texto: "Depois roda o lint também", revisao: 1),
        MXItemDaFila(id: "fila-2", texto: "E atualiza o README com o weeklyTotals", revisao: 1),
    ]
    static let terceiraDaFila = MXItemDaFila(id: "fila-3", texto: "Depois abre um PR de rascunho", revisao: 1)

    /// A conversa de 05 até a resposta com passos (sem o balão de quem trabalha).
    static func historico(autor: String) -> [MXItemDaConversa] {
        [
            MXItemDaConversa(id: "ex-1", .agente(MXMensagemDoAgente(
                autor: autor, hora: nil, cabecalho: false,
                texto: "Ontem terminei a revisão do módulo de ritmo. Ficaram três pontos: o cálculo de semana na virada do ano, a falta de teste para datas inválidas e o formato do tempo total, que hoje mostra segundos demais. Posso cuidar dos três hoje, começando pelo cálculo de semana."))),
            MXItemDaConversa(id: "ex-2", .usuario(MXMensagemDoUsuario(texto: "Pode seguir. Começa pelo total por semana."))),
            MXItemDaConversa(id: "ex-3", .separador("Hoje 19:22")),
            MXItemDaConversa(id: "ex-4", .usuario(MXMensagemDoUsuario(texto: "Cria um helper weeklyTotals(runs) que soma distância e tempo por semana, com teste."))),
            MXItemDaConversa(id: "ex-5", .agente(MXMensagemDoAgente(
                autor: autor, hora: "19:25", cabecalho: true,
                texto: "Adicionei `weeklyTotals(runs)` em `src/pace.js`. Agrupa por semana ISO e soma distância e tempo. Os 4 testes passam e nada foi commitado.",
                passos: MXPassos(resumo: "3 passos · Leitura, Edição, Comando · 2 min", itens: [
                    MXPasso(id: "p-1", texto: "Leu src/pace.js", concluido: true),
                    MXPasso(id: "p-2", texto: "Editou src/pace.js", concluido: true),
                    MXPasso(id: "p-3", texto: "Rodou npm test", concluido: true),
                ])))),
        ]
    }

    /// 05: trabalhando, com o gomo.
    static func trabalho(autor: String, atividade: String = "Adicionando a guarda de data inválida") -> MXItemDaConversa {
        MXItemDaConversa(id: "ex-trabalhando", .trabalhando(MXTrabalhoEmCurso(autor: autor, atividade: atividade)))
    }

    /// 06: o pedido de "Byte quer mudar arquivos".
    static let pedido = MXPedidoDeAprovacao(
        id: "ex-pedido", titulo: "Byte quer mudar arquivos", detalhe: MXTextoLongo("Edit src/pace.js"), acao: "Edição de arquivos",
        onde: "MacBook Pro · pace",
        opcoes: [
            MXOpcaoDeAprovacao(id: "allow_once", rotulo: "Permitir uma vez", permite: true, principal: true),
            MXOpcaoDeAprovacao(id: "allow_always", rotulo: "Sempre permitir", permite: true, principal: false),
            MXOpcaoDeAprovacao(id: "reject_once", rotulo: "Recusar", permite: false, principal: false),
        ],
        resultado: .pendente,
        detalhes: [],
        diffs: exemploDeDiff.map { [MXDiffPrevia($0, indice: 0)] } ?? []
    )

    /// A alteração do pedido de exemplo (o tipo do app não tem inicializador público: lido de JSON).
    static let exemploDeDiff: FileDiff? = try? JSONDecoder().decode(FileDiff.self, from: Data(#"""
        {"path":"src/pace.js","added":3,"removed":1,"isNew":false,"startLine":12,
         "patch":"@@ -12,4 +12,6 @@\n function isoWeek(date) {\n-  const year = date.getFullYear()\n+  const d = new Date(date)\n+  d.setDate(d.getDate() + 3 - ((d.getDay() + 6) % 7))\n+  const year = d.getFullYear()"}
        """#.utf8))

    /// 06: a conversa esperando a decisão.
    static func aprovar(autor: String) -> [MXItemDaConversa] {
        [
            MXItemDaConversa(id: "ap-1", .usuario(MXMensagemDoUsuario(texto: "Corrige o bug do isoWeek na virada do ano."))),
            MXItemDaConversa(id: "ap-pedido", .aprovacao(pedido)),
            MXItemDaConversa(id: "ap-trabalhando", .trabalhando(MXTrabalhoEmCurso(
                autor: autor, atividade: "",
                passos: MXPassos(resumo: "Trabalhando · 1 passo", itens: [MXPasso(id: "ap-p1", texto: "Leu src/pace.js", concluido: true)]),
                esperandoDecisao: true))),
        ]
    }

    /// Função longa do menu do agente (05b: "Desenvolvedor"); as outras repetem a etiqueta da lista.
    static func funcaoLonga(_ id: String) -> String { id == "byte" ? "Desenvolvedor" : (agentes.first { $0.id == id }?.funcao ?? "") }

    static let modelo = "Claude · Opus"
    static let contexto = 42
}

// DADOS DE EXEMPLO DAS SESSÕES E DOS AJUSTES — NÃO SÃO DO HUB.
// Copiados de `telas/Sessoes.dc.html` (07) e `telas/ConfigBot.dc.html` (08). Só a `MXFonteExemplo` usa.
public extension MXExemplo {
    private static func s(_ id: String, _ nome: String, _ estado: MXEstadoDaSessao, _ hora: String,
                          principal: Bool = false, _ filhas: [MXSessao] = []) -> MXSessao {
        MXSessao(id: id, nome: nome, estado: estado, hora: hora, principal: principal, filhas: filhas)
    }

    /// As sessões de cada agente do desenho (Byte, Berga, Prosa, Crivo); os outros têm a Principal e duas.
    static func sessoes(_ agente: String) -> (sessoes: [MXSessao], arquivadas: Int) {
        switch agente {
        case "byte":
            return ([
                s("byte-principal", "Principal", .precisaDeVoce, "09:12", principal: true, [
                    s("byte-revisao", "Revisão dos testes", .trabalhando, "agora", [
                        s("byte-suite", "Suíte do checkout", .trabalhando, "agora", [
                            s("byte-cupom", "Teste instável do cupom", .precisaDeVoce, "09:10"),
                        ]),
                    ]),
                    s("byte-notas", "Notas da versão", .livre, "08:40"),
                ]),
                s("byte-guarda", "Guarda de data inválida", .trabalhando, "09:05", [
                    s("byte-fusos", "Pesquisa de fusos horários", .livre, "Ontem"),
                ]),
                s("byte-changelog", "Rascunho do changelog", .livre, "Ontem"),
                s("byte-benchmark", "Benchmark do weeklyTotals", .livre, "Seg", [
                    s("byte-comparar", "Comparar com a versão 2.3", .livre, "Seg"),
                ]),
                s("byte-google", "Corrigir login com Google", .livre, "Seg"),
                s("byte-migracao", "Migração do banco", .livre, "2 nov", [
                    s("byte-backup", "Backup antes da migração", .livre, "2 nov", [
                        s("byte-indices", "Conferir índices", .livre, "2 nov", [
                            s("byte-relatorio", "Relatório dos índices", .livre, "2 nov"),
                        ]),
                    ]),
                ]),
                s("byte-dependencias", "Atualizar dependências", .livre, "30 out"),
                s("byte-pagamentos", "Revisão do PR de pagamentos", .livre, "28 out"),
            ], 3)
        case "berga":
            return ([
                s("berga-principal", "Principal", .livre, "09:00", principal: true, [s("berga-resumo", "Resumo da manhã", .livre, "08:00")]),
                s("berga-semana", "Organizar a semana", .trabalhando, "agora", [
                    s("berga-saldo", "Pedir números ao Saldo", .trabalhando, "agora"),
                    s("berga-prosa", "Roteiros com a Prosa", .precisaDeVoce, "08:47"),
                ]),
                s("berga-viagem", "Viagem de dezembro", .livre, "Ontem", [
                    s("berga-passagens", "Passagens", .livre, "Ontem"), s("berga-hotel", "Hotel", .livre, "Ontem"),
                ]),
                s("berga-presente", "Presente de aniversário", .livre, "Seg"),
            ], 4)
        case "prosa":
            return ([
                s("prosa-principal", "Principal", .precisaDeVoce, "08:47", principal: true, [s("prosa-reels", "Roteiro do Reels 1", .livre, "08:30")]),
                s("prosa-novembro", "Roteiros de novembro", .livre, "Ontem"),
            ], 1)
        case "crivo":
            return ([
                s("crivo-principal", "Principal", .livre, "09:05", principal: true),
                s("crivo-avaliacoes", "Avaliações dos concorrentes", .trabalhando, "agora", [
                    s("crivo-coletar", "Coletar avaliações", .trabalhando, "agora"),
                ]),
            ], 0)
        default:
            return ([
                s("\(agente)-principal", "Principal", .livre, "Hoje", principal: true, [s("\(agente)-tarefa", "Tarefa da semana", .livre, "Ontem")]),
                s("\(agente)-pedido", "Pedido do líder", .livre, "Seg"),
            ], 0)
        }
    }

    /// Os valores da lista de ajustes do desenho (08).
    static let itensDeAjuste: [MXItemDeAjuste] = [
        MXItemDeAjuste(.identidade, letra: "I", nome: "Identidade", valor: "Nome, função e aparência"),
        MXItemDeAjuste(.motor, letra: "M", nome: "Motor", valor: "Claude · Opus"),
        MXItemDeAjuste(.comportamento, letra: "C", nome: "Comportamento", valor: "Direto, sem enrolar"),
        MXItemDeAjuste(.ferramentas, letra: "F", nome: "Ferramentas", valor: "4 conectores · 3 skills"),
        MXItemDeAjuste(.whatsapp, letra: "W", nome: "WhatsApp", valor: "Desligado"),
        MXItemDeAjuste(.memoria, letra: "R", nome: "Memória", valor: "12 lembranças"),
        MXItemDeAjuste(.voz, letra: "V", nome: "Voz", valor: "Calma · português"),
        MXItemDeAjuste(.rotinas, letra: "T", nome: "Rotinas", valor: "2 ativas"),
    ]
}
