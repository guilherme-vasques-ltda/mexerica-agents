# Transporte remoto (inativo)

O ponto de entrada do perfil remoto não cria o transporte nem monta o renderer compartilhado. Não há endpoint do Hub, credencial ambiente ou flag para ligar a ponte.

## Ligação do transporte

`createRemoteHub(resolve)` recebe um vínculo explícito para cada workspace: origem HTTPS, função de autorização por requisição e `fetch` injetado. `hub.host` implementa a ponte que `createWebBridge` entrega ao renderer. O `token` nessa ponte é apenas um identificador local da sessão, não uma credencial de rede.

## Pré-requisitos para ativar

Nada disso deve ser ligado antes de estar pronto:

- B-2: sessão Clerk verificada pelo Hub e prova de posse do dispositivo por requisição;
- descoberta de servidor confiável;
- rota HTTP/SSE com autorização remota e validação em laboratório;
- isolamento do cache compartilhado persistente (trabalho separado).

Não use o token local do host nem acesso por loopback como alternativa. Se a autenticação remota falhar, o cliente falha fechado.

## Épocas do workspace (`workspaceEpoch`)

- A abertura inicial de `/events` não envia uma época desconhecida: ela vem em `hello.workspace.workspaceEpoch`.
- Nos RPCs seguintes, `workspaceEpoch` vai no corpo. Nas assinaturas SSE do renderer, vai na query.

### Divergência de época em HTTP

- O cliente limpa o workspace e inicia apenas um novo handshake.
- A ação que causou a divergência **nunca é reenviada**.

### Revogação terminal ou mudança de papel

- A sessão para.
- O cliente volta ao seletor de workspace.

### Offline

- O cliente não reconecta sozinho.
- O usuário precisa tentar de novo manualmente.

Os testes abaixo da ponte usam um Hub falso injetado, sem rede, conta ou aparelho real.
