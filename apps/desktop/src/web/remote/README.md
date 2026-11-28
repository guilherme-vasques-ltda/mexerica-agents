# Web remota (desligada por padrão)

O build local continua separado. Mesmo com a configuração presente, sem `WEB_REMOTE_ENABLED=1` o perfil remoto não carrega o Clerk, não abre WebSocket e não inclui o renderer remoto na saída.

## Build e hospedagem de teste

Rode `npm run build:web:remote` na pasta do app. A configuração de laboratório fica fora do repositório:

- `WEB_REMOTE_ENABLED=1`: habilita o **build** de laboratório; com a flag ausente, o build fica desligado.
- `WEB_REMOTE_CLERK_TEST_PUBLISHABLE_KEY`: chave publicável de desenvolvimento; chaves de produção são recusadas.
- `WEB_REMOTE_HUBS`: lista JSON com `workspaceId`, `hubId`, `origin`, `signKey` e, opcionalmente, `relayOrigin`. As origens aceitam só WSS, sem usuário, senha, caminho ou query. A chave Ed25519 pública do Hub vem de uma fonte confiável, não do próprio socket. Tudo nessa configuração é público no bundle; não use valores privados.
- Sirva com `npm run web:remote`, que escuta só no loopback (`WEB_REMOTE_PORT` escolhe a porta), atrás de um proxy HTTPS em uma **origem dedicada**. Preserve os cabeçalhos `Content-Security-Policy` e `X-Frame-Options` das respostas: o portão usa `frame-ancestors 'none'`; o renderer, `frame-ancestors 'self'`. Não sirva o bundle com fallback de SPA nem de uma origem que hospede conteúdo não confiável.
- Autorize a origem HTTPS do portão em `authorizedParties` no vínculo Clerk do Hub e, se houver relay, em `WEB_ALLOWED_ORIGINS` no relay. São listas distintas. O proxy WSS direto deve preservar o cabeçalho `Origin` do navegador.
- Entre na conta de teste, escolha o workspace e cole o código exibido pelo Hub. O pareamento sempre usa o WSS direto. Depois do vínculo, se houver `relayOrigin`, a sessão usa apenas o relay, sem fallback silencioso. O relay precisa conhecer o Hub e receber sua lista assinada de aparelhos. Não há descoberta automática, fallback por HTTP/SSE remoto ou uso do token local.

O navegador gera uma chave Ed25519 privada **não extraível** no WebCrypto e guarda o par de chaves no IndexedDB. Navegadores que não suportam esse armazenamento ou algoritmo não pareiam. Não há fallback para chave extraível. O token Clerk só vai no corpo cifrado de `pair` e `sessionAuthenticate`, nunca na URL ou no armazenamento do app.

## Queda, troca e revogação

- Toda sessão confirma a identidade, o Hub e o workspace antes de montar o app. O Hub concede ao navegador apenas chat e eventos, por até 24 horas a partir do pareamento; renovar a sessão Clerk não estende esse prazo. Vínculos Web antigos com escopo mais amplo exigem novo código. RPCs e assinaturas usam o canal cifrado, prova de posse do aparelho e a época confirmada. Uma chamada sem resposta não é repetida pelo transporte.
- Cada contexto usa uma instância nova do renderer. O iframe é de mesma origem e serve para montar e desmontar a interface; não é sandbox nem separa a confiança do portão e do renderer. O cache e os rascunhos ficam em memória, separados por Hub, workspace, usuário e época. Uma queda não purga essa memória; após 24 h sem confirmação ela fica selada até uma sessão autenticada. Não há exibição offline.
- Um corte assinado ou uma época mais nova confirmada determinam o que descartar. Revogação do aparelho encerra o acesso; recusa assinada de aparelho desemparelhado pede novo código. Uma recusa não assinada ou um fechamento sem prova só informa queda. Quando o Hub retornar, “Tentar de novo” abre um canal novo com a mesma chave, sem repetir chamadas em andamento.

Este perfil não publica nem ativa a Web em produção. A flag atual habilita apenas o laboratório. Antes de publicar para usuários, é preciso escolher a origem HTTPS, preparar e validar um perfil de build com a instância Clerk de produção e provisionar os vínculos e o relay. Para voltar ao perfil desligado, retire a flag, refaça o build e substitua a saída inteira, sem deixar os assets do bundle anterior disponíveis. Se necessário, revogue os aparelhos Web no Hub para cortar também sessões já abertas.
