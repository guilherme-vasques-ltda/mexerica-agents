# Ícone do app da Mexerica: onde fica a fonte (D464)

**Fonte do ícone do app, dos favicons e do apple-touch:** o pacote oficial do dono, PR #542, em
[`brand/assets/logo/`](../../logo/LEIA-ME.txt) (`05-icone-app/` e `06-web-e-redes/`). O app e o site usam esses
arquivos **byte a byte** (D464, Pergunta 107 = A); nada é gerado nem redesenhado. Os derivados do #520 (iconset da
opção A, favicons e camadas) saíram, porque não têm mais uso.

| origem no pacote | destino | sha256 |
|---|---|---|
| `brand/assets/logo/05-icone-app/macos/mexerica.icns` | `apps/desktop/resources/brand/Donnabot.icns` | `c931ee20b83fbff302b4993d4877dcdd4391d584132dc9fac0d88fcb266f20b3` |
| `brand/assets/logo/05-icone-app/macos/icone-macos-1024.png` | `apps/desktop/resources/icon.png` | `008dde7f90e7912c0c3f7e36ee42d23dc57c974a82781364a35b7b9bbfeaddd2` |
| `brand/assets/logo/05-icone-app/icone-mestre-1024.png` | `apps/desktop/resources/AppIcon.icon/Assets/image 9.png` | `a8bc2e3c047ccb3fa68bbdca43da1742c77f807873247a936dd5fbfddc8cade1` |
| `brand/assets/logo/06-web-e-redes/favicon.ico` | `apps/desktop/src/renderer/assets/brand/favicons/favicon.ico` | `ce5b34e7bb65325f32ae04b3b5940c7bfae7dc117fa9f47d24f0fb2e6f5e01ff` |
| `brand/assets/logo/06-web-e-redes/favicon-16.png` | `apps/desktop/src/renderer/assets/brand/favicons/favicon-16.png` | `3aa50101b0d1c2f3f533ace227501dc1df51c54f04aaba9003cf4d79abedbcf8` |
| `brand/assets/logo/06-web-e-redes/favicon-32.png` | `apps/desktop/src/renderer/assets/brand/favicons/favicon-32.png` | `8002bf845d8d05979928c72a8b37b41aa9a50b95e02c2677f5c6eec64bb01c91` |
| `brand/assets/logo/06-web-e-redes/apple-touch-icon-180.png` | `apps/desktop/src/renderer/assets/brand/favicons/favicon-180.png` | `436a8c838c42f63dca7d86295a493a303fc7eae144ef07e9067a3b87d1a7f249` |
| `brand/assets/logo/06-web-e-redes/icone-app-web-192.png` | `apps/desktop/src/renderer/assets/brand/favicons/favicon-192.png` | `362e93b13271a7d3eace38458af274fe3e8af6b3c80cda3005d3c45552307075` |
| `brand/assets/logo/06-web-e-redes/favicon.ico` | `web/app/favicon.ico` | `ce5b34e7bb65325f32ae04b3b5940c7bfae7dc117fa9f47d24f0fb2e6f5e01ff` |
| `brand/assets/logo/06-web-e-redes/favicon.svg` | `web/public/icon.svg` | `5451337fd0904ca2d97102ccae47428db3e4cfc6f0162af5467850d77f662355` |
| `brand/assets/logo/06-web-e-redes/icone-app-web-512.png` | `web/public/icon.png` | `a3a8e77bb01af8120a32498dd355b0107e5953a7d13e6d199ccfd8b9f9faa83f` |
| `brand/assets/logo/06-web-e-redes/apple-touch-icon-180.png` | `web/public/apple-touch-icon.png` | `436a8c838c42f63dca7d86295a493a303fc7eae144ef07e9067a3b87d1a7f249` |
| `brand/assets/logo/05-icone-app/macos/icone-macos-128.png (embutido)` | `apps/desktop/resources/icon.svg` | `f000a79e557e5b5721a1ef6140ba81e0c11be7c73f5aca6f35e9812c670e3bfc` |

O `apps/desktop/resources/icon.svg` é um SVG que só embute, sem vetorização, o PNG de 128 px do pacote
(`05-icone-app/macos/icone-macos-128.png`). O teste `apps/desktop/src/shared/fork-icone-mexerica.test.ts` confere
todos os SHA-256 acima.

## Barra de menus (D452)

A barra usa a mexerica de rostinho (opção 3): fontes em `apps/desktop/resources/brand/barra-menus.svg` e
`barra-menus-alerta.svg`, PNG em `apps/desktop/resources/tray/`. A **reserva** (opção 1, silhueta cheia) fica em
[`barra-opcao-1-reserva/`](barra-opcao-1-reserva/), com SVG e PNG prontos, se os olhinhos não ficarem legíveis no
app instalado.
