// D432 (07/11, docs/reference/design/barrinha-novas-20261107): com `?longa`, o Crivo ganha uma conversa longa
// fictícia (34 mensagens, você e o bot alternando; as 3 últimas respostas não lidas). Só demo.
export const longaDemo = () => typeof location !== 'undefined' && new URLSearchParams(location.search).has('longa')

const PERGUNTAS = [
  'Can you review the weeklyTotals diff?', 'Is the ISO week math right at the year boundary?', 'What about runs without a time zone?',
  'Should we clamp negative distances?', 'Does the test cover a 53-week year?', 'Any perf concern with 10k runs?',
  'Is the helper name clear enough?', 'Should totals round to one decimal?', 'Do we need a changelog line?',
  'Can you check the empty-input case?', 'Is the README example still valid?', 'Anything blocking the merge?',
  'Should pace use minutes per km or per mile?', 'Can you re-run the suite after the rebase?', 'Is the lint warning real?',
  'Ready to ship?', 'One last look at the docs?',
]
const RESPOSTAS = [
  'Looks good overall. One nit: the loop can use `reduce`, but it is fine as is.\nNo blockers.',
  'Yes. Week 1 is the week with the first Thursday, and the test for 2026→2027 proves it.',
  'They are read as UTC. Worth a comment at the top of `weeklyTotals` so nobody is surprised.',
  'I would reject them instead of clamping: a negative distance is a data bug, not a run.',
  'It does now: 2026 has 53 weeks and the last one sums correctly.',
  'No. It is a single pass, about 3 ms for 10k runs on my side.',
  'Clear enough. `weeklyTotals(runs)` says what it returns.',
  'Keep the raw numbers in the data and round only when displaying.',
  'Yes, one line: "Pace now shows your weekly distance and time totals."',
  'Empty input returns an empty list, and there is a test for it.',
  'The example still runs. I updated the output to the new shape.',
  'Nothing blocking. Two small comments, both optional.',
  'Minutes per km by default, with the unit in settings.',
  'Re-ran after the rebase: 48 passed, 0 failed.',
  'It is real: an unused import in `pace.test.js`. Removed.',
  'Ready. All checks are green.',
  'Docs read well. I fixed one typo in the example.',
]

export function entradasLongas(now: number) {
  return PERGUNTAS.flatMap((q, i) => {
    const at = now - (PERGUNTAS.length - i) * 7 * 60_000
    return [
      { id: `longa-u${i}`, kind: 'user', at, data: { text: q, status: 'sent' } },
      { id: `longa-a${i}`, kind: 'agent', at: at + 60_000, data: { text: RESPOSTAS[i]!, final: true, author: 'crivo' } },
    ]
  })
}
