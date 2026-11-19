// D371 (T1): o campo de regras (geral em Ajustes → Delegação; de uma área na folha de Equipes), com contador,
// Salvo/Não salvo e Tentar de novo. O modelo (regras.ts) fala com o Hub e devolve chaves; aqui só a apresentação.
import { useEffect, useId } from 'react'
import { Button } from '../../../components/Controls'
import { formatarNumero } from '../../../lib/fork-i18n/index'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import { CampoRegras as Modelo, contar, LIMITE, type RegrasLink } from './regras'
import './delegacao.css'

/** `guardarComo`: a chave do rascunho em Ajustes; fora de Ajustes o modelo é novo a cada montagem, como antes. */
export function CampoRegras({ link, teamId, guardarComo = 'avulso' }: { link: RegrasLink; teamId: string | null; guardarComo?: string }) {
  // O modelo guarda o texto editado: fica no pai, para a página voltar com o rascunho (estado-pagina.ts).
  const [modelo] = useEstadoDaPagina(`delegacao/regras/${guardarComo}`, () => new Modelo(link, teamId))
  const campo = useModel(modelo)
  const id = useId()
  const t = useT()
  const idioma = useIdioma()
  // Com rascunho sujo do modelo que já leu, a releitura do Hub não apaga o texto digitado.
  useEffect(() => { void modelo.ler(modelo.salvo !== null && modelo.sujo) }, [modelo])
  const alerta = campo.fase === 'conflito' || campo.fase === 'falha' || campo.excede
  return (
    <div className="delegacao-campo idioma-superficie" {...superficieIdioma(idioma)}>
      <label htmlFor={id} className="delegacao-titulo">{t(teamId ? 'delegacao.equipe.titulo' : 'delegacao.regras.titulo')}</label>
      <p className="delegacao-ajuda">{t(teamId ? 'delegacao.equipe.ajuda' : 'delegacao.regras.ajuda')}</p>
      <textarea id={id} className="delegacao-texto" dir="auto" value={campo.texto} placeholder={t(teamId ? 'delegacao.equipe.exemplo' : 'delegacao.regras.exemplo')} rows={7}
        disabled={campo.fase === 'lendo'} onChange={(e) => modelo.editar(e.target.value)} aria-describedby={`${id}-estado`} />
      <div className="delegacao-rodape">
        <span id={`${id}-estado`} className={alerta ? 'delegacao-alerta' : undefined} role={alerta ? 'alert' : undefined}>{t(campo.estado)}</span>
        <span className={`delegacao-contador${campo.excede ? ' delegacao-alerta' : ''}`}>
          {t('delegacao.contador', { n: formatarNumero(idioma, contar(campo.texto)), max: formatarNumero(idioma, LIMITE) })}
        </span>
      </div>
      <div className="delegacao-acoes">
        {campo.podeRepetir ? <Button kind="secondary" onClick={() => void modelo.tentarDeNovo()}>{t('delegacao.acao.tentarDeNovo')}</Button> : null}
        <Button disabled={!campo.sujo || campo.excede || campo.fase !== 'pronto'} onClick={() => void modelo.salvar()}>{t('delegacao.acao.salvar')}</Button>
      </div>
    </div>
  )
}
