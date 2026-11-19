# Mexerica Open — teste local no macOS

No diretório `apps/desktop`, use Node 24, `npm ci` e
`npm run dist:mac:open`. O Xcode e o SDK do Mac compilam o helper de fala.
A saída é `dist/open/mac-arm64/Mexerica Open.app` no Apple Silicon e
`dist/open/mac/Mexerica Open.app` no Intel. É um pacote local com assinatura
ad hoc, sem instalador, download ou atualizações.

Abra o `.app` pelo Finder ou com `open`, sem mover para Aplicativos.
Confirme que a janela apareceu: em um teste num Mac limpo, a primeira
tentativa não iniciou o app e não mostrou erro; duas aberturas posteriores
funcionaram. Não execute o binário dentro do pacote para tentar abrir de novo:
isso deixou um segundo processo sem janela no teste.

A janela do cliente abre sem conta e não conecta ao Hub, SSH ou a conectores.
O pacote leva o helper de fala, mas a janela de teste não o executa nem pede
microfone.

O macOS identifica este pacote por `app.mexerica.open`; ele declara apenas o
esquema `mexerica-open:`. O cliente usa a porta 19524 como identificador,
mas não abre uma porta nem tenta acessar um serviço local. Ele mantém dados em
`~/Library/Application Support/Mexerica Open` e uma raiz própria
`~/.mexerica-open`. Não importa configurações de outros apps. O nome do
Safe Storage associado à identidade é **Mexerica Open Safe Storage**;
o teste sem conta não chama o Safe Storage nem cria esse item no Chaveiro.

Encerre pela janela ou com Command+Q. Em caso de saída forçada, o cliente
pode recusar a reabertura se encontrar arquivos de trava antigos; não use
outro perfil para contornar a recusa. Para limpar o teste, feche o app e
apague só as duas pastas da Open e o próprio pacote. O macOS pode manter
metadados próprios de `app.mexerica.open`, como preferências, caches, estado
da janela ou o registro do esquema; eles não pertencem aos outros apps.
A separação não isola usuários do macOS nem torna seguro executar código
não revisado.
