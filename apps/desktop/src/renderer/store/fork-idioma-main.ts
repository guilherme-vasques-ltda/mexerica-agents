// D454/D495 (R1 do #563): o idioma efetivo do app também vai ao processo principal, que desenha o menu do Mac, o
// painel Sobre e a barra de menus. Vai ao abrir e a cada troca. Sem import de prefs/DOM: a entrada do renderer
// (`main.tsx`) registra a fonte (o idioma atual) e o envio (a ponte).
import type { AppLanguage } from '../../shared/fork-idioma.ts'

/** Avisa o main agora e a cada troca. Devolve como parar. */
export function registrarIdiomaNoMain(atual: () => AppLanguage, aoTrocar: (aviso: () => void) => () => void, enviar: (idioma: AppLanguage) => void): () => void {
  const avisar = () => enviar(atual())
  avisar()
  return aoTrocar(avisar)
}
