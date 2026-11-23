# Transporte remoto (inativo)

O ponto de entrada do perfil remoto não cria o transporte nem monta o renderer compartilhado. Não há endpoint do Hub, credencial ambiente ou flag para ligar a ponte.

## Ligação do transporte

`createRemoteHub(resolve)` recebe um vínculo explícito para cada workspace: origem HTTPS, função de autorização por requisição e `fetch` injetado. `hub.host` implementa a ponte que `createWebBridge` entrega ao renderer. O `token` nessa ponte é apenas um identificador local da sessão, não uma credencial de rede.

## Pré-requisitos para ativar

Nada disso deve ser ligado antes de estar pronto:

- sessão Clerk verificada pelo Hub e prova de posse do dispositivo por requisição;
- descoberta de servidor confiável;
- rota HTTP/SSE com autorização remota e validação em laboratório;
- isolamento do cache compartilhado persistente (trabalho separado).

Não use o token local do host nem acesso por loopback como alternativa. Se a autenticação remota falhar, o cliente falha fechado.

## Épocas do workspace (`workspaceEpoch`)

- A abertura inicial de `/events` não envia uma época desconhecida: ela vem em `hello.workspace.workspaceEpoch`.
- Nos RPCs seguintes, `workspaceEpoch` vai no corpo. Nas assinaturas SSE do renderer, vai na query.
- Os eventos de dados também precisam trazer o `workspaceEpoch` atual. Antes de ativar, a rota remota deve emitir esse campo nos eventos que serão repassados ao renderer.

### Divergência de época ou fim de contexto

- `409 workspace_epoch_mismatch`, `workspace_revoked` e `role_changed` cancelam a sessão anterior e limpam o cache antes de buscar um novo `hello` no mesmo workspace. Se a divergência se repetir, o cliente mostra erro e espera uma nova escolha.
- A ação que causou a divergência **nunca é reenviada**.

### Aparelho revogado

- `device_revoked` encerra a sessão, limpa o cache e indica que o acesso deste aparelho acabou. Não há reconexão automática.

### Offline

- O cliente não reconecta sozinho.
- O usuário precisa tentar de novo manualmente.

Os testes abaixo da ponte usam um Hub falso injetado, sem rede, conta ou aparelho real.
