# Mexerica Mobile

Clientes nativos iOS (SwiftUI) e Android (Kotlin/Compose) para conversar com os bots da Mexerica que rodam no Hub. O app principal se chama **Mexerica**, e a variante de desenvolvimento, **Mexerica Dev**. Os clientes são baseados no Codync.

## iOS

Use macOS com Xcode 26 ou mais recente, XcodeGen e iOS 26 ou superior. O app usa o Liquid Glass nativo. Na raiz do repositório:

```sh
xcodegen generate --spec apps/mobile/project.yml
open apps/mobile/MexericaMobile.xcodeproj
```

O projeto gerado não é versionado. O esquema `iOS` usa `Debug` e `Release`; `iOS Dev` usa `DevDebug` e `DevRelease`. Para testar sem assinatura, selecione `iOS Dev` e um simulador no Xcode. Para executar os testes do pacote Swift, abra `apps/mobile/ios/Kit/Package.swift` e selecione um simulador iOS.

O time de assinatura Apple (`DEVELOPMENT_TEAM`) fica fora do repositório. Configure-o no Xcode após gerar o projeto ou passe-o ao `xcodebuild` pela linha de comando ou com `-xcconfig` apontando para um arquivo `*.local.xcconfig` ignorado pelo Git. O padrão `MEXERICA_SIGNING=free` usa App Groups e Keychain; `MEXERICA_SIGNING=full` exige perfis compatíveis com as capacidades adicionais, como Sign in with Apple e push.

A exceção de rede `ts.net` nos `Info.plist` do iOS e dos Widgets é intencional: permite acessar o Hub pela rede privada Tailscale. Ela não configura um servidor nem substitui o pareamento com o Hub.

## Android

Use JDK 21, Android SDK 37 e Build Tools 37.0.0. Configure `JAVA_HOME` e `ANDROID_HOME` ou defina `sdk.dir` no arquivo local `local.properties`, ignorado pelo Git.

```sh
cd apps/mobile/android
./gradlew :core:test :app:testDebugUnitTest
./gradlew :app:assembleDebug
```

O APK de Debug fica em `app/build/outputs/apk/debug/app-debug.apk` dentro desse diretório. A variante Release não é assinada para distribuição por padrão. O aplicativo Android e seus widgets usam conexão direta; autenticação em nuvem e pareamento exclusivo por relay não estão disponíveis. Veja o [guia Android](../../apps/mobile/android/README.md).

## Configuração local

Os plists versionados em `apps/mobile/shared/Config/main.plist` e `apps/mobile/shared/Config/dev.plist` ficam neutros: nenhum serviço hospedado, endpoint ou chave vem preenchido. Se sua instalação precisar de configuração, crie `main.local.plist` ou `dev.local.plist` no mesmo diretório. Esses arquivos são ignorados pelo Git e só substituem valores do próprio ambiente. Use XML e apenas valores públicos da sua instalação, pois o app os incorpora no pacote. Não coloque credenciais, tokens ou chaves privadas nesses arquivos.

No iOS, a variante Main exige configuração de conta no Release e em builds para aparelho. Sem configuração, o pareamento por QR está disponível em Dev e em Main Debug no simulador. A nuvem continua desativada em todas as variantes Android mesmo com overrides locais. Push remoto depende de configuração explícita; no Android também exige configuração Firebase.

Para conferir o seletor iOS sem alterar o transporte, defina `workspaceDirectoryEnabled` como booleano verdadeiro no plist local. Nesse modo, a lista usa as memberships do Clerk; Pessoal aparece apenas quando o metadado público `hasPersonal` é `true` para a conta. Essa chave é uma entrada de teste administrada no backend, não um vínculo com o Hub. Até a integração de descoberta, pareamento e autorização do Hub por workspace, o seletor não abre conversas e não usa a lista de computadores do modo legado. Deixe o campo ausente na configuração normal.

## Compatibilidade com o Hub

`CodyncKit`, `CodyncUI`, os pacotes `com.codync.*` e os rótulos do protocolo foram preservados. iOS e Android usam o mesmo [fixture de vetores do relay](../reference/fixtures/remote-relay-vectors.json) que o Hub; alterar os vetores, os rótulos HKDF ou a assinatura `Codync-Sig` pode quebrar a compatibilidade.

Veja também [regras de contribuição](../../AGENTS.md), [licenças e atribuições](../../THIRD_PARTY.md), [modificações em relação ao Codync](../../MODIFICACOES.md) e [avatares dos bots](avatares-dos-bots.md).
