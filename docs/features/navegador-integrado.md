# Navegador integrado: abrir e ler páginas

Um bot com a preferência **Integrado** pode abrir páginas HTTPS públicas e ler o texto visível.
No Mac, o dono local pode assistir à página no app. Clique, digitação, login e arquivos não estão
disponíveis nesta etapa. Cada sessão pertence a um bot, no seu Space.

## No Mac do dono

Em **Configurações do bot → Comportamento → Navegador**, selecione **Integrado ao app** e salve.
Leia o aviso e clique em **Ativar navegador integrado** para conceder o consentimento local. O Hub
usa o Google Chrome ou o Chromium instalado na pasta Aplicativos. Antes da primeira ativação,
o app avisa que o navegador se conecta à internet como o Chrome pessoal, mas com um perfil
temporário separado e com o sandbox nativo do Chrome ligado. O Hub recusa abrir se detectar
que o Mac é gerenciado, achar uma política de navegador relevante ou indício de gestão persistida
do Chrome, ou não conseguir concluir a verificação. O Hub repete a verificação enquanto houver
sessão ativa e encerra as sessões ao detectar gestão ou se a verificação falhar. Não é um bloqueio
instantâneo de rede no sistema. A ativação vale para os bots com a preferência Integrado. **Assistir ao navegador**
mostra a página enquanto o bot navega; não precisa de aparelho pareado nem aceita cliques.

O Hub oferece abrir, ler e fechar ao Codex durante o turno iniciado pelo dono no app, tanto numa
conversa nova quanto numa retomada. O perfil começa vazio, sem contas, cookies ou senhas pessoais,
e é descartado ao fim do turno. Desativar o navegador encerra as sessões. O Hub nunca troca para
o navegador pessoal nem usa busca como substituto da navegação.

O Hub fala com o Chrome por um pipe local, sem porta de depuração, e usa o mesmo proxy HTTPS
validado para a navegação. No Mac, esse proxy é uma configuração do Chrome, não uma barreira
de saída do sistema como no Linux. O modo de leitura do Mac desativa os scripts das páginas e
limita as requisições interceptadas a GET e HEAD sem corpo na mesma origem HTTPS da página
pedida. Redirecionamentos para outra origem e sites que dependem de scripts podem não carregar.
GET ainda pode ter efeitos do lado do site. Os quadros vão ao app nativo do dono; não fazem
parte das respostas ao modelo.

## Ferramentas

Os argumentos das três ferramentas são fechados (campo extra é
recusado, e nada que a página devolve volta como argumento):

- `browser_open {url}`: abre o endereço na sessão do próprio bot. Devolve o endereço final e o
  título, sem o corpo da página.
- `browser_read {}`: devolve o texto visível da página aberta (até 20.000 caracteres, com aviso
  de corte). Ler nunca cria navegador. Sem sessão, o servidor `memory` devolve `browser_no_page`;
  a ponte local do Codex devolve o erro genérico `Browser operation refused or unavailable.`.
- `browser_close {}`: encerra a sessão e apaga o perfil temporário. Nunca é recusado ao dono da
  sessão. No fluxo do dono local, desligar a preferência já encerra a sessão.

A ponte local do Codex (`mexerica_browser`) lista abrir, ler e fechar diretamente no turno autorizado, sem
`browser_status`. A negociação a seguir vale apenas para o servidor MCP `memory`.

No servidor MCP `memory`, `browser_status` aceita a versão 3 e informa se o navegador está disponível, o motivo
(`ready`, `engine_missing`, `running_as_root`, `privileged_group`, `containment_unavailable`,
`unsupported_platform`, `preference_computer`, `preference_off`) e as ferramentas oferecidas.
Quando responde `available`, o vínculo de execução daquele bot fica **negociado**: só então
`browser_open` e `browser_read` aparecem no catálogo e são aceitos. Sem negociação, a chamada falha
com `browser_negotiation_required`. Como o catálogo só muda depois da negociação, o cliente precisa listar as ferramentas de novo:

