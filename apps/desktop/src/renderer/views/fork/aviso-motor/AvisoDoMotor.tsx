import { superficieIdioma, useIdioma, tradutor } from '../../../lib/fork-i18n/estado'
import { avisoDoMotor, textosSemAprovacao } from './aviso-motor'
import './aviso-motor.css'

/**
 * Nota abaixo da escolha de motor (Novo bot e editor do bot). Só informa: não bloqueia nem troca o motor.
 * `semAprovacao`: o Hub marcou o motor como "age sem pedir aprovação"; a nota explica o que isso quer dizer.
 */
export function AvisoDoMotor({ backend, semAprovacao = false }: { backend: string; semAprovacao?: boolean }) {
  const idioma = useIdioma()
  const aviso = avisoDoMotor(backend)
  if (!aviso && !semAprovacao) return null
  return (
    <>
      {aviso ? <p className="aviso-motor idioma-superficie" role="note" {...superficieIdioma(idioma)}>{tradutor(idioma)(aviso)}</p> : null}
      {semAprovacao ? <p className="aviso-motor idioma-superficie" role="note" {...superficieIdioma(idioma)}>{textosSemAprovacao(idioma).texto}</p> : null}
    </>
  )
}
