# Navegador integrado: abrir e ler páginas

Um bot com a preferência **Integrado** pode abrir e ler páginas HTTPS num servidor Linux com
contenção. Não há tela do navegador, clique, digitação, login nem arquivo nesta etapa: o bot só
abre um endereço público e lê o texto visível. Cada bot tem a sua própria sessão, dentro do seu
Space.

## Ferramentas

O servidor MCP `memory` oferece três ferramentas de argumentos fechados (campo extra é
recusado, e nada que a página devolve volta como argumento):

- `browser_open {url}`: abre o endereço na sessão do próprio bot. Devolve o endereço final e o
  título, sem o corpo da página.
- `browser_read {}`: devolve o texto visível da página aberta (até 20.000 caracteres, com aviso
  de corte). Sem sessão aberta, recusa com `browser_no_page`: ler nunca cria navegador.
- `browser_close {}`: encerra a sessão e apaga o perfil temporário. Nunca é recusado ao dono da
  sessão, mesmo com a preferência em `computer` ou `off`.

`browser_status` aceita a versão 3 e informa se o navegador está disponível, o motivo
(`ready`, `engine_missing`, `running_as_root`, `privileged_group`, `containment_unavailable`,
`unsupported_platform`, `preference_computer`, `preference_off`) e as ferramentas oferecidas.
Quando responde `available`, o vínculo de execução daquele bot fica **negociado**: só então
`browser_open` e `browser_read` aparecem no catálogo e são aceitos. Sem negociação, a chamada falha
com `browser_negotiation_required`. Como o catálogo só muda depois da negociação, o motor ou cliente precisa listar as ferramentas de novo
depois de `browser_status` v3 para vê-las; quem lista só no início da sessão não as enxerga. Clientes
que usam a versão 2 continuam recebendo a recusa de sempre. Preferência `computer` ou `off` recusa abrir, sem cair em outro navegador.

## Sessão

Uma por Hub, Space e bot, criada na primeira abertura: Chromium do sistema (caminhos em
`MEXERICA_CHROMIUM` e `MEXERICA_XVFB`, ou os padrões), Xvfb próprio e perfil temporário vazio. No
máximo 3 sessões ao mesmo tempo e 5 minutos de ociosidade. Só Linux; em outros sistemas o status é
`unsupported_platform`. O Hub não abre o navegador sem namespaces de usuário, rede, montagem e PID,
como usuário root ou se o usuário do Hub estiver nos grupos `docker`, `adm` ou `systemd-journal`. Os
grupos são resolvidos pelo sistema (arquivo local e serviços de nomes); se a lista ou algum nome não puder
ser lido, o navegador fica indisponível (`containment_unavailable`) em vez de liberar.

O Chromium roda com um proxy por sessão que só aceita HTTPS na porta 443 e recusa destinos
internos ou respostas DNS mistas. O processo enxerga um `/proc` novo e não vê os dados do Hub. O
token do proxy chega só pelo canal de controle, nunca por argumento ou ambiente.

## Autoridade e revogação

- A sessão pertence ao vínculo de execução que a abriu e à geração da configuração do bot naquele instante.
  A geração muda a cada escrita da configuração; por isso sair da preferência Integrado e voltar, ou trocar o
  vínculo ativo do bot, nunca revalida uma sessão velha. Outro vínculo (por exemplo, depois da
  troca de conta ou de motor) nunca reaproveita a aba nem os cookies: abre uma sessão nova e vazia.
- Abrir e ler na mesma aba são serializados por instância: uma operação só começa depois de a anterior
  terminar, então uma abertura nunca devolve a página de outra chamada.
- Ler e fechar capturam a instância pretendida antes de qualquer espera e só valem para ela; uma instância
  substituta, mesmo do mesmo vínculo, nunca é lida nem fechada.
- A autoridade completa (execução original, contexto, geração e vínculo) é conferida em cada comando de
  página no instante do envio, já com a trava do escritor do canal CDP. Revogada, o comando não sai. O corte
  acorda uma escrita que estava presa.
- A instância entra no registro de cortes antes de autenticar: revogar durante o arranque corta o proxy e o
  controle na hora. Arranque cancelado ou que falha apaga o perfil, derruba o proxy e mata os processos.
- Revogação do vínculo, troca de conta ou de Space, bot apagado e preferência fora de Integrado
  derrubam a sessão por um corte que não usa a trava do supervisor: o arranque lento de outra
  instância não atrasa a revogação. O corte desliga o proxy e o canal de controle; processos e
  perfil são encerrados em seguida pela manutenção.
- A conferência roda a cada 250 ms. Esse é o período de checagem, não um tempo máximo garantido.
- A queda do canal de controle (por falha) corta o proxy da instância na hora, sem depender da trava do
  supervisor. O corte é efetivo antes de qualquer limpeza começar.
- Ao encerrar uma sessão, o Hub corta proxy e controle antes de qualquer limpeza. Se parar o
  Chromium ou apagar o perfil falhar, a sessão deixa de ser servida, o diagnóstico fica no log (sem
  credencial) e a limpeza pendente é refeita pela manutenção e a cada nova abertura; enquanto
  pendente, ocupa uma vaga do limite.

## A página é dado, nunca instrução

O texto vem sempre dentro de `page`, com `untrustedContent: true` e um aviso fixo de que não é
instrução nem autorização.

- A leitura roda num mundo isolado do CDP com script fixo: a página não consegue trocar `String`,
  `JSON`, `document.body` nem `document.title` para forjar o resultado.
- Nenhum texto da página vira comando CDP, URL ou argumento. O transporte CDP só aceita uma lista
  de métodos permitidos, e a navegação só existe com um destino validado.
- Redirecionamento ou subrecurso fora de HTTPS falha antes de sair do navegador. Toda janela nova
  só é retomada (o \`window.open\` do opener espera por isso) com o filtro de rede ligado e a execução de
  scripts da janela desligada, e é fechada logo em seguida. Se qualquer um dos dois falhar, ela nunca é
  retomada e a sessão cai. O fechamento só vale com confirmação explícita, e sem ela a sessão inteira
  (Chromium, proxy e perfil) é descartada. Download é negado.
- O texto é higienizado (sem controles, bidi, largura zero nem caracteres "tag" Unicode) e fica num
  envelope JSON: não consegue fechar o envelope nem criar campos.
- Página que não é web, ou que mudou de endereço durante a leitura, não devolve conteúdo.

A página ainda pode tentar convencer o modelo. O Hub entrega o texto como dado, avisa, e garante
que a página não tem como agir sozinha, mas não impede o modelo de obedecer. Ações sensíveis
precisam de autorização fora do conteúdo da página.

## Testes

Os testes unitários rodam com `cargo test fork::browser`. Os testes com Chromium real são
`#[ignore]` e exigem Chromium, Xvfb, namespaces de usuário/rede/montagem/PID e, no de redirects, `python3`,
`openssl` e `MEXERICA_TEST_CHROMIUM_FLAGS=--ignore-certificate-errors`:

```sh
cargo build --bin donnabot-host
cargo test --bin donnabot-host fork::browser -- --ignored --test-threads=1
```
