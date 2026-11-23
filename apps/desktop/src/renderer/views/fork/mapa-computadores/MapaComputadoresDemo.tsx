// GUI-467: demonstração do mapa de computadores com dados fictícios. Só aparece com a flag do gancho ligada;
// não oferece adicionar, conectar nem revogar, e não consulta nenhum computador real.
import { useMemo, useState } from 'react'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { fixturas } from './fixtures.ts'
import { filtrar, projetarAmostra, projetarFatos, type Computador, type Filtro } from './modelo.ts'
import './mapa-computadores.css'

const FILTROS: readonly Filtro[] = ['todos', 'tunel_em_uso', 'sem_autorizacao']
const CHAVE_FILTRO = { todos: 'mapaComputadores.filtro.todos', tunel_em_uso: 'mapaComputadores.filtro.tunel', sem_autorizacao: 'mapaComputadores.filtro.semAutorizacao' } as const
const CHAVE_FATO = { rota: 'mapaComputadores.fato.rota', tunel: 'mapaComputadores.fato.tunel', identidade: 'mapaComputadores.fato.identidade', autorizacao: 'mapaComputadores.fato.autorizacao' } as const

export function MapaComputadoresDemo() {
  const t = useT()
  const idioma = useIdioma()
  const [agora] = useState(() => Date.now())
  const computadores = useMemo(() => fixturas(agora), [agora])
  const [filtro, setFiltro] = useState<Filtro>('todos')
  const [selecionadoId, setSelecionadoId] = useState('esta')
  const visiveis = filtrar(computadores, filtro)
  const selecionado = computadores.find((c) => c.id === selecionadoId) ?? computadores[0]

  return (
    <section className="mapa-computadores idioma-superficie" aria-labelledby="mapa-computadores-titulo" {...superficieIdioma(idioma)}>
      <h3 id="mapa-computadores-titulo" className="mapa-computadores-titulo">{t('mapaComputadores.titulo')}</h3>
      <p className="mapa-computadores-aviso">{t('mapaComputadores.aviso')}</p>

      <div role="group" aria-label={t('mapaComputadores.filtro.rotulo')} className="mapa-computadores-filtros">
        {FILTROS.map((f) => (
          <button key={f} type="button" aria-pressed={filtro === f} onClick={() => setFiltro(f)}>
            {t(CHAVE_FILTRO[f])}
          </button>
        ))}
      </div>

      <div role="group" aria-label={t('mapaComputadores.mapa')} className="mapa-computadores-mapa">
        {visiveis.map((c) => (
          <button
            key={c.id}
            type="button"
            className="mapa-computadores-ponto"
            style={{ left: `${c.posicao.x}%`, top: `${c.posicao.y}%` }}
            aria-label={t('mapaComputadores.selecionar', { nome: t(c.nomeChave) })}
            aria-pressed={selecionado?.id === c.id}
            onClick={() => setSelecionadoId(c.id)}
          >
            {t(c.nomeChave)}
          </button>
        ))}
      </div>

      <ul aria-label={t('mapaComputadores.lista')} className="mapa-computadores-lista">
        {visiveis.length === 0 ? <li>{t('mapaComputadores.listaVazia')}</li> : null}
        {visiveis.map((c) => (
          <li key={c.id}>
            <button type="button" aria-pressed={selecionado?.id === c.id} onClick={() => setSelecionadoId(c.id)}>
              <span>{t(c.nomeChave)}</span>
              <span>{t(papelDe(c))}</span>
              {projetarFatos(c.fatos).map((f) => (
                <span key={f.fato} data-tom={f.tom}>
                  <span>{t(CHAVE_FATO[f.fato])}</span>
                  <strong>{t(`mapaComputadores.est.${f.fato}.${f.estado}` as 'mapaComputadores.est.rota.configurada')}</strong>
                </span>
              ))}
            </button>
          </li>
        ))}
      </ul>

      {selecionado ? <Detalhes computador={selecionado} agora={agora} /> : null}
      <p className="mapa-computadores-aviso">{t('mapaComputadores.semAcoes')}</p>
    </section>
  )
}

function papelDe(c: Computador) {
  return c.papel === 'esta' ? 'mapaComputadores.papel.esta' : 'mapaComputadores.papel.demo'
}

function Detalhes({ computador, agora }: { computador: Computador; agora: number }) {
  const t = useT()
  const amostra = projetarAmostra(computador.amostra, agora)
  return (
    <div className="mapa-computadores-detalhes" role="group" aria-label={t('mapaComputadores.detalhes', { nome: t(computador.nomeChave) })}>
      <h4>{t(computador.nomeChave)}</h4>
      <dl>
        {projetarFatos(computador.fatos).map((f) => (
          <div key={f.fato} data-tom={f.tom}>
            <dt>{t(CHAVE_FATO[f.fato])}</dt>
            <dd>{t(`mapaComputadores.est.${f.fato}.${f.estado}` as 'mapaComputadores.est.rota.configurada')}</dd>
          </div>
        ))}
      </dl>
      <h5>{t('mapaComputadores.amostra.titulo')}</h5>
      {amostra.estado === 'fresca' ? (
        <dl>
          <div><dt>{t('mapaComputadores.amostra.cpu')}</dt><dd>{t('mapaComputadores.amostra.valor', { valor: amostra.cpu })}</dd></div>
          <div><dt>{t('mapaComputadores.amostra.memoria')}</dt><dd>{t('mapaComputadores.amostra.valor', { valor: amostra.memoria })}</dd></div>
          <div><dt>{t('mapaComputadores.amostra.disco')}</dt><dd>{t('mapaComputadores.amostra.valor', { valor: amostra.disco })}</dd></div>
          {amostra.alertaDisco ? <p>{t('mapaComputadores.alerta.disco')}</p> : null}
        </dl>
      ) : (
        <p>{t(`mapaComputadores.amostra.${amostra.estado}` as 'mapaComputadores.amostra.obsoleta')}</p>
      )}
    </div>
  )
}
