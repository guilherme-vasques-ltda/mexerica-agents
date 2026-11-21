# Fontes do guia de extensibilidade da Mexerica

[Índice](README.md)

**Consulta: 04/11/2026 (UTC), para todas as fontes abaixo.** Este inventário
registra as fontes oficiais usadas neste guia.
Os capítulos são sínteses autorais; não são espelhos das fontes nem prova de
instalação, compatibilidade do adaptador ACP ou disponibilidade em uma conta.

O guia acrescenta a skill passiva autoral `docs/skills/criador-de-skills/SKILL.md`,
com referências geradas destes seis capítulos em JSON passivo dentro de `host/`.
O JSON versionado é a entrada de compilação; os Markdown continuam fonte legível.
Ao editar, gere/confira pelo comando documentado no índice e inclua o artefato
no mesmo PR, sem include direto dos novos documentos nem mudança de hook/CI.
A recomendação/rascunho e a nomenclatura “Extensões da Mexerica” /
“Plugins de IA” seguem o índice, sem instalação, ligação, edição global ou execução novas.
A origem do pacote aponta este guia; o `sourceDigest` é o SHA-256 da
concatenação nome de arquivo + NUL + bytes dos seis capítulos, ordenados por
nome. `ADAPTATION.md` distingue essa origem dos textos da build atual; hashes
por arquivo e digest de envelope identificam os bytes atuais. Sem cópia do PDF
ou da skill externa, sem consulta de rede em runtime.

O histórico de URLs, datas, sínteses e mudanças fica no Git destes arquivos.
Em uma revisão, atualize a data e registre o que mudou, qual comportamento foi
conferido no código e o que continua planejado. Não atualize por `latest`, branch
móvel ou extensão em draft como se houvesse implementação ou aceite novos.
URLs sem commit são páginas mutáveis, mesmo quando trazem revisão datada do
protocolo; não têm hash de captura fixado neste guia. Se uma captura futura for
necessária e permitida, registre seu SHA-256 e revisão, sem publicar o original
externo na wiki. Um hash não é assinatura do publicador.

## Skills

Responsáveis: Anthropic e projeto Agent Skills. Sustentam [skills](skills.md)
e as fronteiras de autoria/licença em [segurança](seguranca.md).

