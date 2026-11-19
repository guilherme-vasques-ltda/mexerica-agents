/** Apenas a prévia, com a ponte fictícia já instalada, pode montar o portão com conta simulada. */
export function showsDemoAccountControls(): boolean {
  return Object.hasOwn(window, 'donnabotEditorDemo')
}
