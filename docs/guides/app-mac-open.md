# Mexerica Open — teste local no macOS

No diretório `apps/desktop`, use Node 24, `npm ci` e
`npm run dist:mac:open`. O Xcode e o SDK do Mac compilam o helper de fala.
A saída é `dist/open/mac-arm64/Mexerica Open.app` no Apple Silicon e
`dist/open/mac/Mexerica Open.app` no Intel. É um pacote local com assinatura
ad hoc, sem instalador, download ou atualizações.

Abra o pacote pelo caminho acima, sem mover para Aplicativos. A janela do
cliente abre sem conta e não conecta ao Hub, SSH ou a conectores. O pacote leva
o helper de fala, mas a janela de teste não o executa nem pede microfone.

O macOS identifica este pacote por `app.mexerica.open`; ele declara apenas o
esquema `mexerica-open:`. O cliente reserva a porta 19424 como identificador,
mas não abre uma porta nem tenta acessar um serviço local. Ele mantém dados em
`~/Library/Application Support/Mexerica Open` e uma raiz própria
`~/.mexerica-open`. Não importa configurações de outros apps. O nome do
Safe Storage associado à identidade é **Mexerica Open Safe Storage**;
o teste sem conta não chama o Safe Storage nem cria esse item no Chaveiro.

Feche o app antes de apagar suas duas pastas de dados e o próprio pacote. A
separação vale para este cliente de teste; não separa usuários do macOS nem
transforma o Mac em um ambiente para executar código não revisado.
