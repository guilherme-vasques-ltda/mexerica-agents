import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readPicked, readPasted } from './fork-composer-files.ts'

const file = (name: string, bytes: number[]) => new File([new Uint8Array(bytes)], name)

test('prefere nativo sem duplicar e usa captura quando vazio ou falha', async () => {
  const captured = [file('a', [1, 2])]
  const native = [{ name: 'native', data: new Uint8Array([3]) }]
  assert.deepEqual(await readPasted(captured, async () => native), native)
  for (const read of [async () => [], async () => { throw Error('IPC') }]) {
    assert.deepEqual(await readPasted(captured, read), [{ name: 'a', data: new Uint8Array([1, 2]) }])
  }
})

test('ignora item ilegível e conserva ordem e bytes dos demais', async () => {
  const bad = file('pasta', [])
  bad.arrayBuffer = async () => { throw Error('ilegível') }
  assert.deepEqual(await readPicked([file('a', [0, 255]), bad, file('b', [2])]), [
    { name: 'a', data: new Uint8Array([0, 255]) }, { name: 'b', data: new Uint8Array([2]) },
  ])
  assert.match((await readPicked([file('', [9])]))[0]!.name, /^pasted-\d+\.png$/)
})
