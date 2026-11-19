# App da Mexerica no Mac

> **Nome do pacote (D443, PR d):** o app instalado se chama **Mexerica.app** (Dev: **Mexerica Dev.app**). Só o
> nome do pacote mudou: `appId`/bundleId (`app.donnabot.mac`), esquemas (`donnabot://`), pasta de dados
> (`~/Library/Application Support/Donnabot/`), o item do Chaveiro ("Donnabot Safe Storage"), os
> LaunchAgents e a identidade de assinatura "Donnabot Local" continuam os de antes. Quem já tem o
> `Donnabot.app` instalado troca pelo roteiro de [Trocar o Donnabot.app pelo Mexerica.app](#trocar-o-donnabotapp-pelo-mexericaapp-d443-pr-d).

O app do Mac é só o cliente do Hub. Os bots rodam numa máquina só deles, nunca no Mac (D76).
Sem Apple Developer (D48/D73), o app não é notarizado nem se atualiza sozinho: para atualizar,
você recompila e reinstala.

## Entrada obrigatória no aplicativo

O app só abre depois do login confirmado. Uma preferência antiga de boas-vindas não libera a entrada; sem configuração de conta, fica indisponível, sem modo offline. Recuperação aparece nas boas-vindas. Consulte a [regra canônica de entrada](../features/manual-do-harness.md#entrada-obrigatória-no-aplicativo).

## O que o app faz no Mac

- Não instala nem inicia o `donnabot-host` neste Mac: o serviço `app.donnabot.host` nunca é
  criado. O corte fica em `forkFeatures.localHost` (`apps/desktop/src/shared/fork-features.ts`)
  e não volta por preferência nem por variável de ambiente. O `HostController` fica no estado
  `disabled`, e as telas reagem a ele: as ações "Install host", "Restart host" e "Uninstall host
  service" somem da janela e da barra de menus.
- Se um build anterior deixou o LaunchAgent `app.donnabot.host`, o app o tira ao abrir
  (`launchctl bootout` e apaga o plist pelo label, sem precisar do binário). Os dados em
  `~/.donnabot` ficam. "Reset all data" também não depende do binário.
- A barra de menus mostra a conexão real com o Hub: "Connected", "Connecting…", "Connection
  failed", "Confirme a chave do computador na Mexerica", "Not connected" ou "No computer set up".
- Na variante principal, a primeira abertura cria o perfil SSH do Hub configurado pelo operador, com a porta do
  Donnabot 19422, **só se o alias existir como `Host` exato no `~/.ssh/config` deste Mac** (D76, D293;
  segue `Include` com caminho fixo ou curinga só no nome do arquivo). Sem o alias, nenhum computador é
  criado, nada é gravado e o app não tenta SSH: adicione o computador em Ajustes → Computadores.
  Usuário, porta SSH e chave vêm do `~/.ssh/config`. Você pode editar ou remover
  esse computador em Ajustes → Computadores. Se remover, ele não volta, e o campo Host sugere
  o alias ao adicionar outro.
- Conecta por um túnel SSH ao `donnabot-host` do Hub, que roda na interface de loopback do Hub
  (veja Contas e SSH). Os bots e a criação de bots ficam no Hub.
- Se o alias tem `AddressFamily inet6`, o túnel passa
  `-o AddressFamily=any`: o OpenSSH aplica a família também ao listener local, e o túnel
  escuta em `127.0.0.1`. O endereço do Hub continua vindo do `HostName`.
- A tela "Computer access" (Gravação de Tela e Acessibilidade deste Mac, pelo Donnabot Screen)
  fica escondida enquanto `forkFeatures.localHost` for `false` (`showsComputerAccess`, no mesmo
  arquivo), inclusive com um host de desenvolvimento: não aparece no primeiro uso nem em
  Ajustes, o menu da barra não tem "Set up computer access…" e o comando de janela não abre a
  folha. No editor do bot e no painel de detalhes, o interruptor "Use the computer" e a dica que
  manda abrir "Settings → Computer access" também somem: o computador dos bots é o Hub, um
  servidor sem tela. Tudo isso só volta com `forkFeatures.localHost` em `true`, o que também
  instala e inicia o host neste Mac. Para trazer a tela com o ajudante do Mac (D83, futura tela
  Computadores) com os bots ainda só no Hub, `showsComputerAccess` vai precisar de uma chave
  própria.
- No Mac, use o app empacotado conectado ao Hub remoto. `npm run dev` compila o Hub
  localmente e não faz parte deste fluxo; as checagens do Hub rodam no servidor.

## Antes de gerar

As instruções SSH abaixo atendem à conexão direta da variante principal. Elas não
são requisito do cliente Dev pela nuvem, cujo roteiro próprio aparece adiante.

- macOS com Xcode ou as Command Line Tools (`swift`), Node 24 e `npm ci` feito em `apps/desktop`.
- `ssh -o BatchMode=yes meu-hub true` tem de funcionar no Terminal sem pedir
  senha. `meu-hub` é um exemplo: substitua pelo alias configurado no seu `~/.ssh/config`.
  O app só entra com a chave (ssh-agent ou `IdentityFile`) e não digita senha.
- A chave do `IdentityFile` fica em `~/.ssh` como arquivo comum. Ela não pode estar em
  `~/Documents`, `~/Desktop` ou `~/Downloads`, nem ser um atalho para lá. Essas pastas são
  protegidas pelo macOS, então o `ssh` do app trava esperando o pedido "deseja ter acesso a
  arquivos na pasta Documentos", e o app fica em "Signing in…". O Terminal não mostra o
  problema porque já tem esse acesso.
- A chave do Hub já está no `~/.ssh/known_hosts`. Se não estiver, o app mostra as impressões
  digitais para você conferir antes de conectar.

## Gerar o app

Em `apps/desktop`:

```sh
env -u ELECTRON_RUN_AS_NODE DONNABOT_MAC_IDENTITY="Donnabot Local" npm run dist:mac:local
```

O comando:

1. escreve `resources/account-config.json` do ambiente `main`, sem nuvem e sem login;
2. compila o `codync-speech` (reconhecimento de fala no Mac, para chamadas de voz);
3. compila o app (`electron-vite build`);
4. empacota só a pasta do app para a arquitetura deste Mac (`tools/local-builder.cjs`):
   `dist/mac-arm64/Mexerica.app` no Apple Silicon, `dist/mac/Mexerica.app` no Intel.

A arquitetura vem do Mac (`sysctl hw.optional.arm64`), não do Node: um Node rodando no
Rosetta ainda gera o app arm64 num Apple Silicon.

O pacote não leva o `donnabot-host` nem o helper de tela, e não gera DMG nem zip de
atualização. Não há notarização, hardened runtime nem a identidade de assinatura do upstream.

### Conta e nuvem (desligadas neste recorte)

Neste recorte os plists `apps/desktop/config/dev.plist` e `main.plist` vêm vazios e o módulo de conta e nuvem fica **desligado** (falha fechada).
Nenhuma configuração de produção, chave publicável, endereço de nuvem ou manifesto de implantação faz parte desta cópia, e nenhuma deve ser
commitada. Uma instalação própria que queira ativar conta e nuvem precisa fornecer a sua configuração pública fora do repositório e conferir
o código de `apps/desktop/src/shared/fork-cloud-build.ts` (ambiente, origens e manifesto precisam coincidir; configuração incompleta ou de outro ambiente mantém o módulo fechado).

### Assinatura

Por padrão, a assinatura é ad-hoc (`-`). Com ela, cada recompilação muda a assinatura, e o
macOS pede de novo as permissões já dadas (microfone, acesso ao item "Donnabot Safe Storage"
do Keychain). Na primeira abertura depois de uma recompilação, escolha "Sempre permitir".

Use sempre o mesmo certificado para manter estável o requisito designado do app.
Isso não garante acesso ao Chaveiro entre builds: além da lista de aplicativos
confiáveis, o macOS verifica a partição do item. Com certificado autoassinado,
essa partição pode ser `cdhash:`, que muda quando o conteúdo do app muda.
"Sempre permitir" autoriza o build apresentado; outro build local pode pedir de novo,
mesmo com requisito designado idêntico. Permissões de microfone/TCC são outra verificação.

Nesta fase local sem Apple Developer, o certificado "Donnabot Local" é criado uma vez
no Keychain de login. Não o recrie para tentar resolver pedidos repetidos:

```sh
d=$(mktemp -d) && cd "$d"
cat > cert.cnf <<'EOF'
[ req ]
distinguished_name = dn
prompt = no
x509_extensions = ext
[ dn ]
CN = Donnabot Local
[ ext ]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
EOF
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 \
  -keyout key.pem -out cert.pem -config cert.cnf
pw=$(/usr/bin/openssl rand -hex 24)
/usr/bin/openssl pkcs12 -export -inkey key.pem -in cert.pem -name "Donnabot Local" \
  -out id.p12 -passout "pass:$pw"
security import id.p12 -k ~/Library/Keychains/login.keychain-db -P "$pw" -T /usr/bin/codesign
security add-trusted-cert -r trustRoot -p codeSign -k ~/Library/Keychains/login.keychain-db cert.pem
cd - >/dev/null && rm -rf "$d"
security find-identity -v -p codesigning | grep "Donnabot Local"
```

O `/usr/bin/openssl` é o LibreSSL do macOS, cujo `.p12` o `security import` aceita. O
`add-trusted-cert` pode pedir a senha do Mac. Sem essa confiança, o `electron-builder` não acha
a identidade, avisa "skipped macOS application code signing" e gera o app sem assiná-lo com
ela. Se o `grep` acima não imprimir nada, refaça a confiança. Pelo Acesso às Chaves dá no mesmo: Assistente de Certificado → Criar um
Certificado → "Raiz autoassinada", tipo "Assinatura de código", e depois Confiança →
"Assinatura de código: Sempre confiar".

Depois, todo build usa a identidade:

```sh
DONNABOT_MAC_IDENTITY="Donnabot Local" npm run dist:mac:local
```

Um certificado "Apple Development" próprio também serve para desenvolvimento
(`DONNABOT_MAC_IDENTITY` aceita o nome ou o hash SHA-1 de
`security find-identity -v -p codesigning`). O `codesign` pode pedir autorização
para usar a chave de assinatura; esse pedido é separado do acesso do app ao Safe Storage.
Nunca use o certificado ou o Team ID do upstream.

#### Chaveiro depois de atualizar

Se o macOS pedir acesso a "Donnabot Safe Storage" ou "Donnabot Dev Safe Storage",
confira o nome do app e do item no diálogo. Depois do PR (d), o diálogo pode dizer "Mexerica quer usar…
'Donnabot Safe Storage'": o item é o mesmo de antes, porque o nome dele vem do `app.setName('Donnabot')`, não
do nome do pacote. A própria pessoa escolhe "Sempre permitir"
se reconhecer ambos e fornece a senha somente no diálogo do sistema, se solicitada.
Não automatizar o SecurityAgent protegido, apagar o item, liberar todos os aplicativos,
editar a partição ou guardar a sessão sem cifra para evitar a autorização.

Para diagnosticar, compare `codesign -d -r-` e `codesign -d --verbose=4` do pacote
instalado e do rollback. Uma assinatura estrita válida e um requisito igual ainda
não comprovam admissão pela partição. Leia somente os metadados da ACL do item exato,
sem solicitar seu valor; não use dump de todos os itens nem opções que exibam senhas.

O cliente distribuído exige Developer ID e notarização já previstos na
[arquitetura do desktop](../architecture/desktop-app.md). A cadeia Apple válida e o
Team ID próprio permitem ao macOS usar uma partição `teamid:` estável; escrever um
Team ID no bundle ou confiar numa raiz autoassinada não equivale a essa identidade.
Antes da distribuição, qualifique duas versões assinadas da mesma equipe: o usuário
autoriza a primeira normalmente, atualiza no mesmo caminho e reabre a segunda sem
novo pedido causado apenas pela atualização. Mudança de equipe, ACL ou Chaveiro
bloqueado continua podendo exigir intervenção. A migração de um build local pode
precisar de autorização uma vez; a prova com duas versões de distribuição continua
necessária. Este guia não contrata Apple Developer nem ativa distribuição.

Fontes: [identidade de código e requisitos, Apple TN2206](https://developer.apple.com/library/archive/technotes/tn2206/),
[seleção de partição pelo securityd da Apple](https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/clientid.cpp)
e [armazenamento protegido do Electron](https://www.electronjs.org/docs/latest/api/safe-storage/).

## Instalar

As próximas seções tratam de `Mexerica.app`, a variante principal. Para a variante
Dev, use o roteiro acima.

O app vai para `~/Applications`, não para `/Applications`:

```sh
osascript -e 'quit app "Mexerica"' 2>/dev/null
mkdir -p ~/Applications
rm -rf ~/Applications/Mexerica.app
ditto dist/mac-arm64/Mexerica.app ~/Applications/Mexerica.app   # Intel: dist/mac/Mexerica.app
open ~/Applications/Mexerica.app
```

Um app compilado neste Mac não recebe o atributo de quarentena, então o Gatekeeper não o
bloqueia. Copiado de outro computador, ele seria bloqueado sem notarização.

Se você abrir o app de um terminal que roda dentro de outro app Electron (o de um agente, por
exemplo), o `ELECTRON_RUN_AS_NODE` herdado faz a Mexerica fechar ao abrir. Use
`env -u ELECTRON_RUN_AS_NODE open ~/Applications/Mexerica.app`. Pelo Finder ou pelo Dock, isso
não acontece.

### Trocar o Donnabot.app pelo Mexerica.app (D443, PR d)

Só para quem já tem `~/Applications/Donnabot.app`. **Só com GO do dono e com backup frio do banco do Hub antes**
(D444: a troca do app vem junto com a do Hub, que renomeia o principal). O roteiro não automatiza o diálogo do
Chaveiro e não apaga dados em nenhum passo.

1. **Antes:** anote se "Abrir ao iniciar sessão" está ligado. Encerre o app pela barra de menus; fechar a janela
   não basta.
2. **Rollback:** **mova** (não copie) o app antigo para a pasta de rollback da instalação, que fica fora do
   Spotlight, e tire o registro dele do LaunchServices:

   ```sh
   LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
   ROLLBACK="<pasta da instalação>.noindex/rollback"   # a mesma convenção operacao-install-<id>.noindex
   mkdir -p "$ROLLBACK" && mv ~/Applications/Donnabot.app "$ROLLBACK/Donnabot.app"
   "$LSREGISTER" -u "$ROLLBACK/Donnabot.app"
   ```

   Confira o ASAR/SHA do rollback como de costume.
3. **Instalar:**

   ```sh
   ditto dist/mac-arm64/Mexerica.app ~/Applications/Mexerica.app   # Intel: dist/mac/Mexerica.app
   "$LSREGISTER" -f ~/Applications/Mexerica.app
   env -u ELECTRON_RUN_AS_NODE open ~/Applications/Mexerica.app
   ```

4. **Conferir** (prova obrigatória antes de aceitar):
   - **Pasta de dados lida:** a mesma `~/Library/Application Support/Donnabot/` (perfis SSH e Hub conectado logo
     depois de abrir). Não existe pasta "Mexerica" em Application Support.
   - **Sessão sem novo login:** a sessão salva volta.
   - **Chaveiro:** nenhum item novo. `security find-generic-password -s "Mexerica Safe Storage"` deve **falhar**.
     Nunca use `-g` (mostraria o segredo). Se o macOS pedir acesso a "Donnabot Safe Storage", siga
     [Chaveiro depois de atualizar](#chaveiro-depois-de-atualizar).
   - **Assinatura:** `codesign -d -r- ~/Applications/Mexerica.app` dá o mesmo requisito do rollback
     (`identifier "app.donnabot.mac" and …`).
   - **Microfone (TCC):** em Ajustes do Sistema → Privacidade → Microfone, aparece "Mexerica" ligado. O TCC guarda
     bundleId + requisito de código, não o caminho.
   - **Item de login:** em Ajustes do Sistema → Geral → Itens de Início, só um item, "Mexerica". Religue em
     Ajustes → Geral do app se estava ligado.
   - **LaunchServices:** um único registro do bundleId, no caminho novo:
     `"$LSREGISTER" -dump | grep -B2 -A2 "app.donnabot.mac"` só mostra `~/Applications/Mexerica.app`.
   - **Dock:** o ícone fixado antes aponta para o arquivo antigo; fixe de novo.
5. **Volta:** encerre a Mexerica, mova `~/Applications/Mexerica.app` para um `.noindex` com `lsregister -u`,
   devolva o `Donnabot.app` do rollback a `~/Applications`, rode `lsregister -f` nele e abra. A troca de nome não
   migra dados, então a volta não perde nada do app. A volta do Hub tem regras próprias (plano Mexerica/Berga, §4).
6. Em nenhum momento apague `~/Library/Application Support/Donnabot/` nem o item "Donnabot Safe Storage".

## Conferir

- A janela abre em "Your remote computers" e passa a mostrar os bots do Hub assim que o túnel
  conecta. Em Ajustes → Computadores, o Hub aparece como "Over SSH".
- Nenhum serviço local do Hub no Mac:
  `launchctl print gui/$(id -u)/app.donnabot.host` responde que o serviço não existe, e
  `~/Library/LaunchAgents/app.donnabot.host.plist` não existe.
- O Codync instalado continua intacto, com o host dele na porta 19222.

## Dados no Mac

- Ajustes do app e perfis SSH: `~/Library/Application Support/Donnabot/`
  (`ssh-profiles.json`, `host-prefs.json`). O nome da pasta é técnico e continua "Donnabot" com o pacote
  Mexerica.app (D443).
- `~/.mexerica/ssh_known_hosts`: só se você confirmar no app a chave de um computador novo.

## Remover

```sh
osascript -e 'quit app "Mexerica"'
rm -rf ~/Applications/Mexerica.app "$HOME/Library/Application Support/Donnabot"   # a pasta de dados segue com o nome técnico
```

## Gate do runtime pessoal — PR3 candidato, sem instalação pela frente

O recorte Main M1 do PR3 usa o **beforePack existente** para gerar o recibo do
runtime antes de montar app.asar e assinar. Nenhum comando novo de instalação,
hook ou dependência foi adicionado. O fluxo local thin usa Electron44.5.1 oficial
por arquitetura e o diretório privado build/personal-runtime; não recebe mirror,
política ou recibo da interface. Cache/dist divergentes e lock de pack pendente
exigem conferir a operação original, sem sobrescrever ou repetir às cegas.
Não empacotar duas arquiteturas no mesmo out/main simultaneamente.

A prova de pacote pelo agente confere recibo selado, duas imagens, projeção
Mach-O restrita, ASAR/plist e `codesign --verify --deep --strict --all-architectures`
no pacote **sem abrir/instalar/copiar para Applications**. x64 tem origem unsigned
explícita (SIM31); alteração fora da projeção, inclusive strsize/-p, bloqueia até
conferir delta concreto. A closure do builder seleciona original-zero, ainda a
confirmar no pacote real. Não ampliar máscara ou desligar integridade para passar.
O runtime em execução ainda verifica o próprio bundle/Framework/instância antes
do relógio; build/assinatura isolados não são execução qualificada ou D138.

Nesta frente, SOMENTE Operação instala/copia/abre app no Mac do dono. Conferência
visual fica na sandbox/demo; D138 real é handoff Operação, sem QA do dono. None/null,
helper/produtor PR4, SSH e paridade de contas E chaves antes de retirá-lo continuam;
M1 não entrega P30/conexão genérica/C4 nem libera oferta de produção.

### Segundo ASAR oficial — PR3/SIM37 candidato

O electronDist oficial custom de Darwin44.5.1 conserva default_app.asar e o
builder26.15.3 inclui seu header em ElectronAsarIntegrity. Não remover esse
arquivo para passar o gate nem editar o pacote depois de assinado. O consumidor
exige exatamente app.asar e default_app.asar, algoritmo/hash fechados, o arquivo
extra inteiro oficial (110862B/SHA fixo dos ZIPs admitidos) e FD raw protegido.
O recibo continua somente no app.asar próprio. O digest de integridade inclui
as duas entradas em ordem fixa; máscaras/formatos/assinatura não são ampliados.

Antes do JSON extraído por plutil, a leitura RAW do Info.plist deve rejeitar
ambiguidade sintática. A gramática XML fechada aceita a serialização efetiva do
builder, com limites e sem entidades/DTD extra/duplicatas/alias/binary. plutil
sozinho pode colapsar duplicatas sem erro e não fornece essa prova. Layout
inesperado recusa; não transformar arquivo arbitrário em política admitida.

Root deve repetir pacote local assinado e inspeção completa de arm64 E x64,
sem abrir/instalar/copiar para Applications. Gate exige também positivo do
wholehash oficial e alteração fora do header recusada. O candidato não declara
que esses gates passaram. R36 f692 verde focal permanece histórico; fixture
Linux54 com sampler nativo real não é Darwin, Owner, UI ou D138 instalado.
Só Operação instala/copia/abre appMac; M2/C4 e demais gates continuam obrigatórios.

### VM assinada x64 — PR3/SIM38 candidato

O diagnóstico do pacote x64 encontrou VM LINKEDIT igual à fórmula16K, distinta
da4K; projeção somente em memória confirmou as outras guardas/hashes originais,
sem aceitar ou reescrever o pacote. Após RED real na e482, o candidato exige
exatamente a fórmula4K OU16K com a mesma VM original mínima, sem ampliar os
demais campos normalizados, aceitar intervalo ou inferir quantum do sistema.

Os54 testes e o produtor original permanecem iguais. Root deve repetir os dois
focais R38 e validar executable E Framework assinados REAIS sem projeção ou
alteração dos arquivos, além dos gates de M1 e fresh39. Nenhum resultado pós-fix
presumido, execução qualificada/Owner ou D138 instalado. M2/C4 seguem pendentes;
ninguém desta frente instala/copia/abre appMac, somente Operação.


### Pasta do Hub — D520-B1

O cliente novo usa `~/.mexerica` ou `MEXERICA_HOME` para arquivos locais da ponte
SSH. `userData`, safeStorage, cofre e labels mantêm suas identidades técnicas.
Variáveis de pasta antigas recusam a seleção. O app antigo instalado deve ser
conferido pela Operação antes da troca conjunta com o Hub; este patch não muda
instalação nem movimenta os dados anteriores. Roteiro **NÃO EXECUTADO**.
