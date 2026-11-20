# Mexerica Open — teste local no macOS

No diretório `apps/desktop`, use Node 24, `npm ci` e
`npm run dist:mac:open`. O pacote usa o Xcode, o SDK do Mac e a versão do Rust
fixada em `rust-toolchain.toml` para compilar os binários da arquitetura local.
A saída é `dist/open/mac-arm64/Mexerica Open.app` no Apple Silicon e
`dist/open/mac/Mexerica Open.app` no Intel. É um pacote local com assinatura
ad hoc, sem instalador, download ou atualizações.

Abra o `.app` pelo Finder ou com `open`, sem mover para Aplicativos.
Confirme que a janela apareceu: em um teste num Mac limpo, a primeira
tentativa não iniciou o app e não mostrou erro; duas aberturas posteriores
funcionaram. Não execute o binário dentro do pacote para tentar abrir de novo:
isso deixou um segundo processo sem janela no teste.

A janela abre sem conta da Mexerica e inicia o Hub incluído no pacote. O Hub
encerra com o app e não instala um serviço do macOS nem se conecta a um relay.
Esta versão ainda não executa motores: a Open não usa contas, executores ou
credenciais de outros perfis. WhatsApp e a seleção de pastas externas
também ficam indisponíveis. O fluxo de criação de Space pela interface
exige uma conta de motor, por isso termina na seleção da conta. O pacote
leva o helper de fala, mas a janela não o executa nem pede microfone.

O macOS identifica este pacote por `app.mexerica.open`; ele declara apenas o
esquema `mexerica-open:`. O Hub reserva a porta 19524 na interface local,
sem expor a API por TCP. A janela usa um socket Unix privado do processo
que ela iniciou. O perfil da janela fica em
`~/Library/Application Support/Mexerica Open`; o Hub roda com uma pasta
pessoal própria em `~/.mexerica-open/home`, sem importar configurações ou
credenciais de outros apps. O cofre da Open reserva o serviço de Chaveiro
`app.mexerica.open.credentials`, mas está desativado nesta versão. A janela
não usa Safe Storage.

Em ambiente de testes Unix sem o app aberto, use
`cd host && CODYNC_ENV=open CARGO_PROFILE_DEV_DEBUG=0 cargo test --test open_hub`
para testar o Hub isolado.

Encerre pela janela ou com Command+Q. Em caso de saída forçada, o cliente
pode recusar a reabertura se encontrar arquivos de trava antigos; não use
outro perfil para contornar a recusa. Para limpar o teste, feche o app e
apague só as duas pastas da Open e o próprio pacote. O macOS pode manter
metadados próprios de `app.mexerica.open`, como preferências, caches, estado
da janela ou o registro do esquema; eles não pertencem aos outros apps.
A separação não isola usuários do macOS nem torna seguro executar código
não revisado.
