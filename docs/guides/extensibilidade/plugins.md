# Plugins de IA e Extensões da Mexerica

> Agenda interna deixou de ser plugin. Seu cartão,
> interruptores e concessão por bot foram retirados; tarefas próprias usam
> [Rotinas](../../features/routines.md), inclusive pelo Berga, com criação própria autônoma, cota e cartão ao dono.
> As seções Scatty/Agenda abaixo registram o plano e a implementação históricos;
> não descrevem uma capacidade atual de calendário externo. O banco legado é preservado.


O e-mail embutido foi retirado; use os conectores MCP para operações de e-mail. As referências anteriores a esse consumidor são históricas.

[Índice](README.md) · [Segurança](seguranca.md) · [Fontes de plugins](fontes.md#plugins)

**Plugins de IA** são pacotes OpenAI/Claude de skills + MCP;
**Extensões da Mexerica** são funções próprias do produto. Os identificadores
técnicos existentes não mudam por essa nomenclatura.

Um plugin de motor embala skills, configuração MCP e, conforme o cliente,
outros componentes. Pode conter apenas skills ou apenas MCP. Este guia adota o
conhecimento dos plugins oficiais OpenAI; não instala esses pacotes na DONNA
Bot. Os dialetos OpenAI e Claude Code precisam ser identificados antes de
avaliar compatibilidade.

## Formatos oficiais

| Aspecto | OpenAI / Codex | Claude Code |
| --- | --- | --- |
| Manifesto | Agent Plugins portátil: `plugin.json` na raiz; `.codex-plugin/plugin.json` como compatibilidade | `.claude-plugin/plugin.json`; também há descoberta padrão sem manifesto |
| Skills e MCP | Portátil: `skills/` e `mcp.json`; layout Codex compatível pode usar `.mcp.json` | `skills/`, `.mcp.json` ou declarações do manifesto |
| Configuração específica | `extensions.com.openai` no manifesto portátil; quando presente como objeto, substitui o overlay inteiro, sem mesclar | Campos e variáveis próprios do Claude, sem tradução universal |
| Componentes adicionais | Assets, MCP/UI e hooks conforme suporte e distribuição; commands/agents do Claude não são portáveis automaticamente | Commands, agents, hooks, LSP e outros componentes conforme a versão do cliente |
| Marketplace | `.agents/plugins/marketplace.json`, além dos formatos compatíveis documentados | `.claude-plugin/marketplace.json` |
| Escopo | Cliente, fonte e política do workspace determinam disponibilidade | Usuário, projeto ou local; instalação pode habilitar componentes |

Fontes: [empacotamento OpenAI](https://developers.openai.com/plugins/build/plugins) e
[referência Claude Code](https://code.claude.com/docs/en/plugins-reference).
No portátil, paths relativos começam com `./` e ficam dentro da raiz. Não basta
renomear `.mcp.json`: o formato portátil também declara o tipo de transporte.
`.app.json` mapeia um MCP registrado; não representa o app Electron da DONNA.

O identificador deve ser estável e a versão explícita para distribuição. Um
manifesto válido, um upload aceito e uma publicação aprovada são etapas
diferentes. Conversão pode perder componentes: inventarie o que foi preservado,
adaptado ou não suportado, com origem e revisão. Não traduza nem reescreva o
original no lugar, nem retire um script necessário e anuncie a skill funcional.

Hooks podem executar código. Na referência OpenAI consultada, hooks de ciclo
de vida estão restritos à instalação manual no Codex desktop, fora do diretório
público. A confiança do Codex na definição exata do hook e a aprovação de uma
loja não concedem autoridade no Hub. No Claude, componentes iniciados pelo
plugin também podem executar com privilégios do usuário; a política do modelo
não isola automaticamente esses processos.

## Superfícies e dependências

ChatGPT, Codex desktop, CLI, IDE e adaptador ACP não possuem necessariamente
as mesmas capacidades. Os caminhos oficiais de instalação e publicação não
provam suporte no Hub. MCP local/stdio e distribuição pública têm requisitos
próprios de elegibilidade; não se pode prometer publicação aberta de qualquer
pacote local. Login e múltiplas contas dependem do serviço e do cliente.

UI pode usar MCP Apps e extensões específicas do ChatGPT; CSP/iframe não isolam
o servidor MCP. MCP Events está em draft no guia consultado e descreve caminhos
modernos e legados; o guia de auth ainda referencia 2025-11-25. Uma versão
mencionada em uma página não determina a versão de todos os componentes.
Consulte [MCP](mcp.md) e [fontes](fontes.md#plugins) por recurso. Nem eventos
assinados nem dependências em `agents/openai.yaml` criam pedido humano ou grant.

## Extensões da Mexerica: funções próprias

Na Mexerica, plugin é uma função opcional do produto, com configuração, contas,
recursos, concessões e ciclo de vida próprios. Agenda/Foco (Scatty) e E-mail são
consumidores desse contrato; não são sinônimos de pacotes OpenAI/Claude.

Há fundações de registro, configuração e revogação no código. Elas não provam
função Scatty instalada, conta real admitida, cartão consumido ou UI produtiva.
A recriação nativa de agenda/foco e seus gates segue planejada; o MCP Scatty externo
é opcional e separado. Essa direção não autoriza terceiros, instalador universal,
rotinas automáticas ou acesso de outros bots.

## Limite atual e evolução

O Hub entrega skills selecionadas pelo perfil/contexto e conectores elegíveis
por pontes MCP nas sessões ACP. Não há importador/exportador de plugin completo
OpenAI/Claude demonstrado nesse fluxo. Instalar no perfil global do motor para
contornar a ponte alteraria a segurança e não é parte deste guia.

Inspeção sem efeito, exportação e importação passivas são propostas futuras.
Uma inspeção deve ler manifesto e arquivos sem executar `verify`, hooks,
instaladores ou comandos de dependência. Se houver implementação futura, ela
deve reutilizar catálogo, procedência, recibos e autorização existentes, mostrar
incompatibilidades e manter instalação, seleção, login e chamada separados.

Pacotes com código ativo, MCP, agentes, LSP, binários ou UI precisam de perfil
de execução, isolamento e revisão próprios. O guia não implementa instalação ativa,
publicação, marketplace ou atualização automática; a consulta
real do guia pelo agente é somente leitura.
