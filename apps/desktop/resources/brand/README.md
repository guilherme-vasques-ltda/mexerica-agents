# Mexerica — ativos da marca do app

Ativos que o app usa. **Ícone do app (D457/D464):** o ícone do pacote oficial do dono (PR #542,
[`brand/assets/logo/`](../../../../brand/assets/logo/LEIA-ME.txt): mexerica de feltro sobre laranja), copiado byte a
byte; é o único ícone do app, sem variações nem seletor. Origem e SHA-256 de cada arquivo:
[`brand/assets/icon/app/README.md`](../../../../brand/assets/icon/app/README.md).
**Barra de menus (D452):** a mexerica de rostinho. A assinatura e a logo horizontal da Donna Bot foram
aposentadas com a arte nova; o pacote antigo vive em `docs/assets/brand/` do projeto de origem (fora desta cópia). Guia:
docs/guides/marca.md (não faz parte desta cópia).

Nomes de arquivo, pacotes e o certificado ("Donnabot Local") são técnicos e não mudam com a marca.

**Uso destes arquivos.** O ícone do app, a logo, o mascote, o símbolo, os favicons e o ícone da barra de menus não são licenciados sob
Apache-2.0: a Mexerica reserva os direitos que tiver e não concede licença de reuso fora dela; o uso permitido é dentro da Mexerica como distribuída, sem reuso em outros produtos (decisão do dono,
19/11/2026). A regra completa e a lista de caminhos estão em [`brand/LICENCA-DA-ARTE.md`](../../../../brand/LICENCA-DA-ARTE.md). Não é parecer jurídico.

## Arquivos

- `Donnabot.icns`: ícone do app no build local do Mac (`tools/local-builder.cjs`), o `05-icone-app/macos/mexerica.icns`
  do pacote, byte a byte (D464). O nome do arquivo é técnico e não mudou.
- `barra-menus.svg` e `barra-menus-alerta.svg`: glifo da barra de menus da Mexerica (D452, opção 3): a mexerica
  achatada com cabinho, folha e rostinho vazado (olhos e sorriso), numa grade de 36. O de alerta tem um ponto no
  canto superior esquerdo com folga transparente (o direito é da folha). A reserva, opção 1 (silhueta cheia), está
  em `brand/assets/icon/app/barra-opcao-1-reserva/`, com SVG e PNG prontos.
  Geram os PNG template (preto e alfa) de `../tray/`:

  ```sh
  rsvg-convert -w 18 -h 18 -o ../tray/trayTemplate.png barra-menus.svg
  rsvg-convert -w 36 -h 36 -o ../tray/trayTemplate@2x.png barra-menus.svg
  rsvg-convert -w 18 -h 18 -o ../tray/trayAlertTemplate.png barra-menus-alerta.svg
  rsvg-convert -w 36 -h 36 -o ../tray/trayAlertTemplate@2x.png barra-menus-alerta.svg
  ```

Fora desta pasta:

- Os favicons do pacote (16, 32, 180, 192 px e `favicon.ico` com 16/32/48) ficam em
  `src/renderer/assets/brand/favicons/`, dentro da raiz do renderer. O `href` de um `<link>` no
  `index.html` só resolve no `npm run dev` se o arquivo estiver ali.
- `../icon.png`: `05-icone-app/macos/icone-macos-1024.png` do pacote (grade do Mac, forma de 824 em 1024, com sombra).
  Linux, Windows, o Dock em desenvolvimento e a página Ajustes → Ícone do app (só informativa, D457).
- `../icon.svg`: SVG que só embute o `05-icone-app/macos/icone-macos-128.png` do pacote (sem vetorização): o ícone
  das telas do app (Acesso ao computador).
- `../AppIcon.icon` (Icon Composer, usado só pelo build de distribuição): a camada `Assets/image 9.png` é o
  `05-icone-app/icone-mestre-1024.png` do pacote (arte quadrada, opaca, sem cantos), que é o que o Icon Composer
  espera: ele aplica a máscara, a margem e a sombra neutra do grupo. O `fill` do `icon.json` é o laranja do canto
  dessa arte, `srgb:0.97647,0.40000,0.00000` (249, 102, 0). Conferir no `actool` do Xcode antes de uma distribuição.
- O `.icns` do pacote traz 128, 256, 512 e 1024 e as versões @2x de 16, 32, 128 e 256 (não tem 16 e 32 em @1x; o
  macOS reduz a partir dos maiores). O teste `src/shared/fork-icone-mexerica.test.ts` confere essas entradas e os
  SHA-256 de cada arquivo do pacote.
- Favicons do renderer e do site: `06-web-e-redes/` do pacote, byte a byte (lista no README da pasta de marca).