- **Motores ACP** (a ponte `memory` fala MCP por stdio): a ponte anuncia `tools.listChanged` e, quando a
  resposta de `browser_status` v3 diz `available`, envia uma vez `notifications/tools/list_changed`. O cliente
  que obedece essa notificação (é o mecanismo padrão do MCP) relista e passa a ver `browser_open`,
  `browser_read` e `browser_close`. Cliente que lista só no início da sessão e ignora a notificação não as vê.
- **Motor Mexerica** (o Hub é o cliente MCP): antes de cada prompt, entre turnos, o Hub consulta `browser_status`
  v3 pelo servidor interno `memory`, relista as ferramentas e, se mudaram, manda `set_tools` ao motor (que só
  aceita essa troca fora de um turno). O bot vê as ferramentas no mesmo turno em que pede. Um servidor de
  terceiro que se chame `memory` nunca é consultado nem relistado.

Clientes que usam a versão 2 continuam recebendo a recusa de sempre. Preferência `computer` ou `off` recusa abrir, sem cair em outro navegador.

## Sessão

Uma por Hub, Space e bot, criada na primeira abertura, com até 3 sessões simultâneas e limite
de 5 minutos de ociosidade. No Linux, o Hub usa Chromium do sistema (caminhos em
`MEXERICA_CHROMIUM` e `MEXERICA_XVFB`, ou os padrões) e Xvfb próprio. O Hub no Linux não abre
o navegador sem namespaces de usuário, rede, montagem e PID,
como usuário root ou se o usuário do Hub estiver nos grupos `docker`, `adm` ou `systemd-journal`. Os
grupos são resolvidos pelo sistema (arquivo local e serviços de nomes); se a lista ou algum nome não puder
ser lido, o navegador fica indisponível (`containment_unavailable`) em vez de liberar.

O Chromium roda com um proxy por sessão que só aceita HTTPS na porta 443 e recusa destinos
internos ou respostas DNS mistas. No Linux, o processo enxerga um `/proc` novo e não vê os dados do Hub. O
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
  só é retomada (o `window.open` do opener espera por isso) com o filtro de rede ligado e a execução de
  scripts da janela desligada, e é fechada logo em seguida. Se qualquer um dos dois falhar, ela nunca é
  retomada e a sessão cai. O fechamento só vale com confirmação explícita, e sem ela a sessão inteira
  (Chromium, proxy e perfil) é descartada. Download é negado.
- O texto é higienizado (sem controles, bidi, largura zero nem caracteres "tag" Unicode) e fica num
  envelope JSON: não consegue fechar o envelope nem criar campos.
- Página que não é web, ou que mudou de endereço durante a leitura, não devolve conteúdo.

A página ainda pode tentar convencer o modelo. O Hub entrega o texto como dado e avisa; não
impede o modelo de obedecer. Ações sensíveis precisam de autorização fora do conteúdo da página.

## Limites conhecidos

- Desconectar uma conta gerenciada não derruba, na hora, uma sessão de navegador já aberta que estava
  ociosa: nenhum turno novo usa a conta, mas a página aberta só cai pela ociosidade (5 minutos), por outra
  revogação ou pelo fechamento.
- Entre retomar uma janela nova e fechá-la há uma ida e volta de CDP; nela os scripts da janela já estão
  desligados e a rede só passa por HTTPS filtrado.

## Testes

Os testes unitários rodam com `cargo test fork::browser`. Os testes com Chromium real são
`#[ignore]` e exigem Chromium, Xvfb, namespaces de usuário/rede/montagem/PID e, no de redirects, `python3`,
`openssl` e `MEXERICA_TEST_CHROMIUM_FLAGS=--ignore-certificate-errors`:

```sh
cargo build --bin donnabot-host
cargo test --bin donnabot-host fork::browser -- --ignored --test-threads=1
```
