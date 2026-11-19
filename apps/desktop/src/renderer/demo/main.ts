// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
// The website demo: the real renderer on a fake host. The bridge must load first.
import './bridge'
import '../main'
// O app junta todo o CSS num arquivo (electron.vite.config.ts, cssCodeSplit: false); no demo, o CSS das telas sob
// demanda (sob-demanda/telas.tsx) só carregaria ao abrir, e as camadas delas (.rc-host, .times-host, .torre-host)
// ficariam sem altura, encolhendo a janela. Carrega já, como no app.
import '../views/rotinas-central/routines-central.css'
import '../views/fork/times/times.css'
import '../views/fork/torre/torre.css'

// Entrega 3 da revisão do app: `?ajustes=<página>` abre Ajustes direto na página (para as capturas das propostas).
const ajustes = new URLSearchParams(location.search).get('ajustes')
if (ajustes) void import('../views/fork/ajustes/tela').then(({ settingsScreen }) => setTimeout(() => settingsScreen.show(ajustes as Parameters<typeof settingsScreen.show>[0]), 1200))
