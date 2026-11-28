# Web remota (desligada por padrão)

O build local continua separado. Sem a chave publicável de **teste** e uma lista explícita de Hubs, o perfil remoto não carrega o Clerk, não abre WebSocket e não inclui o renderer remoto na saída.

## Teste com um Hub vinculado

Use `vite build --config vite.web.config.remote.ts` na pasta do app. A configuração de laboratório fica fora do repositório:

- `WEB_REMOTE_CLERK_TEST_PUBLISHABLE_KEY`: chave publicável de desenvolvimento; chaves de produção são recusadas.
- `WEB_REMOTE_HUBS`: lista JSON com `workspaceId`, `hubId`, `origin` e `signKey`. A origem aceita só WSS direto, sem usuário, senha ou caminho. A chave Ed25519 pública do Hub é fixada a partir de uma fonte confiável, não aceita do próprio socket. As configurações são públicas no bundle de teste, não use valores privados.
- Sirva a saída em origem segura e permita essa origem no vínculo Clerk do Hub. Ao publicar em servidor HTTP, configure também `frame-ancestors 'none'` no cabeçalho CSP do portão.
- Entre na conta de teste, escolha o workspace e cole o código exibido pelo Hub. Não há descoberta automática, fallback por HTTP/SSE remoto ou uso do token local.

O navegador gera uma chave Ed25519 privada **não extraível** no WebCrypto e guarda o par de chaves no IndexedDB. Navegadores que não suportam esse armazenamento ou algoritmo não pareiam. Não há fallback para chave extraível. O token Clerk só vai no corpo cifrado de `pair` e `sessionAuthenticate`, nunca na URL ou no armazenamento do app.

## Queda, troca e revogação

- Toda sessão confirma a identidade, o Hub e o workspace antes de montar o app. RPCs e assinaturas usam `/channel`, prova de posse do aparelho e a época confirmada. Uma chamada sem resposta não é repetida pelo transporte.
- Cada contexto usa uma instância nova do renderer. O cache e os rascunhos do renderer ficam em memória, separados por Hub, workspace, usuário e época. Uma queda não purga essa memória; após 24 h sem confirmação ela fica selada até uma sessão autenticada. Não há exibição offline.
- Um corte assinado ou uma época mais nova confirmada determinam o que descartar. Revogação do aparelho encerra o acesso; recusa assinada de aparelho desemparelhado pede novo código. Uma recusa não assinada ou um fechamento sem prova só informa queda; a tentativa seguinte abre um canal novo.

O caminho direto requer um Hub com vínculo e um endpoint que encaminhe o WebSocket. Relay, perfis de produção, descoberta e ativação não fazem parte deste teste.
