# Brand Book da Mexerica

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../assets/brand/mexerica/mexerica-logo-escuro.svg">
  <img src="../assets/brand/mexerica/mexerica-logo-claro.svg" width="320" alt="Mexerica: o nome em letras arredondadas, com uma mexerica sorridente no pingo do i">
</picture>

Logo plana da Mexerica, do pacote oficial do dono ([`brand/assets/logo/`](../../brand/assets/logo/LEIA-ME.txt), pasta `02-logo-plana`).

Tudo da identidade da Mexerica num lugar só: o pacote oficial de logo e ícone do dono, as cores, as fontes, a voz, o ícone do app e suas variações, os fundos, os avatares, as peças para redes e o sistema visual do desktop.

| Preciso de… | Onde está |
| --- | --- |
| **Logo e ícone oficiais da Mexerica** | [`brand/assets/logo/`](../../brand/assets/logo/LEIA-ME.txt): pacote do dono, com logo com mascote, logo plana, símbolo, ícone do app e peças para web/redes |
| Logo e símbolo da identidade anterior (Grafite) | `docs/assets/brand/`: `icone/` e `logo-horizontal/` |
| PNG e favicons da identidade anterior (Grafite) | `docs/assets/brand/png/` |
| Foto de perfil e capa da identidade anterior (Grafite) | `docs/assets/brand/redes/` |
| Ícone do app do Mac (`.icns`) e recorte da barra de menus | [`apps/desktop/resources/brand/`](../../apps/desktop/resources/brand/README.md) |
| Variações do ícone do app para o editor | `apps/desktop/design/assets/Icones/` |
| Fundos de space | [Fundos e avatares](#fundos-e-avatares) |
| Propostas de fundos com a marca (05/11/2026) | Cinco cenas aprovadas e integradas no catálogo; Noite azul é o padrão somente sem escolha salva |
| Tokens (cores, tipo, espaço, raio, vidro) | [`apps/desktop/design/tokens.json`](../../apps/desktop/design/tokens.json) → `tokens.css` gerado |
| Sistema visual, componentes e protótipos | [`apps/desktop/design/tokens.json`](../../apps/desktop/design/tokens.json) → `tokens.css` gerado |
| Avatares dos bots | `@donnabot/avatars` em `apps/desktop/src/renderer/lib/donnabot-avatars/` |

## Novo nome: Mexerica e Berga

O app passa a se chamar **Mexerica** e o bot principal, **Berga** (antes
"Donna Bot" e "Donna"). Grafia e artigo: "Mexerica" e
"Berga", só com a inicial maiúscula (nunca "MEXERICA" ou "MexeRica"), sempre com artigo: "a Mexerica", "Sobre a
Mexerica", "o Berga" (masculino). O app de desenvolvimento se chama "Mexerica Dev". Nos 6 idiomas, "Mexerica" e "Berga" são
nomes próprios iguais, em alfabeto latino. Na tela "Sobre", aparece "Mexerica" com uma
linha pequena "baseado no Codync", e a atribuição em `LICENSE` e `NOTICE` fica intacta. O principal fixo chamado "Donna" vira "Berga"
sozinho na atualização, sem mudar conversas, memória e configurações; um nome dado à mão fica. "Donna" fica livre para
bots novos.

O que muda e quando:

- **Documentação pública:** já usa Mexerica e Berga.
- **Textos do app:** já dizem Mexerica e Berga nos 6 idiomas, inclusive o menu do Mac ("Sobre a
  Mexerica", com "baseado no Codync" no painel Sobre), a barra de menus, a ilha e a primeira abertura.
  **Textos do Hub** (nome gravado da principal, base, manual e mensagens): acompanham o mesmo pacote
  instalado.
- **Nome do app instalado:** o pacote instalado se chama **Mexerica** (Dev: **Mexerica Dev**). O título em
  negrito do menu, o Dock, o Finder, as notificações e os Ajustes do Sistema (Microfone, Itens de Início) mostram
  "Mexerica". A troca para quem já tem o Donnabot.app, com rollback, está em [App no Mac](app-mac-local.md).
- **Ícone e logo:** o ícone do app e os favicons são os do pacote oficial, e a conta usa a logo com o
  mascote. A assinatura em curvas "Donna Bot" só sobrevive na apresentação de abertura, até a versão própria
  da Mexerica.
- **Não mudam:** nomes técnicos (pacotes, appId/bundle, pasta de dados, item do Chaveiro,
  esquema `donnabot://`, variáveis `DONNABOT_*`, nome do aparelho na nuvem, identidade "Donnabot Local"), nomes de
  arquivos de docs e ativos (`donnabot-*.svg`, `Donnabot.icns`), copyrights e docs históricas. A atribuição ao Codync
  em `LICENSE` e `NOTICE` fica intacta. O site fica em repositório separado; o nome do cliente OAuth só muda com OK do dono.

## A marca

A marca oficial atual é o pacote do dono em [`brand/assets/logo/`](../../brand/assets/logo/LEIA-ME.txt)
(logo com o mascote no pingo do i, logo plana, símbolo e ícone do app, cores Laranja `#F58A1F`,
Folha `#5C9B3A`, Marrom `#2A1C14` e Creme `#FFF8EE`). A seção abaixo descreve a identidade
anterior, Grafite, que substituiu a Donna
ilustrada e trocou só as cores e a palavra. Fica como registro: os vetores e PNG
dela seguem em `docs/assets/brand/` e ilustram decisões antigas, não a marca atual.
As subseções de variações, regras, paleta e assinatura em curvas abaixo são
registro histórico dessa identidade anterior — não são instrução de uso da
marca atual, que é o pacote oficial do dono.

- **Símbolo:** quadrado grafite `#13201F` de cantos arredondados com um "d" marfim `#FDFBF7`. O bojo do "d"
  é um rosto, com olho redondo marfim e piscadinha verde `#2DD4BF`.
- **Assinatura:** a palavra "Donna Bot" em Montserrat ExtraBold, convertida em curvas (D
  maiúsculo e o resto minúsculo). Peso 800, entreletra −5 no corpo 200, a mesma altura
  de maiúscula de antes; são as mesmas curvas da assinatura antiga da tela de entrada.

O **logo horizontal** junta o símbolo e a assinatura. Os vetores são uma reconstrução geométrica limpa
do conceito escolhido (opção 01), em cores sólidas. A assinatura não se redigita: use as curvas dos arquivos.
A antiga, em roxo `#461DFB` com piscadinha lilás e a palavra "Donnabot", saiu da marca. O roxo era uma das
cores do ícone do app (Violeta).

### Variações da identidade anterior (registro histórico)

Cada variação da identidade Grafite existe como símbolo (`icone/`) e como logo horizontal (`logo-horizontal/`), com o
mesmo nome de arquivo (`donnabot-<variação>.svg`).

| Variação | Quando usar |
|---|---|
| `cor-transparente` | Versão principal, em superfícies claras: símbolo grafite e palavra em grafite `#13201F`, sem fundo externo. |
| `cor-transparente-escuro` | Superfícies escuras: o mesmo símbolo, com a palavra em marfim. No símbolo sozinho é idêntica à `cor-transparente`. |
| `claro-com-fundo` | Quando a peça precisa trazer o próprio fundo claro (marfim), com margem de proteção. |
| `escuro-com-fundo` | O mesmo com fundo escuro `#0C1716` (a noite do site). |
| `preto-transparente` | Uma tinta só (preto), para impressão monocromática. O "d", o olho e a piscadinha são vazados e mostram a superfície de trás. Também é a base da barra de menus do Mac. |
| `branco-transparente` | A versão de uma tinta para superfícies escuras. |
| `preto-com-fundo-branco` / `branco-com-fundo-preto` | Monocromia com superfície opaca, quando a peça não pode depender do fundo. |

<img src="../assets/brand/icone/donnabot-cor-transparente.svg" width="96" alt="Símbolo da identidade anterior (Grafite) em cores">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../assets/brand/icone/donnabot-branco-transparente.svg">
  <img src="../assets/brand/icone/donnabot-preto-transparente.svg" width="96" alt="Símbolo da identidade anterior (Grafite) em uma tinta">
</picture>

<img src="../assets/brand/logo-horizontal/donnabot-claro-com-fundo.svg" width="300" alt="Logo horizontal da identidade anterior (Grafite), claro com fundo">
<img src="../assets/brand/logo-horizontal/donnabot-escuro-com-fundo.svg" width="300" alt="Logo horizontal da identidade anterior (Grafite), escuro com fundo">

<img src="../assets/brand/logo-horizontal/donnabot-preto-com-fundo-branco.svg" width="300" alt="Logo horizontal da identidade anterior (Grafite), preto com fundo branco">
<img src="../assets/brand/logo-horizontal/donnabot-branco-com-fundo-preto.svg" width="300" alt="Logo horizontal da identidade anterior (Grafite), branco com fundo preto">

### Regras da identidade anterior (registro histórico)

- Mantenha as proporções. Não distorça a marca e não recolora a versão principal dela (a Grafite).
- Em fundo escuro o quadrado grafite quase some (1,14:1 contra `#071110`): quem carrega a marca é o "d" marfim (16,2:1 sobre o quadrado). Não acrescente contorno nem troque o quadrado por conta própria.
- Prefira SVG; use PNG só onde o destino não aceita SVG (redes sociais, favicons, lojas). Os PNG de `png/` são exportados dos mesmos vetores, sem retoque.
- Monocromia é só `#000000` ou `#FFFFFF`.
- Não redigite a palavra com a fonte: use a assinatura em curvas dos arquivos.
- Deixe em volta do logo uma folga de pelo menos a altura do "d"; as versões `com-fundo` já trazem essa margem.
- O símbolo não vira avatar de bot: avatares são os rostos do `@donnabot/avatars`.

## Cores

Cores da identidade anterior Grafite (histórico):

| Cor | Hex | RGB | Papel |
|---|---|---|---|
| Grafite | `#13201F` | 19, 32, 31 | Fundo do símbolo; palavra em superfícies claras |
| Marfim | `#FDFBF7` | 253, 251, 247 | O "d" e o olho; superfície clara com fundo; palavra em superfícies escuras |
| Verde | `#2DD4BF` | 45, 212, 191 | A piscadinha (9,0:1 sobre o grafite) |
| Noite | `#0C1716` | 12, 23, 22 | Superfície escura com fundo (`escuro-com-fundo`) |

Contraste (identidade anterior): "d" marfim sobre o quadrado, 16,2:1; palavra grafite sobre branco, 16,75:1; quadrado contra fundo escuro
`#071110`, 1,14:1 (por isso o "d" carrega a marca no escuro).

O destaque padrão do app é **Mexerica**, o laranja-tangerina do Berga,
para quem não tem uma escolha salva. Quem já escolheu qualquer cor, inclusive
Verde ou Violeta (padrões anteriores), mantém a escolha sem migração.

Mexerica e Folha abrem a lista em Ajustes → Aparência, seguidas de Verde, Violeta,
Azul, Jade, Âmbar, Rosa e Grafite. Partem do laranja `#F58A1F` e do verde `#5C9B3A`
do símbolo plano da Mexerica, com tons adaptados ao contraste por tema:

- **Mexerica:** `#b44900` no claro e `#f58a1f` no escuro.
- **Folha:** `#36751f` no claro e `#85c65b` no escuro.

As duas cores passam 5,3:1 com `on-accent` e 4,5:1 sobre `canvas` e `surface`.
O Verde da marca Grafite continua disponível (`#0c706e` no claro / `#2dd4bf` no
escuro); os tokens base `accent` continuam guardando a Violeta. Neutros, vidros e
contraste contextual seguem o sistema visual do desktop.

## Tipografia

- **Assinatura da identidade anterior:** Montserrat ExtraBold, de Julieta Ulanovsky e colaboradores, sob a
  [SIL Open Font License 1.1](https://openfontlicense.org)
  ([fonte e licença](https://github.com/JulietaUla/Montserrat)). Vem convertida em curvas: não
  precisa da fonte instalada, e o arquivo da fonte não é redistribuído neste repositório.
- **Boas-vindas:** títulos em Montserrat SemiBold instalada no Mac; sem ela, SF Pro Display. O app não leva a fonte.
- **Interface do app:** a pessoa escolhe em Ajustes → Aparência → Fonte entre Sistema (SF Pro), Inter, Geist, Manrope, IBM Plex Sans e DM Sans. **Padrão: DM Sans**. As cinco fontes são OFL e vão embutidas no app, com procedência em `THIRD_PARTY.md`.
- **Escala:** `large-title` 26, `title` 22, `title-2` 17, `title-3` 15, `body` 13/19, `row` 13/16, `callout` 12, `section` 12 semibold, `caption` 11 (o piso). Em [tokens](../../apps/desktop/design/tokens.json).

## Voz

Português do Brasil, frases curtas, tratamento por **você**, caixa de frase ("Nova equipe", nunca "NOVA EQUIPE").
Bots falam como gente prestativa, sem exclamação de entusiasmo nem emoji decorativo. Canais com `#` e
minúsculas; equipes pelo nome. Detalhes no guia de conteúdo e voz do sistema visual.

**Nome do produto nos textos do app:** tudo o que a pessoa lê diz **Mexerica** (o app,
"a Mexerica") e **Berga** (o bot principal, "o Berga"): telas, avisos, a ilha, o menu da barra e o título da janela
(`shared/fork-nome-visivel.ts`). Os nomes são iguais nos 6 idiomas.
O `productName` (o nome do pacote, que o macOS mostra no Dock, no Finder e nos Ajustes do Sistema) é
"Mexerica". Os nomes técnicos herdados "Donnabot" também migram para "Mexerica" —
`app.setName` (o que `app.getName()` devolve), bundleId, pasta de dados, o item do Keychain "Donnabot Safe Storage",
o nome do aparelho na nuvem, o app auxiliar "Donnabot Screen", os binários `donnabot-*`, as chaves `donna_*`, os
esquemas `donnabot://`, os pacotes `@donnabot/*` e os bundle ids `app.donnabot.*`. A troca é sempre com migração dos
dados de quem já instalou, nunca rename seco. Até cada etapa chegar, os nomes
atuais continuam valendo e o teste `shared/fork-nome-pacote.test.ts` os trava. A guarda `renderer/lib/textos-marca.test.ts`
recusa "Donna Bot", "DONNA Bot", "Donna Apps" e "a Donna" como bot principal no renderer, no main e no shared (e
"Donnabot" num texto do renderer), além de "Donna" em qualquer valor dos 6 catálogos, com as exceções técnicas listadas
nela; as exceções caem junto de cada etapa de migração. Os códigos e chaves (`donna_only`, `isDonna`,
`botSettings.donnaFixa.*`) migram na etapa de chaves/contratos.

## Ícone do app

O ícone do app do Mac é o do pacote oficial do dono ([`brand/assets/logo/`](../../brand/assets/logo/LEIA-ME.txt):
a mexerica de feltro sobre laranja), copiado byte a byte para `Donnabot.icns`, `icon.png` e o `AppIcon.icon` (nomes
técnicos mantidos até a etapa de migração; origem e SHA-256 em [`brand/assets/icon/app/`](../../brand/assets/icon/app/README.md)). Os favicons do
app e do site são os do mesmo pacote (`06-web-e-redes/`, a mexerica plana, que lê melhor em 16 px). O ícone do pacote continua o
principal: Ajustes → Ícone do app oferece o padrão e as opções prontas das rodadas 2 e 3 da mascote, aplicadas
só neste aparelho ([guia](icone-local.md)); as variações do "d" e o `app-icon-v1.json` antigo seguem fora. Os SVG das
variações em `apps/desktop/design/assets/Icones/` ficam só como registro
de design.
A barra de menus usa o glifo da Mexerica (opção 3): a fruta com cabinho,
folha e rostinho vazado, em template preto (`barra-menus.svg`). A reserva, se os olhinhos não ficarem legíveis no app
instalado, é a opção 1, em `brand/assets/icon/app/barra-opcao-1-reserva/`.

## Fundos e avatares

- **Fundos de space:** paisagens e escritório (serra, duna, maré, noite, cidade, risca de giz, mogno), cada um com
  versão noturna, integrados ao catálogo de fundos do app. Regras de desfoque, véu e
  piso de legibilidade fazem parte do sistema visual do desktop.
- **Avatares:** todo bot é um rosto do `@donnabot/avatars` (13 formatos, 7 olhos, 22 acessórios, 36 selos, 30 funções
  prontas), com humor vindo do estado no Hub. Rostos abaixo de 56px saem sem o selinho do canto
  (componente TeamGroup).

## Redes sociais

As peças oficiais da marca atual estão em [`brand/assets/logo/06-web-e-redes/`](../../brand/assets/logo/06-web-e-redes/):
foto de perfil com o mascote ou com a mexerica plana (1080 × 1080), capa de compartilhamento
(1200 × 630), favicons e ícones PWA. As peças da identidade anterior ficam em
`docs/assets/brand/redes/`, exportadas dos vetores dela:

- **Foto de perfil** 1200 × 1200: roxo com o "d" marfim (principal) ou marfim com o símbolo. O "d" tem folga para o corte redondo.
- **Capa do X** 3000 × 1000 (o dobro de 1500 × 500): azul-noite com brilho roxo e o logo colorido, ou roxo com o logo branco. O logo fica no centro, longe do canto onde o X encaixa a foto.

## Onde a marca está aplicada

- **App do Mac:** ícone do app, barra de menus, logo nas boas-vindas, símbolo no acesso ao
  computador, favicons e cor de destaque. Os arquivos ficam em `apps/desktop/resources/brand/`, que
  tem um README.
  - O build local (`npm run dist:mac:local`) usa o `Donnabot.icns` do pacote.
  - Menu do app no macOS: "Sobre a Mexerica", "Ocultar Mexerica" e "Encerrar Mexerica", com os papéis do
    sistema (`shared/fork-menu-app.ts`). O painel Sobre mostra "Mexerica" com a linha pequena "baseado no Codync"
    (`painelSobre`). O título em negrito da barra e o nome no Dock e no Finder vêm do `CFBundleName`, que é
    "Mexerica".
  - Barra de menus: "Abrir a Mexerica", "Sair da Mexerica" e o tooltip "Mexerica" (`sistema.barra.abrir`/`sair` no catálogo, com o nome visível como parâmetro).
  - Primeiro uso: no passo de idioma, a mexerica do pacote oficial (`04-mascote`); na conta, a logo com o
    mascote, nas versões clara e escura. Chão Creme/Noite, cartões do pacote, e Continuar e Entrar no Laranja com
    texto Marrom. A apresentação de abertura ainda
    mostra a identidade anterior (o "d" de partículas e a assinatura em curvas) até a versão da Mexerica.
  - A barra de menus usa o glifo da Mexerica com rostinho, em template preto.
- **Host:** não exibe marca gráfica.
- **Repositório:** o README e esta página.
- **Redes:** X, com as peças oficiais de `brand/assets/logo/06-web-e-redes/`.

- **Build de distribuição:** `apps/desktop/resources/AppIcon.icon` (Icon Composer) tem fundo laranja sólido
  (`srgb:0.976, 0.40, 0`) e uma única camada de imagem, sem vidro nem translucidez.

Fora do app de desktop: Android e o site, em repositório separado; o mobile vem depois. O ícone do iPhone
está fora deste repositório.

## Pacote da marca

O pacote oficial atual é o do dono, em [`brand/assets/logo/`](../../brand/assets/logo/LEIA-ME.txt):
logo com mascote e logo plana em SVG, PDF e PNG (grande 2400, médio 1200 e pequeno 600 px), símbolo,
ícone do app (`05-icone-app/`, com `mexerica.icns`) e peças para web e redes (`06-web-e-redes/`).
O pacote da identidade anterior fica em `docs/assets/brand/`: vetores em `icone/` e
`logo-horizontal/`; PNG de 256, 512 e 1024 px do símbolo, logo horizontal em 2753 × 816 px, favicons e o
símbolo para o painel do Clerk (`png/clerk/`, 512 e 1024 px) em `docs/assets/brand/png/`;
peças de redes em `docs/assets/brand/redes/`. Os PNG eram exportações diretas dos SVG (Chromium, sem
retoque). Os ativos atuais do app ficam em `apps/desktop/resources/brand/`: `Donnabot.icns` (nome técnico
do ícone oficial do pacote) e o recorte da barra de menus.