| Fonte oficial | Revisão e uso |
| --- | --- |
| [The Complete Guide to Building Skills for Claude — PDF](https://resources.anthropic.com/hubfs/The-Complete-Guide-to-Building-Skill-for-Claude.pdf) | Método de intenção, exemplos, descrição e revisão; 33 páginas na consulta. SHA-256 da captura informado pela pesquisa: `66cbcc06df9271e0bb59986582646f0c4c7c94697554f893f937a5a55cef736d`. PDF não incorporado; sua condição de uso não é deduzida da licença do código |
| [Agent Skills — Specification](https://agentskills.io/specification) | Formato, frontmatter, divulgação progressiva e recursos opcionais. Não é especificação MCP |
| [Anthropic skill-creator — SKILL.md](https://github.com/anthropics/skills/blob/ca45fc496a3436251f191354091d557da31c15b5/skills/skill-creator/SKILL.md) | Método consultado no commit `ca45fc496a3436251f191354091d557da31c15b5`; não importado nem instalado |
| [Licença da skill-creator](https://github.com/anthropics/skills/blob/ca45fc496a3436251f191354091d557da31c15b5/skills/skill-creator/LICENSE.txt) | Apache-2.0 específica dessa skill; não cobre automaticamente PDF ou demais skills |
| [README Anthropic Skills](https://github.com/anthropics/skills/blob/ca45fc496a3436251f191354091d557da31c15b5/README.md) | Licenças por conteúdo; conferir o arquivo específico antes de copiar |

O catálogo da DONNA mantém pin próprio de `skill-creator`,
`dbd4588f9e1033efb41dad4bef2f7947c8993d44`, com indisponibilidade por tamanho.
A consulta a outro commit não atualiza esse bundle. A procedência existente
está em [THIRD_PARTY.md](../../../THIRD_PARTY.md); os limites e o formato real de
instalação estão no [contrato do catálogo](../../reference/catalogo-skills.md).

## MCP

Responsável: projeto Model Context Protocol. Referência moderna:
**2026-07-28**; referência anunciada pelo Hub: **2025-06-18**. Sustentam
[MCP](mcp.md); revisão do protocolo, versão do SDK e versão da DONNA são objetos
distintos.

| Fonte oficial | Revisão e uso |
| --- | --- |
| [Arquitetura](https://modelcontextprotocol.io/specification/2026-07-28/architecture) | 2026-07-28; host, cliente e servidor |
| [Versionamento e compatibilidade](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning) | 2026-07-28; metadados por pedido e compatibilidade entre eras |
| [server/discover](https://modelcontextprotocol.io/specification/2026-07-28/server/discover) | 2026-07-28; versões, capacidades e identidade autodeclarada |
| [stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio) | 2026-07-28; processo local e enquadramento das mensagens |
| [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http) | 2026-07-28; POST, SSE por pedido, cabeçalhos e retirada de sessões |
| [MRTR](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr) | 2026-07-28; entrada necessária e continuação |
| [Subscriptions](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/subscriptions) | 2026-07-28; assinaturas e correlação |
| [Cancelamento](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/cancellation) | 2026-07-28; cancelar pedido não garante desfazer efeito |
| [Tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) | 2026-07-28; descoberta/chamada, schemas e resultados |
| [Resources](https://modelcontextprotocol.io/specification/2026-07-28/server/resources) | 2026-07-28; contexto por URI |
| [Prompts](https://modelcontextprotocol.io/specification/2026-07-28/server/prompts) | 2026-07-28; modelos de mensagens |
| [Cache](https://modelcontextprotocol.io/specification/2026-07-28/server/utilities/caching) | 2026-07-28; TTL e escopo por autorização |
| [Autorização](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization) | 2026-07-28; perfil OAuth HTTP |
| [Discovery OAuth](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/authorization-server-discovery) | 2026-07-28; recurso, emissor e metadados |
| [Registro do cliente](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration) | 2026-07-28; pré-registro, CIMD e DCR |
| [Segurança OAuth](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations) | 2026-07-28; issuer, callback, PKCE e custódia |
| [Elicitation](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation) | 2026-07-28; formulário não secreto e interação por URL |
| [Depreciações](https://modelcontextprotocol.io/specification/2026-07-28/deprecated) | Roots, sampling, logging e DCR; depreciado não significa removido |
| [Changelog 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/changelog) | Mudanças da revisão anunciada pelo Hub |
| [Lifecycle 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle) | Initialize e negociação da era legada |
| [Transportes 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) | Streamable HTTP com sessões opcionais e cabeçalho de versão |
| [Changelog 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/changelog) | Revisão intermediária ainda com handshake |
| [Changelog 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/changelog) | Mudanças estruturais da era moderna |
| [Boas práticas de segurança](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/security_best_practices) | Confused deputy, token passthrough e mitigação |
| [Segurança de servidor local](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/local-server-security) | Execução de processo e limites de isolamento |
| [Extensões](https://modelcontextprotocol.io/extensions/overview) | Estados das extensões, separados do estado de cada SEP |
| [Skills over MCP](https://modelcontextprotocol.io/extensions/skills/overview) | Beta na consulta; requisitos de host de até 512 arquivos/16 MiB. Sem suporte DONNA, cujo catálogo limita a 100/5 MiB |
| [MCP Apps](https://modelcontextprotocol.io/extensions/apps/overview) | UI interativa; não equivale a apps de terceiros |
| [Tasks](https://modelcontextprotocol.io/extensions/tasks/overview) | Extensão assíncrona; não equivale a Rotinas |
| [Registry](https://modelcontextprotocol.io/registry/about) | Catálogo de metadados em preview, distinto de discovery e do catálogo de links oficiais |
| [Moderação do Registry](https://modelcontextprotocol.io/registry/moderation-policy) | Limites de confiança e de moderação |
| [Termos do Registry](https://modelcontextprotocol.io/registry/terms-of-service) | Metadados CC0 não licenciam os pacotes apontados |
| [LICENSE do projeto MCP](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/LICENSE) | Transição de licenças: código/especificação novos Apache-2.0, demais docs CC-BY-4.0, contribuições antigas podem conservar MIT. Verificar por revisão/arquivo antes de copiar |

O inventário local usa `host/src/mcp.rs`, `host/src/market/`,
`host/src/guard_hub/mcp/` e `host/src/agent/bot/session.rs` na base indicada no
[índice](README.md). Os contratos de produto são
[credenciais](../../features/connector-credentials.md),
[marketplace](../../features/marketplace.md) e o
plano de Conectores.
As lacunas registradas são encaminhamento das pesquisas, não correções entregues.

## Plugins

Responsáveis: OpenAI e Anthropic/Claude Code. Páginas mutáveis consultadas na
data acima; sustentam [plugins](plugins.md). O schema portátil referido pela
OpenAI é Agent Plugins **1.0.0**. Não existe uma única versão de protocolo comum
a todos os recursos dessas páginas.

| Fonte oficial | Escopo e ressalva |
| --- | --- |
| [OpenAI — arquitetura](https://developers.openai.com/plugins/concepts/plugins) | Skills, MCP e UI opcional como pacote |
| [Build skills](https://developers.openai.com/plugins/build/skills) | Procedimentos, dependências e recursos |
| [Build an MCP server](https://developers.openai.com/plugins/build/mcp-server) | Ferramentas, schemas e servidor |
| [Package your plugin](https://developers.openai.com/plugins/build/plugins) | Portátil/compatibilidade, paths, overlay e hooks restritos por superfície |
| [Authentication](https://developers.openai.com/plugins/build/auth) | A página consultada ainda referencia MCP 2025-11-25; não comprova suporte geral a 2026 |
| [Add UI](https://developers.openai.com/plugins/build/chatgpt-ui) | MCP Apps e apresentação opcional |
| [MCP Events](https://developers.openai.com/plugins/build/mcp-events) | Extensão em draft, caminhos 2026-07-28 e legado; não ligada por este guia |
| [Extensions](https://developers.openai.com/plugins/build/extensions) | Superfícies próprias do ChatGPT |
| [Submissão](https://developers.openai.com/plugins/deploy/submission) | Versão, metadados, ZIP, revisão e publicação separados |
| [Desktop only](https://developers.openai.com/plugins/deploy/desktop-only) | Requisitos próprios para distribuição local/stdio |
| [Conectar e testar](https://developers.openai.com/plugins/deploy/connect-chatgpt) | Integração de desenvolvimento; não prova funcionamento na DONNA |
| [Migrar plugin Claude](https://developers.openai.com/plugins/guides/submit-claude-plugin) | Conversão e componentes não portáveis |
| [Segurança e privacidade](https://developers.openai.com/plugins/guides/security-privacy) | Dados, UI, rede e isolamento |
| [Diretrizes de plugins](https://developers.openai.com/plugins/plugin-guidelines) | Direitos e regras do diretório; não substituem autorização no Hub |
| [Learn — usar plugins](https://learn.chatgpt.com/docs/plugins) | Disponibilidade por superfície e instalação |
| [Learn — hooks](https://learn.chatgpt.com/docs/hooks) | Definição exata e confiança de hooks; não concede Authority |
| [Claude Code — plugins](https://code.claude.com/docs/en/plugins) | Visão geral |
| [Referência de manifesto](https://code.claude.com/docs/en/plugins-reference) | Layout, campos e componentes |
| [Criar marketplace](https://code.claude.com/docs/en/plugin-marketplaces) | Distribuição específica do Claude |
| [Referência de marketplace](https://code.claude.com/docs/en/plugins/marketplace-reference) | Manifesto do catálogo Claude |
| [Instalar e gerir](https://code.claude.com/docs/en/plugins/install) | Escopos e habilitação |
| [Segurança de plugins](https://code.claude.com/docs/en/plugins/security) | Código de componentes e privilégios do usuário |

Essas páginas não concedem licença geral para redistribuir código, textos ou
assets. O guia usa síntese própria e não incorpora pacotes externos. Para qualquer
cópia futura, conferir licença por artefato/revisão antes da distribuição.
O conceito local de Extensões da Mexerica permanece separado desses
formatos oficiais e sem instalação ativa de terceiros.

O plano de senha por referência
é o contrato documental de destino, coleta,
prazo e isolamento referido em [segurança](seguranca.md). O plano não
significa implementação ou capacidade disponível no app.
