import { superficieIdioma, useIdioma, tradutor } from '../../../lib/fork-i18n/estado'
import { avisoDoMotor } from './aviso-motor'
import './aviso-motor.css'

/** Nota abaixo da escolha de motor (Novo bot e editor do bot). Só informa: não bloqueia nem troca o motor. */
export function AvisoDoMotor({ backend }: { backend: string }) {
  const idioma = useIdioma()
  const aviso = avisoDoMotor(backend)
  return aviso ? <p className="aviso-motor idioma-superficie" role="note" {...superficieIdioma(idioma)}>{tradutor(idioma)(aviso)}</p> : null
}
