# MCP na Mexerica

[Índice](README.md) · [Segurança](seguranca.md) · [Fontes MCP](fontes.md#mcp)

MCP conecta um aplicativo a ferramentas e dados usando JSON-RPC 2.0. O host MCP
coordena clientes que conversam com servidores; esse papel do padrão não deve
ser confundido com o nome do processo Rust da Mexerica. Este guia adota a referência
**2026-07-28** como conhecimento. Os servidores internos do Hub implementam
**2025-06-18**; as pontes também aceitam 2025-03-26 e 2024-11-05 no perfil legado
restrito. Protocolo próprio em Rust, sem SDK MCP oficial no runtime examinado.

## O essencial das duas revisões

| Tema | 2025-06-18 | 2026-07-28 |
| --- | --- | --- |
| Início | `initialize` negocia versão/capacidades; depois `notifications/initialized` | Sem esse handshake; cada pedido leva versão e capacidades em `params._meta`. `server/discover` informa versões, capacidades e identidade autodeclarada |
| Transporte | stdio; Streamable HTTP com POST, GET/SSE e sessões opcionais; HTTP+SSE anterior é legado | stdio continua; HTTP usa POST com JSON ou SSE por pedido, sem GET separado, sessão de protocolo, DELETE de sessão ou retomada por `Last-Event-ID` |
| Metadados HTTP | Cabeçalho `MCP-Protocol-Version` após negociação | Versão também no cabeçalho; `Mcp-Method`/`Mcp-Name` espelham o corpo e `x-mcp-header` tem regras próprias. Divergências precisam ser recusadas |
| Respostas e interação | Adiciona saída estruturada, links de recursos e elicitation; retira batch JSON-RPC | `resultType` distingue conclusão e entrada necessária. MRTR substitui pedidos reversos independentes; `subscriptions/listen` trata assinaturas |
| Autorização HTTP | MCP como recurso OAuth, discovery do recurso protegido e `resource` para o destinatário do token | Reforça vínculo ao issuer, callback e cadastro; cliente pré-registrado/CIMD e DCR de compatibilidade, com consentimento e PKCE |
| Capacidades | Somente o que cliente/servidor negociaram | JSON Schema 2020-12 por padrão, regras de cache e metadados por pedido; roots, sampling e logging são depreciados, não já removidos |

Referências: [mudanças de 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/changelog),
[versionamento moderno](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning) e
[transporte HTTP moderno](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http).
2025-11-25 ainda usa handshake/sessões e introduz recursos como elicitation por
URL e CIMD; é uma revisão intermediária, não sinônimo da era de 2026.

As primitivas continuam diferentes: **tools** expõem operações (`tools/list`,
`tools/call`); **resources** fornecem contexto por URI (`resources/read` e listas,
inclusive templates); **prompts** são modelos de mensagens (`prompts/get`).
Annotations não concedem acesso. Uma URI ou `$ref` não autoriza leitura de
arquivo nem busca arbitrária na rede.

Em [MRTR](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr),
`input_required` traz pedidos de entrada e estado opaco; a continuação usa novo
ID e respostas vinculadas ao pedido. Não significa operação concluída. Falha
de conexão ou cancelamento tampouco prova que um efeito externo foi desfeito;
resultado incerto exige reconciliação antes de repetir uma escrita. Cache/TTL
não prolongam autorização nem permitem cruzar contas ou contextos.

## Autenticação não é aprovação da ação

O perfil [OAuth HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
envolve descoberta do recurso e do servidor de autorização, emissor validado,
PKCE S256 com suporte conferido, state, redirect exato, escopos mínimos e token
destinado ao recurso correto. Tokens de entrada não podem ser reaproveitados
como credenciais de outra API (token passthrough). Um intermediário também não
pode usar sua concessão para atender um cliente sem o consentimento necessário.
Em stdio, o perfil OAuth do transporte não se aplica; iniciar o servidor executa
um programa e não cria isolamento nem autoriza herdar todo o ambiente do Hub.

Elicitation em formulário admite dados não secretos. O modo URL mantém a
interação sensível fora do modelo/cliente, exige consentimento e não autoriza
inspecionar página ou digitação. Login cliente→MCP, login MCP→terceiro e
[segredo por referência](seguranca.md#segredos-por-referência) são fluxos
distintos. O cofre e `type_login` atuais não demonstram suporte a elicitation URL.

## O que a Mexerica usa hoje

O motor recebe servidores internos e pontes stdio pelo caminho ACP. Para um
conector remoto, a ponte do Hub fala HTTP/SSE com o serviço; para um local,
inicia o comando configurado. Credenciais são resolvidas pelo Hub. A versão
numérica do ACP não é a revisão MCP. Compatibilidade precisa ser conferida nas
duas pontas, motor→ponte e ponte→serviço.

O cadastro manual, cofre/OAuth e seleção por bot já existem. Salvar não seleciona
bot nem comprova funcionamento. `verify` faz handshake e descoberta inicial,
podendo iniciar processo local; não comprova uma ação de negócio. A busca e
instalação pelo Registry genérico foram retiradas. O
catálogo de links oficiais,
Linear → Notion → GitHub, continua planejado; Google segue por conector MCP comum.
Isso não autoriza code mode ou cadastro próprio de app OAuth Google.

A mediação de retornos locais e remotos existe no Guard, com cobertura parcial:
rotula texto externo, redige credenciais reconhecidas e registra exposição.
Projeta schemas e conteúdo em subconjunto textual; retém binários não mediados,
omite `instructions` externas no initialize e não oferece sampling, elicitation
ou roots ao motor. Não equivale a aprovação universal antes do efeito, nem
cobre automaticamente shell e ferramentas nativas. O contrato atual
de mediação remota está em [marketplace](../../features/marketplace.md); planos
antigos que a chamam futura não anulam o código já existente.

**A data anunciada não é prova de conformidade.** Nenhuma ponta
ecoa ou coage versão: servidores internos respondem 2025-06-18 (contraproposta
para outra string, `-32602` sem versão textual); pontes e verify preservam a
versão válida devolvida pelo serviço e encerram a conexão, com erro fixo e depois
do recibo, quando ela é ausente ou desconhecida. Cada sessão HTTP envia só a sua
versão aceita no cabeçalho `MCP-Protocol-Version`, e o fallback legado ocorre só
com 404/405. As provas são fixtures locais deste perfil, não certificam todos os
fornecedores, a especificação inteira nem a era 2026-07-28. Regras em
[versões por ponta](../../features/marketplace.md).

## Extensões e catálogos distintos

[Skills over MCP](https://modelcontextprotocol.io/extensions/skills/overview)
usa `skills/list`, `skills/get` e `resources/read`, com identidade por origem e
URI e verificação de manifesto/conteúdo. Ler não ativa. A extensão está beta no
índice consultado; o SEP Final é outro objeto. Seus requisitos para hosts incluem
suportar até **512 arquivos/16 MiB por skill**; o catálogo local limita a
**100 arquivos/5 MiB**. São contratos distintos: o app não implementa a
extensão, não anuncia conformidade e não aumenta limites neste guia.

MCP Apps trata de UI; Tasks trata de operações assíncronas anunciadas por ambos
os lados. Nenhuma equivale a Rotinas do app. O Registry oficial
é catálogo de metadados, não atestado de segurança; `server/discover` consulta
um servidor escolhido, sem instalar nada. Nenhum substitui o catálogo de links oficiais.

## Lacunas planejadas

A versão legada honesta está implementada no código conforme acima; os demais itens seguem planejados, não
entregues.

| Item | Trabalho pendente |
| --- | --- |
| Versões legadas honestas | Implementado: interseção por ponta, sem eco/coerção, cabeçalho HTTP por sessão e incompatibilidade como erro técnico fatal |
| URLs OAuth e loopback | Usar parsing de URL e comparação exata de loopback; validar HTTPS, metadados, DNS e redirects com limites próprios, preservando AS legítimo em outra origem |
| Vínculo por tentativa OAuth | Fixar issuer, recurso, endpoints, contexto/geração e autoridade original no pending; conferir PKCE anunciado e callback `iss`; proteger commit/refresh contra resposta antiga |
| Escopos e cadastro | Perfis mínimos, escopos concedidos e consentimento incremental; registro prévio/CIMD/DCR e redirects por tipo de app, sem cadastrar serviço novo por inferência |
| Remoto moderno | Discovery, metadados, `resultType` e erros modernos na ponte existente; sem fallback enganoso ou novos poderes |
| Contrato ao motor | Provar versão, conteúdo, cache/cabeçalhos pertinentes e mediação nas duas pontas com o motor real; manter compatibilidade explícita quando necessária |
| Referências e envio final | Consumir os contratos de captura, envio final e validade pelos produtores canônicos, sem cofre/executor paralelo; separar entrada não secreta de interação sensível |

Esses trabalhos devem se integrar às frentes de Conectores existentes. Trocar a
constante ou adotar um SDK não resolve autoridade e compatibilidade por si só.
Novas dependências, instalações, extensões e execução exigem escopo próprio.
